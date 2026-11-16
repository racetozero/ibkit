use crate::platform;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use ibkit_gateway_core::recovery::{Recovery, RecoveryState};
use ibkit_gateway_core::{
    AgentLaunch, AgentStatus, Command as AgentCommand, Config, Control, Credentials, ErrorCode,
    Event, EventKind, StartResult, State, Status,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::{Child, Command},
    sync::broadcast,
    time::{sleep, timeout},
};

pub struct Gateway {
    config: Config,
    credentials: Credentials,
    status: Status,
    child: Option<Child>,
    display: Option<(Child, String)>,
    _lock: File,
    events: broadcast::Sender<Event>,
    offsets: HashMap<u32, u64>,
    known_ready: HashSet<u32>,
    started: Instant,
    restart_deadline: Option<Instant>,
    restart_count: u32,
    restart_epoch: Instant,
    agent_alias: Option<tempfile::TempDir>,
    api_checked: Instant,
    api_failed_since: Option<Instant>,
    active_pid: Option<u32>,
    closed_agents: HashSet<u32>,
    reload: bool,
    heap_checked: HashSet<u32>,
    /// When the heartbeat of the active Gateway JVM stopped (a stall, a
    /// crash, or the daily restart handoff); cleared when it reports again.
    agent_lost: Option<Instant>,
    /// When the launcher exited before Ready with nothing of it left
    /// running.
    launcher_gone: Option<Instant>,
    /// Process groups of this session's Gateway processes: the launcher
    /// group that this controller started, and the groups of fresh agents
    /// of this session. Never this controller's own group.
    groups: HashSet<u32>,
    /// The scheduled reset window whose hold was reported.
    window_noted: Option<DateTime<Utc>>,
    /// x11vnc serving the virtual display (`vnc_listen`).
    vnc: Option<Child>,
    /// The TOTP secret of the running configuration, to see a changed
    /// secret file on reload.
    totp: Option<Vec<u8>>,
}

/// How long nothing of a session may run before the controller reports a
/// crash. Gateway's daily restart child appears within this time.
const PROCESS_GONE_GRACE: Duration = Duration::from_secs(120);

impl Gateway {
    pub fn new(mut config: Config) -> Result<Self> {
        let credentials = check_config(&mut config)?;
        let unwritable = || {
            format!(
                "Cannot create or use state_dir {}. Use a directory that this user ({}) can write, for example a volume mounted with that owner, or set state_dir.",
                config.state_dir.display(),
                platform::user()
            )
        };
        platform::private_dir(&config.state_dir).with_context(unwritable)?;
        let lock = platform::private_file(
            OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true),
        )
        .open(config.state_dir.join("controller.lock"))?;
        // A child that another thread forks holds a copy of the lock until it
        // calls exec, and a controller that just exited may still close it,
        // so wait up to 2 seconds before reporting a second controller.
        let deadline = Instant::now() + Duration::from_secs(2);
        while let Err(error) = lock.try_lock() {
            if Instant::now() >= deadline {
                return Err(error).context("Another controller (`ibkit gateway run` or the TUI) holds the lock of this state_dir. Stop it first: run, reset-recovery and orphan cleanup each need the lock. Or select a different state_dir.");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // Commands queued for an earlier controller do not apply to this one.
        // A container restart reuses the controller PID, so the tag alone
        // cannot tell them apart.
        match fs::remove_dir_all(config.state_dir.join(CONTROL_QUEUE)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error).context("Cannot clear the command queue of an earlier controller. Make state_dir/control writable, or remove it, and start again.");
            }
            _ => {}
        }
        platform::private_dir(&config.state_dir.join("agents"))?;
        prepare_settings(&config)?;
        let (events, _) = broadcast::channel(256);
        let totp = config.totp_key()?;
        Ok(Self {
            config,
            credentials,
            status: Status {
                session: uuid::Uuid::new_v4().to_string(),
                controller_pid: std::process::id(),
                at: Utc::now(),
                state: State::Exited,
                agents: Vec::new(),
                last_start_result: StartResult::new(ErrorCode::None),
                recovery: None,
                process_group: None,
            },
            child: None,
            display: None,
            _lock: lock,
            events,
            offsets: HashMap::new(),
            known_ready: HashSet::new(),
            started: Instant::now(),
            restart_deadline: None,
            restart_count: 0,
            restart_epoch: Instant::now(),
            agent_alias: None,
            api_checked: Instant::now(),
            api_failed_since: None,
            active_pid: None,
            closed_agents: HashSet::new(),
            reload: false,
            heap_checked: HashSet::new(),
            agent_lost: None,
            launcher_gone: None,
            groups: HashSet::new(),
            window_noted: None,
            vnc: None,
            totp,
        })
    }

    /// This controller's event stream: call `subscribe` on it for a
    /// receiver, once per reader.
    pub fn events(&self) -> broadcast::Sender<Event> {
        self.events.clone()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// True once after a reload command; the caller then reads the
    /// configuration again and calls `reconfigure`.
    pub fn take_reload(&mut self) -> bool {
        std::mem::take(&mut self.reload)
    }

    /// Ask for a reload as the `reload` command does (SIGHUP).
    pub fn request_reload(&mut self) {
        self.reload = true;
    }

    /// The scheduled weekly cold restart (`cold_restart_at`): stop Gateway
    /// and log in again. Only a running Gateway restarts.
    pub async fn cold_restart(&mut self, recovery: &mut Recovery) -> Result<()> {
        if !self.is_running() || matches!(self.status.state, State::Recovering | State::Paused) {
            return Ok(());
        }
        self.emit(EventKind::Output {
            message: "Scheduled cold restart (cold_restart_at): Gateway logs in again.".into(),
        });
        self.stop().await?;
        self.relaunch(recovery).await
    }

    pub fn status(&self) -> &Status {
        &self.status
    }
    pub fn last_start_result(&self) -> &StartResult {
        &self.status.last_start_result
    }
    pub fn is_running(&self) -> bool {
        !matches!(
            self.status.state,
            State::Exited | State::Failed | State::Halted | State::Paused
        )
    }

    pub fn is_authenticating(&self) -> bool {
        matches!(
            self.status.state,
            State::Starting | State::LoggingIn | State::AwaitingSecondFactor
        )
    }

    pub fn export_logs(&self) -> Result<()> {
        self.command_agents(Control::ExportLogs)
    }

    /// Emit an `output` event (for the run loop's own messages).
    pub fn emit_output(&self, message: &str) {
        self.emit(EventKind::Output {
            message: message.into(),
        });
    }

    fn emit(&self, kind: EventKind) {
        let _ = self.events.send(Event {
            at: Utc::now(),
            session: self.status.session.clone(),
            pid: std::process::id(),
            kind,
        });
    }

    fn save(&mut self) -> Result<()> {
        self.status.at = Utc::now();
        atomic_json(&self.config.state_dir.join("status.json"), &self.status)
    }

    fn save_recovery(&mut self, recovery: &Recovery) -> Result<()> {
        recovery.validate()?;
        self.status.recovery = Some(recovery.state);
        let path = self.config.state_dir.join("recovery.json");
        atomic_json(&path, recovery)?;
        File::open(path)?.sync_all()?;
        File::open(&self.config.state_dir)?.sync_all()?;
        Ok(())
    }

    /// Start a managed session, or resume its saved recovery wait and budget.
    /// Explicit launch/start calls remain manual operations.
    pub async fn resume(&mut self, recovery: &mut Recovery) -> Result<State> {
        if self.is_running() || self.child.is_some() {
            bail!(
                "You resumed an active controller. Poll it instead, or stop it before you resume."
            );
        }
        *recovery = match fs::read(self.config.state_dir.join("recovery.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("The saved recovery file is invalid. Keep it for review, correct the failure, and use reset-recovery with the controller stopped.")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Recovery::default(),
            Err(error) => return Err(error).context("Cannot read the saved recovery file. Use a readable private state directory and correct its permissions before you start again."),
        };
        recovery.validate()?;
        self.status.recovery = Some(recovery.state);
        if self.config.state_dir.join(PAUSED).exists() {
            self.status.state = State::Paused;
            self.emit(EventKind::Output {
                message: "Gateway is paused (pause or closedown_at). Send restart to log in again."
                    .into(),
            });
            self.save()?;
            return Ok(State::Paused);
        }
        let mut interrupted_reason = ErrorCode::AgentDisconnected;
        let previous = match fs::read(self.config.state_dir.join("status.json")) {
            Ok(bytes) => Some(serde_json::from_slice::<Status>(&bytes).context("The previous controller status is invalid. Review the private state files before you start again.")?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("Cannot read the previous controller status. Restore readable status.json permissions before you start again."),
        };
        if previous.is_none() && !matches!(recovery.state, RecoveryState::Idle) {
            bail!(
                "A managed session has no saved ownership status. Review the previous Gateway process and restore status.json, or close it manually before reset-recovery."
            );
        }
        if let Some(previous) = previous {
            if previous.last_start_result.has_error() {
                interrupted_reason = previous.last_start_result.error_code;
            }
            let reports = read_agent_reports(&self.config.state_dir, &previous.session)?;
            self.status.session.clone_from(&previous.session);
            if let Some(agent) = reports
                .iter()
                .filter(|agent| {
                    agent.result.has_error() && !Recovery::is_retryable(agent.result.error_code)
                })
                .max_by_key(|agent| agent.at)
            {
                interrupted_reason = agent.result.error_code;
            }
            if !matches!(
                recovery.state,
                RecoveryState::Idle | RecoveryState::Blocked { .. }
            ) && !Recovery::is_retryable(interrupted_reason)
            {
                recovery.state = RecoveryState::Blocked {
                    reason: interrupted_reason,
                };
                self.save_recovery(recovery)?;
            }
            let agents = read_agents(&self.config.state_dir, &previous.session)?;
            // A controller that stopped without closing its Gateway (crash,
            // kill, host supervisor restart) leaves a live session. Keep it:
            // a new login would log the account out and back in.
            if let Some((pid, state)) = agents
                .iter()
                .filter(|agent| agent.state != State::Failed)
                .max_by_key(|agent| agent.at)
                .map(|agent| (agent.pid, agent.state))
                && matches!(recovery.state, RecoveryState::Idle | RecoveryState::Running)
                && !matches!(
                    previous.state,
                    State::Exited | State::Stopping | State::Failed | State::Halted | State::Paused
                )
            {
                for agent in &agents {
                    let events = self
                        .config
                        .state_dir
                        .join("agents")
                        .join(format!("{}.events", agent.pid));
                    self.offsets
                        .insert(agent.pid, fs::metadata(events).map_or(0, |meta| meta.len()));
                }
                if state == State::Ready {
                    self.known_ready.insert(pid);
                }
                self.active_pid = Some(pid);
                // On Windows the session's job can be reopened by its name.
                if let Some(group) = previous
                    .process_group
                    .and_then(|group| platform::reopen_group(group, &previous.session))
                {
                    self.groups.insert(group);
                    self.status.process_group = Some(group);
                }
                self.status.agents = agents;
                self.status.state = state;
                self.status.last_start_result = StartResult::new(ErrorCode::None);
                self.started = Instant::now();
                self.api_checked = Instant::now();
                recovery.retry_started();
                self.save_recovery(recovery)?;
                self.emit(EventKind::Output {
                    message: format!(
                        "Adopted the running Gateway (agent pid {pid}, state {state:?}) of the previous controller; no new login is needed."
                    ),
                });
                self.save()?;
                return Ok(state);
            }
            // A launcher group that no longer exists (a new container, or a
            // kill of the whole group) cannot report: retry normally.
            let launcher_may_run = previous
                .process_group
                .is_none_or(|group| platform::group_alive(group, &previous.session));
            if !recovery.budget_exhausted(&self.config, Utc::now())
                && agents.is_empty()
                && launcher_may_run
                && (matches!(recovery.state, RecoveryState::AwaitingAgent { .. })
                    || matches!(recovery.state, RecoveryState::Running)
                        && !matches!(
                            previous.state,
                            State::Exited | State::Failed | State::Halted | State::Paused
                        )
                        && reports.is_empty())
            {
                if !matches!(recovery.state, RecoveryState::AwaitingAgent { .. }) {
                    recovery.state = RecoveryState::AwaitingAgent {
                        deadline: Utc::now()
                            + chrono::Duration::seconds(
                                self.config.initialization_timeout_secs as i64,
                            ),
                    };
                    self.save_recovery(recovery)?;
                }
                self.status.state = State::Recovering;
                self.status.last_start_result = StartResult::new(ErrorCode::AgentDisconnected);
                self.save()?;
                return Ok(State::Recovering);
            }
            if !agents.is_empty() {
                self.status.agents = agents;
                self.status.last_start_result = StartResult::new(interrupted_reason);
                self.stop().await?;
            }
        }
        match recovery.state {
            RecoveryState::Idle => {
                recovery.retry_started();
                self.save_recovery(recovery)?;
                self.launch().await?;
            }
            RecoveryState::Running
            | RecoveryState::Exhausted
            | RecoveryState::AwaitingAgent { .. } => {
                self.set_error(interrupted_reason);
                if !self.recover(recovery).await? {
                    return self.halt(recovery).await;
                }
            }
            RecoveryState::Waiting { reason, .. } => {
                if !self.config.auto_recover {
                    self.set_error(reason);
                    return self.halt(recovery).await;
                }
                self.status.state = State::Recovering;
                self.status.last_start_result = StartResult::new(reason);
                self.save()?;
            }
            RecoveryState::Blocked { reason } => {
                self.set_error(reason);
                return self.halt(recovery).await;
            }
        }
        Ok(self.status.state)
    }

    /// Stop Gateway and keep the controller up after a fault that automatic
    /// recovery must not retry. The status heartbeat continues, and
    /// `poll_halted` serves stop, restart and reload commands. A container
    /// restart policy therefore cannot turn the fault into a login loop.
    pub async fn halt(&mut self, recovery: &Recovery) -> Result<State> {
        let mut result = self.status.last_start_result.clone();
        if let RecoveryState::Blocked {
            reason: reason @ ErrorCode::LoginRetriesExhausted,
        } = recovery.state
        {
            result = StartResult::new(reason);
        }
        self.stop().await?;
        self.status.state = State::Halted;
        self.status.last_start_result = result;
        self.status.recovery = Some(recovery.state);
        let exhausted = matches!(recovery.state, RecoveryState::Exhausted);
        let next = if exhausted {
            "Automatic recovery continues when the hourly attempt budget permits; send restart to log in now, or stop to exit."
        } else {
            "Correct the cause, then send restart to log in again, or stop to exit."
        };
        self.emit(EventKind::Halted {
            reason: self.status.last_start_result.error_code,
            exhausted,
            message: format!(
                "Gateway is halted: {} {next}",
                self.status.last_start_result.error_message
            ),
        });
        self.save()?;
        Ok(State::Halted)
    }

    /// Serve a halted controller. Returns `Exited` after a stop command,
    /// `Halted` while halted, or the new state after a restart.
    pub async fn poll_halted(&mut self, recovery: &mut Recovery) -> Result<State> {
        if matches!(recovery.state, RecoveryState::Exhausted)
            && !recovery.budget_exhausted(&self.config, Utc::now())
            && self.recover(recovery).await?
        {
            return Ok(self.status.state);
        }
        // A JVM of this session that still runs (a late restart child)
        // holds the API port and the IBKR session: close it.
        let agents = read_agents(&self.config.state_dir, &self.status.session)?;
        if !agents.is_empty()
            || self
                .groups
                .iter()
                .any(|group| platform::group_alive(*group, &self.status.session))
        {
            self.emit(EventKind::Output {
                message: "A Gateway process of the halted session still runs; closing it.".into(),
            });
            self.status.agents = agents;
            // Keep the error that halted the session.
            let last = self.status.last_start_result.clone();
            let result = self.stop().await;
            self.status.last_start_result = last;
            self.report_command_error(Control::Stop, result);
            self.status.state = State::Halted;
        }
        match self.take_control()? {
            Some(Control::Stop) => {
                self.status.state = State::Exited;
                self.status.last_start_result = StartResult::new(ErrorCode::None);
                self.save()?;
                return Ok(State::Exited);
            }
            Some(Control::Restart) => {
                let result = self.relaunch(recovery).await;
                self.report_command_error(Control::Restart, result);
                return Ok(self.status.state);
            }
            Some(Control::Pause) => {
                self.pause().await?;
                return Ok(self.status.state);
            }
            Some(Control::Reload) => self.reload = true,
            Some(_) => self.emit(EventKind::Output {
                message: "Gateway is halted. Send restart to log in again, reload to apply a changed configuration and log in, pause, or stop to exit.".into(),
            }),
            None => {}
        }
        self.save()?;
        Ok(self.status.state)
    }

    /// Apply a new configuration: Gateway stops and a new session starts
    /// with it. On an error the current configuration and session stay.
    pub async fn reconfigure(&mut self, mut config: Config, recovery: &mut Recovery) -> Result<()> {
        let credentials = check_config(&mut config)?;
        if config.state_dir != self.config.state_dir {
            bail!(
                "You changed state_dir in a reload. The controller lock and recovery journal belong to the current state_dir; keep it unchanged, or stop the controller and start it with the new state_dir."
            );
        }
        prepare_settings(&config)?;
        let mut changed = changed_keys(&self.config, &config);
        // PUT /v1/credentials and PUT /v1/totp keep the file paths and change
        // the contents: a new login must use them.
        let totp = config.totp_key()?;
        if credentials.username != self.credentials.username
            || credentials.password != self.credentials.password
            || totp != self.totp
        {
            changed.push("credentials".into());
        }
        // A halted controller logs in on reload: the operator corrected the
        // cause (for example the password after LoginFailed).
        let restart = self.status.state == State::Halted
            || changed
                .iter()
                .any(|key| !CONTROLLER_KEYS.contains(&key.as_str()));
        if changed.iter().any(|key| key.starts_with("control_")) {
            self.emit(EventKind::Output {
                message:
                    "control_listen and the control token apply after the controller restarts."
                        .into(),
            });
        }
        if !restart || matches!(self.status.state, State::Recovering | State::Paused) {
            // No Gateway setting changed, or no Gateway runs: no new login.
            // A pending retry keeps its wait and then uses the new settings.
            self.config = config;
            self.credentials = credentials;
            self.totp = totp;
            self.emit(EventKind::Output {
                message: format!(
                    "Applied the new settings without a new login (changed: {}).",
                    if changed.is_empty() {
                        "none".into()
                    } else {
                        changed.join(", ")
                    }
                ),
            });
            return Ok(());
        }
        self.stop().await?;
        self.config = config;
        self.credentials = credentials;
        self.totp = totp;
        self.relaunch(recovery).await
    }

    /// Start a new login session as a recovery attempt.
    async fn relaunch(&mut self, recovery: &mut Recovery) -> Result<()> {
        recovery.retry_started();
        self.save_recovery(recovery)?;
        // `launch` refuses an active state such as Recovering.
        self.status.state = State::Exited;
        self.launch().await
    }

    /// The oldest command queued for this controller, if any. Each command
    /// is one file in `control/`, named so that names sort in send order; a
    /// command for an earlier controller is dropped. Reloads in a row count
    /// as one: the settings that they saved are read once.
    fn take_control(&self) -> Result<Option<Control>> {
        let directory = self.config.state_dir.join(CONTROL_QUEUE);
        let mut names: Vec<_> = match fs::read_dir(&directory) {
            Ok(entries) => entries
                .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
                .filter(|name| {
                    let name = name.to_string_lossy();
                    !name.starts_with('.') && name.ends_with(".json")
                })
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        names.sort();
        let mut taken = None;
        for name in names {
            let path = directory.join(name);
            // An entry that cannot be read or parsed is dropped, as a stale one.
            let queued = fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<QueuedControl>(&bytes).ok())
                .filter(|queued| queued.controller_pid == self.status.controller_pid);
            match (taken, queued) {
                (None, None) => {}
                (None, Some(queued)) => taken = Some(queued.control),
                (
                    Some(Control::Reload),
                    Some(QueuedControl {
                        control: Control::Reload,
                        ..
                    }),
                ) => {}
                (Some(_), _) => break,
            }
            fs::remove_file(&path)?;
        }
        Ok(taken)
    }

    pub fn cancel_recovery(&mut self, recovery: &mut Recovery) -> Result<()> {
        let reason = self.status.last_start_result.error_code;
        if matches!(recovery.state, RecoveryState::AwaitingAgent { .. }) {
            recovery.state = RecoveryState::Blocked {
                reason: ErrorCode::AgentDisconnected,
            };
            self.save_recovery(recovery)?;
            self.emit(EventKind::Output { message: "The previous Gateway has not reported its agent. Close that session manually and review it before reset-recovery; another automatic login is blocked.".into() });
            return Ok(());
        }
        if reason != ErrorCode::None && !Recovery::is_retryable(reason) {
            recovery.state = RecoveryState::Blocked { reason };
            return self.save_recovery(recovery);
        }
        if matches!(
            recovery.state,
            RecoveryState::Running | RecoveryState::Waiting { .. }
        ) {
            recovery.state = RecoveryState::Idle;
            self.save_recovery(recovery)?;
        }
        Ok(())
    }

    /// The controller lock held by this Gateway excludes a concurrent run.
    pub fn reset_recovery(&self) -> Result<()> {
        if self.is_running() || self.child.is_some() {
            bail!(
                "You reset recovery while Gateway was active. Stop the controller, correct the failure, and run reset-recovery again."
            );
        }
        let previous = match fs::read(self.config.state_dir.join("status.json")) {
            Ok(bytes) => Some(serde_json::from_slice::<Status>(&bytes).context("The saved status is invalid. Review and repair it before you reset recovery.")?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("Cannot read the saved controller status. Restore readable status.json permissions before you reset recovery."),
        };
        if let Some(status) = previous
            && !read_agents(&self.config.state_dir, &status.session)?.is_empty()
        {
            bail!(
                "You reset recovery while a Gateway agent was still active. Close that session and wait for its heartbeat to stop before you reset recovery."
            );
        }
        match fs::remove_file(self.config.state_dir.join("recovery.json")) {
            Ok(()) => File::open(&self.config.state_dir)?.sync_all()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(error) => return Err(error).context("Cannot reset the saved recovery policy. Use a writable private state directory and correct its permissions."),
        }
        Ok(())
    }

    /// Close agents left by a stopped controller. Construction holds the same
    /// exclusive lock as normal startup; this method never starts a login.
    pub async fn stop_existing(&mut self) -> Result<()> {
        if self.is_running() || self.child.is_some() {
            bail!(
                "You requested orphan cleanup from an active controller. Use stop for the active session instead."
            );
        }
        let previous = read_status(&self.config.state_dir)?;
        let agents = read_agents(&self.config.state_dir, &previous.session)?;
        if agents.is_empty() && previous.state != State::Exited {
            bail!(
                "The previous Gateway has no fresh agent report. Close its desktop session manually and review recovery before you start again; process ownership is unknown."
            );
        }
        self.status.session = previous.session;
        self.status.agents = agents;
        self.status.last_start_result = previous.last_start_result;
        self.stop().await
    }

    /// Start a new Gateway session. A failure before the launcher runs (the
    /// display, VNC, a state file) is a failed session, so recovery applies
    /// its back-off instead of waiting for an agent that cannot come.
    pub async fn launch(&mut self) -> Result<()> {
        if self.is_running() || self.child.is_some() {
            bail!("You started an active Gateway again. Stop it first or call restart.");
        }
        let result = self.launch_session().await;
        if let Err(error) = &result
            && self.child.is_none()
            && self.status.state == State::Starting
        {
            self.status.state = State::Failed;
            self.status.last_start_result = StartResult {
                error_code: ErrorCode::ProcessStartFailed,
                error_message: format!(
                    "Gateway could not be started: {error:#}. Correct the cause; recovery retries."
                ),
            };
            self.emit(EventKind::Error {
                result: self.status.last_start_result.clone(),
            });
            let _ = self.save();
        }
        result
    }

    async fn launch_session(&mut self) -> Result<()> {
        self.status.state = State::Starting;
        self.status.last_start_result = StartResult::new(ErrorCode::None);
        self.status.session = uuid::Uuid::new_v4().to_string();
        self.known_ready.clear();
        self.offsets.clear();
        self.restart_deadline = None;
        self.api_checked = Instant::now();
        self.api_failed_since = None;
        self.status.agents.clear();
        self.active_pid = None;
        self.closed_agents.clear();
        self.agent_lost = None;
        self.launcher_gone = None;
        self.groups.clear();
        self.window_noted = None;
        prune_agent_files(&self.config.state_dir);
        let _ = fs::remove_file(self.config.state_dir.join("launch.json"));
        let launch = AgentLaunch {
            session: self.status.session.clone(),
            config: self.config.clone(),
        };
        let path = self.config.state_dir.join("launch.json");
        atomic_json(&path, &launch)?;
        let mut command = Command::new(&self.config.executable);
        command.current_dir(
            self.config
                .executable
                .parent()
                .context("The launcher path has no parent directory")?,
        );
        // install4j supports -J options and passes them to the JVM. The inherited
        // JAVA_TOOL_OPTIONS also covers Gateway's daily restart child JVM.
        // install4j parses java -version output during runtime selection. A
        // quoted JAVA_TOOL_OPTIONS entry can be mistaken for the version text.
        // Pass config through the environment and alias a spaced library path.
        let (agent_path, alias) = platform::agent_path(&self.config.agent)?;
        self.agent_alias = alias;
        let mut agent_option = format!("-agentpath:{}=env", agent_path.display());
        if cfg!(target_os = "linux") {
            // The Java ATK accessibility bridge can deadlock Swing during
            // login when installed; the agent does not use it. Inherited by
            // Gateway's restart child.
            agent_option.push_str(" -Djavax.accessibility.assistive_technologies=");
        }
        command.env("IBKIT_AGENT_CONFIG", &path);
        // Gateway has no use for the control API token.
        command.env_remove(&self.config.control_token_env);
        let inherited = std::env::var("JAVA_TOOL_OPTIONS").unwrap_or_default();
        if inherited.contains("ibkit_gateway_agent") {
            bail!(
                "JAVA_TOOL_OPTIONS already loads this agent. Remove that entry; the controller sets it for each session."
            );
        }
        command.env("JAVA_TOOL_OPTIONS", format!("{inherited} {agent_option}"));
        command.arg(format!(
            "-J-DjtsConfigDir={}",
            self.config.settings_dir.display()
        ));
        if let Some(mb) = self.config.java_heap_mb {
            // -J options follow the vendor vmoptions, so this -Xmx wins. The
            // Linux launcher also reads INSTALL4J_ADD_VM_PARAMS, which a
            // restart child inherits.
            command.arg(format!("-J-Xmx{mb}m"));
            if cfg!(target_os = "linux") {
                command.env("INSTALL4J_ADD_VM_PARAMS", format!("-Xmx{mb}m"));
            }
        }
        platform::cap_open_files()?;
        command.arg(&self.config.settings_dir);
        if self.config.manage_display {
            if self.display.is_none() {
                let mut display_command = Command::new("Xvfb");
                display_command
                    .args([
                        "-displayfd",
                        "1",
                        "-screen",
                        "0",
                        "1280x800x24",
                        "-nolisten",
                        "tcp",
                    ])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true);
                // Its own group: a signal to the controller's group (Ctrl-C,
                // `tini -g` in the image) must not end the display before
                // Gateway has logged out.
                platform::isolate(&mut display_command);
                let mut display = display_command.spawn().context("Cannot start Xvfb. Install Xvfb or set manage_display=false and supply DISPLAY.")?;
                let mut number = Vec::new();
                let mut stdout = display
                    .stdout
                    .take()
                    .context("Xvfb has no display output")?;
                timeout(Duration::from_secs(15), async {
                    loop {
                        let b = stdout.read_u8().await?;
                        if b == b'\n' {
                            break;
                        }
                        if number.len() > 8 {
                            return Err(std::io::Error::other("Invalid Xvfb display number"));
                        }
                        number.push(b);
                    }
                    Ok::<_, std::io::Error>(())
                })
                .await??;
                let number = String::from_utf8(number)?;
                command.env("DISPLAY", format!(":{number}"));
                if let (Some(address), Some(password)) =
                    (self.config.vnc_listen, &self.config.vnc_password_file)
                {
                    let mut vnc = Command::new("x11vnc");
                    vnc.args(["-display", &format!(":{number}"), "-rfbport"])
                        .arg(address.port().to_string())
                        .arg("-listen")
                        .arg(address.ip().to_string())
                        .arg("-passwdfile")
                        .arg(password)
                        .args(["-forever", "-shared", "-quiet"])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .kill_on_drop(true);
                    platform::isolate(&mut vnc);
                    self.vnc = Some(vnc.spawn().context(
                        "Cannot start x11vnc for vnc_listen. Install x11vnc (the image has it) or remove vnc_listen.",
                    )?);
                }
                self.display = Some((display, number));
            } else if let Some((_, number)) = &self.display {
                command.env("DISPLAY", format!(":{number}"));
            }
        }
        platform::isolate_launcher(&mut command);
        // Keep raw launcher output private. It can contain session tokens.
        let stdio =
            platform::private_file(OpenOptions::new().write(true).create(true).truncate(true))
                .open(self.config.state_dir.join("launcher-stdio.log"))?;
        command
            .stdout(Stdio::from(stdio.try_clone()?))
            .stderr(Stdio::from(stdio))
            .kill_on_drop(true);
        self.started = Instant::now();
        // Publish the session before exec so a replacement controller can find
        // its agent even if this controller dies immediately after spawn.
        self.save()?;
        // launch() reports a failure here as ProcessStartFailed, with this error.
        let child = command.spawn().with_context(|| {
            format!(
                "cannot run the launcher {}",
                self.config.executable.display()
            )
        })?;
        // The launcher's session processes: Gateway's JVMs, also the restart
        // child, stay in them.
        let group = child
            .id()
            .map(|pid| platform::session_spawned(pid, &self.status.session))
            .transpose();
        self.child = Some(child);
        let group = group?;
        self.status.process_group = group;
        self.groups.extend(group);
        self.emit(EventKind::Started);
        self.save()
    }

    /// Start and wait for UI configuration AND an IB API protocol response.
    /// Call poll regularly after start to receive restart and exit events.
    pub async fn start(&mut self) -> Result<StartResult> {
        self.launch().await?;
        loop {
            if self.started.elapsed() > Duration::from_secs(self.config.initialization_timeout_secs)
            {
                self.set_error(ErrorCode::InitializationTimeout);
                self.save()?;
                return Ok(self.status.last_start_result.clone());
            }
            match self.poll().await? {
                State::Ready if api_handshake(self.config.api_port).await.is_ok() => {
                    return Ok(self.status.last_start_result.clone());
                }
                State::Failed => return Ok(self.status.last_start_result.clone()),
                State::Exited => {
                    if !self.status.last_start_result.has_error() {
                        self.set_error(ErrorCode::ProcessStartFailed);
                    }
                    self.save()?;
                    return Ok(self.status.last_start_result.clone());
                }
                _ => sleep(Duration::from_millis(500)).await,
            }
        }
    }

    pub async fn poll(&mut self) -> Result<State> {
        if matches!(
            self.status.state,
            State::Exited | State::Failed | State::Halted | State::Paused
        ) {
            return Ok(self.status.state);
        }
        let agents = read_agents(&self.config.state_dir, &self.status.session)?;
        for agent in &agents {
            let path = self
                .config
                .state_dir
                .join("agents")
                .join(format!("{}.events", agent.pid));
            if let Ok(mut file) = File::open(path) {
                let offset = self.offsets.entry(agent.pid).or_default();
                if file.metadata()?.len() < *offset {
                    *offset = 0;
                }
                file.seek(SeekFrom::Start(*offset))?;
                let mut buffer = Vec::new();
                file.take(1024 * 1024).read_to_end(&mut buffer)?;
                if let Some(end) = buffer.iter().rposition(|byte| *byte == b'\n') {
                    *offset += (end + 1) as u64;
                    for line in buffer[..end].split(|byte| *byte == b'\n') {
                        if let Ok(mut event) = serde_json::from_slice::<Event>(line) {
                            if event.session != self.status.session || event.pid != agent.pid {
                                continue;
                            }
                            if let EventKind::Output { message } = &mut event.kind {
                                *message = self.credentials.redact(message);
                            }
                            let _ = self.events.send(event);
                        }
                    }
                }
            }
        }
        let selected = agents
            .iter()
            .filter(|agent| {
                Some(agent.pid) != self.active_pid && !self.known_ready.contains(&agent.pid)
            })
            .max_by_key(|agent| agent.at)
            .or_else(|| {
                agents
                    .iter()
                    .find(|agent| Some(agent.pid) == self.active_pid)
            });
        self.groups.extend(
            agents
                .iter()
                .filter_map(|agent| platform::foreign_group(agent.pid)),
        );
        if let Some(agent) = selected {
            // The JVM reports again (a stall ended) or a new one took over.
            self.agent_lost = None;
            self.launcher_gone = None;
            if self.active_pid != Some(agent.pid) && !self.known_ready.is_empty() {
                self.restart_deadline.get_or_insert_with(|| {
                    Instant::now() + Duration::from_secs(self.config.soft_restart_timeout_secs)
                });
            }
            self.active_pid = Some(agent.pid);
            if agent.state == State::Ready && self.known_ready.insert(agent.pid) {
                if self.known_ready.len() > 1 {
                    self.emit(EventKind::Restarted);
                }
                self.restart_deadline = None;
            }
            if agent.result.has_error() {
                self.status.last_start_result = agent.result.clone();
            }
            self.status.state = agent.state;
        } else if self.known_ready.is_empty() {
            if self.started.elapsed() > Duration::from_secs(self.config.initialization_timeout_secs)
            {
                self.set_error(ErrorCode::InitializationTimeout);
            } else if let Some(child) = &mut self.child
                && let Some(exit) = child.try_wait()?
            {
                // install4j can exit its launcher while its Java child starts.
                if !exit.success() {
                    let log = fs::read_to_string(self.config.state_dir.join("launcher-stdio.log"))
                        .unwrap_or_default();
                    if log.contains("agent library failed to init") {
                        // Another login cannot fix this: not retried.
                        let reason = log
                            .lines()
                            .find(|line| line.starts_with("ibkit agent:"))
                            .unwrap_or("the JVM gave no reason");
                        self.set_error_result(StartResult {
                            error_code: ErrorCode::InvalidConfiguration,
                            error_message: format!(
                                "The native agent {} could not start ({reason}). Use the agent built with this ibkit (the one next to the ibkit binary), or rebuild both with `cargo build -p ibkit-cli`.",
                                self.config.agent.display()
                            ),
                        });
                    } else {
                        self.set_error(if log.contains("No suitable Java Virtual Machine") {
                            ErrorCode::JavaNotFound
                        } else {
                            ErrorCode::ProcessStartFailed
                        });
                    }
                } else if !self
                    .groups
                    .iter()
                    .any(|group| platform::group_alive(*group, &self.status.session))
                {
                    // Exit code 0 and nothing of the launcher left: no JVM
                    // will report.
                    let gone = *self.launcher_gone.get_or_insert_with(Instant::now);
                    if gone.elapsed() >= PROCESS_GONE_GRACE {
                        self.set_error(ErrorCode::ProcessStartFailed);
                    }
                }
            }
        } else if self.status.state != State::Stopping {
            self.status.state = State::Restarting;
            let lost = *self.agent_lost.get_or_insert_with(Instant::now);
            // Reap an exited launcher: as a zombie it still counts as a
            // member of its process group.
            if let Some(child) = &mut self.child {
                let _ = child.try_wait()?;
            }
            // A JVM that ended with nothing of the session left (a crash, an
            // OOM kill) gets no 10-minute wait; a hung one, or the daily
            // restart handoff, keeps the soft restart limit.
            let dead = self
                .active_pid
                .is_none_or(|pid| !platform::process_alive(pid))
                && !self
                    .groups
                    .iter()
                    .any(|group| platform::group_alive(*group, &self.status.session));
            if dead && lost.elapsed() >= PROCESS_GONE_GRACE {
                self.set_error(ErrorCode::AgentDisconnected);
            } else if lost.elapsed() >= Duration::from_secs(self.config.soft_restart_timeout_secs) {
                self.set_error(ErrorCode::SoftRestartTimeout);
            }
        }
        if self
            .restart_deadline
            .is_some_and(|deadline| Instant::now() > deadline)
        {
            self.set_error(ErrorCode::SoftRestartTimeout);
        }
        if self.known_ready.is_empty()
            && self.started.elapsed() > Duration::from_secs(self.config.initialization_timeout_secs)
        {
            self.set_error(ErrorCode::InitializationTimeout);
        }
        if self.status.state == State::Ready
            && self.api_checked.elapsed()
                >= Duration::from_secs(self.config.api_probe_interval_secs)
        {
            self.api_checked = Instant::now();
            if api_handshake(self.config.api_port).await.is_ok() {
                if self.api_failed_since.take().is_some() {
                    self.emit(EventKind::ApiRestored);
                }
            } else {
                if self.api_failed_since.is_none() {
                    self.api_failed_since = Some(Instant::now());
                    self.emit(EventKind::ApiUnavailable);
                }
                if self.api_failed_since.is_some_and(|since| {
                    since.elapsed() >= Duration::from_secs(self.config.api_failure_timeout_secs)
                }) && !ibkit_gateway_core::schedule::is_within_scheduled_server_reset_times(
                    Utc::now(),
                    self.config.region,
                ) {
                    self.set_error(ErrorCode::ApiConnectionUnavailable);
                }
            }
        }
        if let Some(wanted) = self.config.java_heap_mb {
            for agent in &agents {
                // Runtime.maxMemory is a little below -Xmx; 90% leaves margin.
                if let Some(heap) = agent.max_heap_mb
                    && heap * 10 < u64::from(wanted) * 9
                    && self.heap_checked.insert(agent.pid)
                {
                    self.emit(EventKind::Output {
                        message: format!(
                            "Gateway JVM {} runs with a {heap} MiB heap, below java_heap_mb = {wanted}. Gateway's restart child ignored the launcher option; set -Xmx{wanted}m in the Gateway .vmoptions file to keep the larger heap after daily restarts.",
                            agent.pid
                        ),
                    });
                }
            }
        }
        self.status.agents = agents;
        if let Some(control) = self.take_control()? {
            let result = match control {
                // A stop that fails must not turn into recovery restarting
                // Gateway after an operator stop: it stays an error.
                Control::Stop => {
                    self.status.last_start_result = StartResult::new(ErrorCode::None);
                    self.stop().await?;
                    Ok(())
                }
                Control::Pause => {
                    self.pause().await?;
                    Ok(())
                }
                Control::Restart => {
                    self.stop().await?;
                    self.launch().await
                }
                Control::SoftRestart => self.soft_restart(),
                Control::Reload => {
                    self.reload = true;
                    Ok(())
                }
                Control::ExportLogs | Control::ReconnectAccount | Control::ReconnectData => {
                    self.command_agents(control)
                }
            };
            self.report_command_error(control, result);
        }
        self.save()?;
        Ok(self.status.state)
    }

    /// An operator command that fails (a soft restart before Ready, a launch
    /// that cannot start) is an event, not a controller failure.
    fn report_command_error(&mut self, control: Control, result: Result<()>) {
        if let Err(error) = result {
            self.emit(EventKind::Output {
                message: format!("The {control:?} command failed: {error:#}"),
            });
        }
    }

    /// Stop Gateway and stay paused until `restart`, also across controller
    /// restarts: the marker file `paused` in the state directory holds it.
    pub async fn pause(&mut self) -> Result<()> {
        self.status.last_start_result = StartResult::new(ErrorCode::None);
        self.stop().await?;
        write_private(&self.config.state_dir.join(PAUSED), b"paused\n")?;
        self.status.state = State::Paused;
        self.emit(EventKind::StateChanged {
            state: State::Paused,
        });
        self.save()
    }

    pub fn is_paused(&self) -> bool {
        self.status.state == State::Paused
    }

    /// Serve a paused controller: `restart` logs in again, `reload` applies
    /// the settings and stays paused, `stop` exits. Returns `Exited` after
    /// stop.
    pub async fn poll_paused(&mut self, recovery: &mut Recovery) -> Result<State> {
        match self.take_control()? {
            Some(Control::Stop) => {
                self.status.state = State::Exited;
                self.save()?;
                return Ok(State::Exited);
            }
            Some(Control::Restart) => {
                let _ = fs::remove_file(self.config.state_dir.join(PAUSED));
                let result = self.relaunch(recovery).await;
                self.report_command_error(Control::Restart, result);
            }
            Some(Control::Reload) => self.reload = true,
            Some(_) => self.emit(EventKind::Output {
                message: "Gateway is paused. Send restart to log in again, reload to apply changed settings, or stop to exit.".into(),
            }),
            None => {}
        }
        self.save()?;
        Ok(self.status.state)
    }

    pub async fn recover(
        &mut self,
        recovery: &mut ibkit_gateway_core::recovery::Recovery,
    ) -> Result<bool> {
        if !matches!(
            self.status.state,
            State::Failed | State::Exited | State::Halted
        ) {
            return Ok(false);
        }
        let reason = self.status.last_start_result.error_code;
        let Some((attempt, retry_at)) = recovery.schedule(&self.config, reason, Utc::now()) else {
            if reason != ErrorCode::None {
                self.save_recovery(recovery)?;
            }
            return Ok(false);
        };
        self.save_recovery(recovery)?;
        self.stop().await?;
        self.status.state = State::Recovering;
        self.status.last_start_result = StartResult::new(reason);
        self.emit(EventKind::RecoveryScheduled {
            attempt,
            retry_at,
            reason,
        });
        self.save()?;
        Ok(true)
    }

    pub async fn poll_recovery(
        &mut self,
        recovery: &mut ibkit_gateway_core::recovery::Recovery,
    ) -> Result<State> {
        if self.status.state != State::Recovering {
            let state = self.poll().await?;
            if state == State::Ready && recovery.ready() {
                self.save_recovery(recovery)?;
            }
            return Ok(state);
        }
        let reports = read_agent_reports(&self.config.state_dir, &self.status.session)?;
        if let Some(agent) = reports
            .iter()
            .filter(|agent| {
                agent.result.has_error() && !Recovery::is_retryable(agent.result.error_code)
            })
            .max_by_key(|agent| agent.at)
        {
            self.status.agents = read_agents(&self.config.state_dir, &self.status.session)?;
            self.set_error(agent.result.error_code);
            recovery.schedule(&self.config, agent.result.error_code, Utc::now());
            self.save_recovery(recovery)?;
            self.save()?;
            return Ok(State::Failed);
        }
        // Taken after the early return above, so no command is lost.
        let control = self.take_control()?;
        let agents: Vec<_> = read_agents(&self.config.state_dir, &self.status.session)?
            .into_iter()
            .filter(|agent| !self.closed_agents.contains(&agent.pid))
            .collect();
        if !agents.is_empty() {
            self.status.agents = agents;
            let reason = self.status.last_start_result.error_code;
            self.stop().await?;
            if matches!(recovery.state, RecoveryState::AwaitingAgent { .. }) {
                self.set_error(reason);
                self.recover(recovery).await?;
            } else {
                self.status.state = State::Recovering;
            }
        }
        match control {
            Some(Control::Stop) => {
                self.cancel_recovery(recovery)?;
                self.status.last_start_result = StartResult::new(ErrorCode::None);
                self.stop().await?;
                return Ok(State::Exited);
            }
            Some(Control::Restart) if matches!(recovery.state, RecoveryState::AwaitingAgent { .. }) => self.emit(EventKind::Output { message: "The previous Gateway has not reported its agent. Wait for cleanup or stop the controller and close that session manually before reset-recovery.".into() }),
            Some(Control::Restart) => {
                let result = self.relaunch(recovery).await;
                self.report_command_error(Control::Restart, result);
                return Ok(self.status.state);
            }
            Some(Control::Pause) => {
                self.cancel_recovery(recovery)?;
                self.pause().await?;
                return Ok(self.status.state);
            }
            Some(Control::Reload) => self.reload = true,
            Some(_) => self.emit(EventKind::Output { message: "Gateway is waiting for recovery. Use stop to cancel, pause, or restart to request a new login.".into() }),
            None => {},
        }
        if let RecoveryState::AwaitingAgent { deadline } = recovery.state {
            if Utc::now() >= deadline {
                self.set_error(ErrorCode::AgentDisconnected);
                recovery.state = RecoveryState::Blocked {
                    reason: ErrorCode::AgentDisconnected,
                };
                self.save_recovery(recovery)?;
            }
            self.save()?;
            return Ok(self.status.state);
        }
        let now = Utc::now();
        if recovery.can_retry(&self.config, now) {
            let result = self.relaunch(recovery).await;
            self.report_command_error(Control::Restart, result);
        } else if let RecoveryState::Waiting { retry_at, .. } = recovery.state
            && now >= retry_at
        {
            // IBKR's scheduled reset window holds the retry; say until when.
            let end = ibkit_gateway_core::schedule::reset_window_end(now, self.config.region);
            if self.window_noted != Some(end) {
                self.window_noted = Some(end);
                self.emit(EventKind::Output {
                    message: format!(
                        "The retry waits for the end of IBKR's scheduled reset window at {end} (region {:?}). Send restart to log in now.",
                        self.config.region
                    ),
                });
            }
        }
        self.save()?;
        Ok(self.status.state)
    }

    fn set_error(&mut self, code: ErrorCode) {
        self.set_error_result(StartResult::new(code));
    }

    fn set_error_result(&mut self, result: StartResult) {
        if self.status.state == State::Failed {
            return;
        }
        self.status.state = State::Failed;
        self.restart_deadline = None;
        self.status.last_start_result = result;
        self.emit(EventKind::Error {
            result: self.status.last_start_result.clone(),
        });
    }

    fn command_agents(&self, control: Control) -> Result<()> {
        for agent in &self.status.agents {
            atomic_json(
                &self
                    .config
                    .state_dir
                    .join(format!("command-{}.json", agent.pid)),
                &AgentCommand {
                    session: self.status.session.clone(),
                    control,
                },
            )?;
        }
        Ok(())
    }

    pub fn soft_restart(&mut self) -> Result<()> {
        if self.status.state != State::Ready {
            bail!(
                "You requested a soft restart before Gateway was ready. Wait for ready state and try again."
            );
        }
        if self.restart_deadline.is_some() {
            return Ok(());
        }
        if self.restart_epoch.elapsed() >= Duration::from_secs(3600) {
            self.restart_count = 0;
            self.restart_epoch = Instant::now();
        }
        if self.restart_count >= 3 {
            bail!(
                "You requested more than three soft restarts in one hour. Wait for the next hour or use a cold restart."
            );
        }
        self.restart_count += 1;
        self.command_agents(Control::SoftRestart)?;
        self.status.state = State::Restarting;
        self.restart_deadline =
            Some(Instant::now() + Duration::from_secs(self.config.soft_restart_timeout_secs));
        self.save()
    }

    pub async fn restart(&mut self) -> Result<StartResult> {
        self.stop().await?;
        self.start().await
    }

    pub async fn stop(&mut self) -> Result<()> {
        let session = self.status.session.clone();
        let alive = |groups: &HashSet<u32>| {
            groups
                .iter()
                .any(|group| platform::group_alive(*group, &session))
        };
        if self.child.is_none() && self.status.agents.is_empty() && !alive(&self.groups) {
            self.status.state = State::Exited;
            self.groups.clear();
            self.status.process_group = None;
            return self.save();
        }
        self.status.state = State::Stopping;
        self.command_agents(Control::Stop)?;
        self.save()?;
        let deadline = Instant::now() + Duration::from_secs(self.config.shutdown_timeout_secs);
        let mut forced = false;
        loop {
            let live = read_agents(&self.config.state_dir, &self.status.session)?;
            self.groups.extend(
                live.iter()
                    .filter_map(|agent| platform::foreign_group(agent.pid)),
            );
            let child_done = match &mut self.child {
                Some(child) => child.try_wait()?.is_some(),
                None => true,
            };
            // A JVM that no longer reports (hung, or a restart child after
            // the launcher exited) still holds the API port and the IBKR
            // session: wait for its group too.
            if live.is_empty() && child_done && !alive(&self.groups) {
                break;
            }
            if Instant::now() >= deadline {
                forced = true;
                // Only this session's processes are eligible: fresh agents of
                // this session and the groups this controller started or read
                // from them. No process-name kill and no saved-PID kill.
                for agent in &live {
                    platform::end_process(agent.pid, false);
                }
                platform::end_groups(&self.groups, &self.status.session, false);
                sleep(Duration::from_millis(500)).await;
                platform::end_groups(&self.groups, &self.status.session, true);
                if let Some(child) = &mut self.child {
                    let _ = child.kill().await;
                }
                for agent in read_agents(&self.config.state_dir, &self.status.session)? {
                    platform::end_process(agent.pid, true);
                }
                break;
            }
            // The heartbeat continues while Gateway shuts down, so the
            // controller stays live and accepts commands.
            self.save()?;
            sleep(Duration::from_millis(250)).await;
        }
        let code = match &mut self.child {
            Some(child) => child.try_wait()?.and_then(|exit| exit.code()),
            None => None,
        };
        self.child = None;
        if let Some(mut vnc) = self.vnc.take() {
            let _ = vnc.kill().await;
        }
        if let Some((mut display, _)) = self.display.take() {
            let _ = display.kill().await;
        }
        self.closed_agents
            .extend(self.status.agents.iter().map(|agent| agent.pid));
        self.status.agents.clear();
        self.groups.clear();
        self.status.process_group = None;
        self.status.state = State::Exited;
        self.restart_deadline = None;
        self.agent_lost = None;
        if forced {
            self.emit(EventKind::Output {
                message: format!(
                    "Gateway did not stop within shutdown_timeout_secs ({} s); the controller killed it, so IBKR may keep its session open for a while. Raise shutdown_timeout_secs if this repeats.",
                    self.config.shutdown_timeout_secs
                ),
            });
        }
        self.emit(EventKind::Exited { code });
        self.save()
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        // Explicit stop is async; this fallback asks agents to close and owned
        // tokio child handles kill their direct children when dropped.
        let _ = self.command_agents(Control::Stop);
    }
}

/// Remove agent reports and event logs that no running agent writes: a
/// file of a fresh agent (any session) stays for adoption.
fn prune_agent_files(state_dir: &Path) {
    let directory = state_dir.join("agents");
    let Ok(entries) = fs::read_dir(&directory) else {
        return;
    };
    let fresh: HashSet<String> = read_agent_reports_any(state_dir)
        .into_iter()
        .filter(|status| (Utc::now() - status.at).num_seconds() <= 3)
        .map(|status| status.pid.to_string())
        .collect();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let pid = name.split('.').next().unwrap_or_default();
        if !fresh.contains(pid) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Every parseable agent report in `state_dir/agents`, of any session.
fn read_agent_reports_any(state_dir: &Path) -> Vec<AgentStatus> {
    let Ok(entries) = fs::read_dir(state_dir.join("agents")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| serde_json::from_slice(&fs::read(entry.path()).ok()?).ok())
        .collect()
}

/// Validate a configuration, resolve its paths, check the launcher and
/// agent files, and read its credentials.
pub fn check_config(config: &mut Config) -> Result<Credentials> {
    config.validate()?;
    config.resolve_paths()?;
    // The agent reads the TOTP secret too; a bad one would stop it silently.
    config.totp_key()?;
    if !config.executable.is_file() {
        bail!("{}", ErrorCode::IbGatewayVersionNotInstalled.message());
    }
    if !config.agent.is_file() {
        bail!(
            "You selected an absent agent library. Build ibkit-cli and set agent to its .so or .dylib path."
        );
    }
    config.credentials()
}

/// Create the settings directory and the launcher settings Gateway reads
/// before login. Existing files keep their content except UseSSL.
fn prepare_settings(config: &Config) -> Result<()> {
    platform::private_dir(&config.settings_dir)?;
    // SSL is a launcher setting; API and encrypted per-user settings are
    // configured through Swing, not by editing the vendor's encrypted files.
    let ini = config.settings_dir.join("jts.ini");
    let text = match fs::read_to_string(&ini) {
        Ok(text) => text,
        // A new file: displayedproxymsg=1 suppresses a startup proxy notice.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            "[IBGateway]\nTrustedIPs=127.0.0.1\nApiOnly=true\n\n[Logon]\nLocale=en\nUseSSL=true\ndisplayedproxymsg=1\n".to_owned()
        }
        Err(error) => return Err(error).context("Cannot read the existing jts.ini file. Use a readable UTF-8 settings file; repair its content or permissions before you start again."),
    };
    // The agent recognizes Gateway's English texts, so the UI language is
    // English; TimeZone follows TZ, because the restart time means local
    // time and TZ can change between starts (for example a chart value).
    let zone = std::env::var("TZ")
        .ok()
        .filter(|zone| zone.parse::<chrono_tz::Tz>().is_ok());
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    for (key, value) in [
        ("UseSSL", Some("true".to_owned())),
        ("Locale", Some("en".to_owned())),
        ("TimeZone", zone),
    ] {
        let Some(value) = value else { continue };
        let line = format!("{key}={value}");
        match lines
            .iter_mut()
            .find(|existing| existing.starts_with(&format!("{key}=")))
        {
            Some(existing) => *existing = line,
            None => lines.extend(["[Logon]".to_owned(), line]),
        }
    }
    fs::write(ini, lines.join("\n") + "\n")?;
    Ok(())
}

pub fn read_status(state_dir: &Path) -> Result<Status> {
    serde_json::from_slice(
        &fs::read(state_dir.join("status.json"))
            .context("No controller status exists. Start Gateway with this state_dir first.")?,
    )
    .context("The controller status is invalid. Stop the controller, then start it again.")
}

/// The directory of queued controller commands in the state directory.
const CONTROL_QUEUE: &str = "control";
/// Marker file of a paused controller (`pause`, `closedown_at`).
const PAUSED: &str = "paused";

/// Settings that only the controller reads: a reload that changes only
/// these needs no new Gateway login. Every other key restarts Gateway.
const CONTROLLER_KEYS: &[&str] = &[
    "alert_webhook_url",
    "auto_recover",
    "recovery_attempts",
    "recovery_delay_secs",
    "recovery_max_delay_secs",
    "max_login_failures",
    "api_probe_interval_secs",
    "api_failure_timeout_secs",
    "api_listen",
    "api_allowed_sources",
    "shutdown_timeout_secs",
    "soft_restart_timeout_secs",
    "region",
    "closedown_at",
    "cold_restart_at",
    "control_api_full_access",
    "control_listen",
    "control_token_env",
    "control_token_file",
];

/// The keys whose values differ between `old` and `new`.
fn changed_keys(old: &Config, new: &Config) -> Vec<String> {
    let (Ok(serde_json::Value::Object(old)), Ok(serde_json::Value::Object(new))) =
        (serde_json::to_value(old), serde_json::to_value(new))
    else {
        return vec!["*".into()];
    };
    new.iter()
        .filter(|(key, value)| old.get(*key) != Some(*value))
        .map(|(key, _)| key.clone())
        .collect()
}

/// A command in the control queue, for the controller with this pid.
#[derive(Deserialize, Serialize)]
struct QueuedControl {
    controller_pid: u32,
    control: Control,
}

/// Queue `control` for the running controller. Commands run in send order,
/// one per controller step (500 ms).
pub fn send_control(state_dir: &Path, control: Control) -> Result<()> {
    static SENT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let status = read_status(state_dir)?;
    if !status.is_live() {
        bail!(
            "You sent a command to an inactive controller. Start Gateway with this state_dir and try again."
        );
    }
    if matches!(control, Control::SoftRestart) && status.state != State::Ready {
        bail!(
            "You requested a soft restart while Gateway is {:?}. A soft restart needs a logged-in Gateway; wait for state ready, or send restart for a new login.",
            status.state
        );
    }
    let directory = state_dir.join(CONTROL_QUEUE);
    platform::private_dir(&directory)?;
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let sent = SENT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    atomic_json(
        &directory.join(format!("{at:020}-{}-{sent:06}.json", std::process::id())),
        &QueuedControl {
            controller_pid: status.controller_pid,
            control,
        },
    )
}

fn read_agents(state_dir: &Path, session: &str) -> Result<Vec<AgentStatus>> {
    Ok(read_agent_reports(state_dir, session)?
        .into_iter()
        .filter(|status| {
            let age = Utc::now() - status.at;
            age.num_milliseconds() >= 0 && age.num_seconds() <= 3
        })
        .collect())
}

fn read_agent_reports(state_dir: &Path, session: &str) -> Result<Vec<AgentStatus>> {
    let mut agents = Vec::new();
    for entry in fs::read_dir(state_dir.join("agents"))? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = fs::read(path)
            && let Ok(status) = serde_json::from_slice::<AgentStatus>(&bytes)
            && status.session == session
        {
            agents.push(status);
        }
    }
    Ok(agents)
}

/// Write `value` as JSON to `path` in one step (a temporary file, then a
/// rename), so a reader never sees a partial file.
pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().context(
        "The state file has no parent directory. Select a state directory and start again.",
    )?)?;
    serde_json::to_writer(&mut file, value)?;
    platform::persist(file, path)?;
    Ok(())
}

/// Write `bytes` to `path` atomically and durably as a 0600 file in a 0700
/// directory (created when absent). For credentials and private settings.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .context("A private file has no parent directory. Select a state_dir with a parent.")?;
    platform::private_dir(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    std::io::Write::write_all(&mut file, bytes)?;
    file.as_file().sync_all()?;
    platform::persist(file, path)?;
    Ok(())
}

/// An IB API handshake checks protocol readiness without sending an order or
/// allocating a client ID. A plain open TCP port is not enough.
pub async fn api_handshake(port: u16) -> Result<u32> {
    timeout(Duration::from_secs(3), async {
        let mut socket = TcpStream::connect(("127.0.0.1", port)).await?;
        let version = b"v100..178";
        socket.write_all(b"API\0").await?;
        socket.write_u32(version.len() as u32).await?;
        socket.write_all(version).await?;
        let size = socket.read_u32().await?;
        if !(3..=4096).contains(&size) { bail!("Gateway returned an invalid API handshake size. Use a supported IB API port."); }
        let mut data = vec![0; size as usize];
        socket.read_exact(&mut data).await?;
        let mut fields = data.split(|value| *value == 0);
        let server: u32 = std::str::from_utf8(fields.next().context("API server version is absent")?)?.parse()?;
        if server < 100 || fields.next().is_none_or(|value| value.is_empty()) { bail!("The selected port did not return an IB API greeting. Set api_port to the Gateway API port."); }
        Ok(server)
    }).await.context("The API handshake timed out. Check the configured API port and trusted client settings.")?
}
