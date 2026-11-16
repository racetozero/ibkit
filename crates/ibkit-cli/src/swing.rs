#![forbid(unsafe_code)]
use anyhow::{Context, Result, bail};
use chrono::{Local, Timelike, Utc};
use ibkit_gateway::platform;
use ibkit_gateway_core::{
    Action, AgentLaunch, AgentStatus, Command, Control, Credentials, Dialog, ErrorCode, Event,
    EventKind, StartResult, State, TradingMode, WindowSnapshot,
};
use jni::{
    JNIEnv,
    objects::{GlobalRef, JClass, JObject, JObjectArray, JString, JValue},
};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

pub enum UiAction {
    Click(GlobalRef),
    Close(GlobalRef),
    /// Menu-shortcut + Alt + key on a window, as IBC's reconnect commands.
    Shortcut(GlobalRef, char),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ExportStage {
    Idle,
    Gateway,
    ApiPending,
    Api,
}

impl UiAction {
    pub fn run(self, env: &mut JNIEnv<'_>) -> jni::errors::Result<()> {
        match self {
            Self::Click(button) => {
                env.call_method(button.as_obj(), "doClick", "(I)V", &[JValue::Int(0)])?;
            }
            Self::Close(window) => {
                let event = env.new_object(
                    "java/awt/event/WindowEvent",
                    "(Ljava/awt/Window;I)V",
                    &[JValue::Object(window.as_obj()), JValue::Int(201)],
                )?;
                env.call_method(
                    window.as_obj(),
                    "dispatchEvent",
                    "(Ljava/awt/AWTEvent;)V",
                    &[JValue::Object(&event)],
                )?;
            }
            Self::Shortcut(window, key) => {
                let toolkit = env
                    .call_static_method(
                        "java/awt/Toolkit",
                        "getDefaultToolkit",
                        "()Ljava/awt/Toolkit;",
                        &[],
                    )?
                    .l()?;
                const ALT_DOWN_MASK: i32 = 512;
                let modifiers = env
                    .call_method(&toolkit, "getMenuShortcutKeyMaskEx", "()I", &[])?
                    .i()?
                    | ALT_DOWN_MASK;
                let now = chrono::Utc::now().timestamp_millis();
                // KEY_PRESSED 401, KEY_TYPED 400, KEY_RELEASED 402; VK_A..VK_Z equal 'A'..'Z'.
                for (id, code, character) in [
                    (401, key as i32, 0xffff_u16),
                    (400, 0, key as u16),
                    (402, key as i32, 0xffff_u16),
                ] {
                    let event = env.new_object(
                        "java/awt/event/KeyEvent",
                        "(Ljava/awt/Component;IJIIC)V",
                        &[
                            JValue::Object(window.as_obj()),
                            JValue::Int(id),
                            JValue::Long(now),
                            JValue::Int(modifiers),
                            JValue::Int(code),
                            JValue::Char(character),
                        ],
                    )?;
                    env.call_method(
                        window.as_obj(),
                        "dispatchEvent",
                        "(Ljava/awt/AWTEvent;)V",
                        &[JValue::Object(&event)],
                    )?;
                }
            }
        }
        Ok(())
    }
}

/// Write the agent report `agents/<pid>.json`.
fn write_report(launch: &AgentLaunch, status: &AgentStatus) -> Result<()> {
    let agents = launch.config.state_dir.join("agents");
    ibkit_gateway::atomic_json(&agents.join(format!("{}.json", status.pid)), status)
}

/// Report that the agent cannot run (for example an unreadable TOTP secret),
/// so the controller stops waiting at once and shows `error`.
pub fn report_start_failure(launch: &AgentLaunch, error: &anyhow::Error) -> Result<()> {
    platform::private_dir(&launch.config.state_dir.join("agents"))?;
    write_report(
        launch,
        &AgentStatus {
            session: launch.session.clone(),
            pid: std::process::id(),
            at: Utc::now(),
            state: State::Failed,
            result: StartResult {
                error_code: ErrorCode::InvalidConfiguration,
                error_message: format!("The Gateway agent cannot start: {error:#}"),
            },
            max_heap_mb: None,
        },
    )
}

/// Answer a confirmation that Gateway opened while the agent's state is
/// locked: a listener that the agent triggered opened a modal dialog, so the
/// event thread runs a nested event loop inside the locked pass. Only
/// confirmations whose answer needs no agent state are clicked.
pub fn answer_nested_confirmation(
    env: &mut JNIEnv<'_>,
    config: &ibkit_gateway_core::Config,
) -> Result<Option<UiAction>> {
    let windows = JObjectArray::from(
        env.call_static_method("java/awt/Window", "getWindows", "()[Ljava/awt/Window;", &[])?
            .l()?,
    );
    for index in 0..env.get_array_length(&windows)? {
        let window = env.get_object_array_element(&windows, index)?;
        if !env.call_method(&window, "isShowing", "()Z", &[])?.z()? {
            continue;
        }
        let nodes = components(env, &window)?;
        let kind = snapshot(env, &window, &nodes)?.classify();
        if matches!(
            kind,
            Dialog::ApiPortChange
                | Dialog::RestartConfirmation
                | Dialog::ExitSettings
                | Dialog::TipOfTheDay
                | Dialog::MarketData
                | Dialog::PasswordExpiryWarning
        ) && let Action::Click(label) = kind.action(config)
        {
            return click(env, &nodes, label);
        }
    }
    Ok(None)
}

pub struct Automation {
    launch: AgentLaunch,
    credentials: Credentials,
    state: State,
    result: StartResult,
    handled: HashSet<(i32, Dialog)>,
    reported: HashSet<i32>,
    second_factor: HashMap<i32, Instant>,
    attempts: u8,
    retry_at: Option<Instant>,
    configured: bool,
    config_opened: bool,
    main_window: Option<GlobalRef>,
    restart_requested: bool,
    export_requested: bool,
    view_logs: Option<GlobalRef>,
    export_finished: bool,
    restarting: bool,
    export_stage: ExportStage,
    authentication_started: Option<Instant>,
    /// Session takeovers in the last hour: a takeover loop with another
    /// session must end.
    takeovers: Vec<Instant>,
    export_deadline: Option<Instant>,
    /// Decoded Mobile Authenticator secret; `None` selects IB Key approval.
    totp: Option<Vec<u8>>,
    /// TOTP time step of the last submitted code; a code is never reused.
    totp_step: Option<u64>,
    /// When each unknown modal dialog was first seen.
    unknown_seen: HashMap<i32, Instant>,
    /// Dialog events already emitted, per window and kind.
    announced: HashSet<(i32, Dialog)>,
    /// launcher.log length when Log In was pressed, and timeouts seen since.
    login_log: Option<(u64, u8)>,
    /// Runtime.maxMemory in MiB, read on the first tick.
    max_heap_mb: Option<u64>,
    /// When a restart child first showed the login screen: a token restore
    /// that IBKR refused leaves it there.
    restart_login_since: Option<Instant>,
    /// Mobile Authenticator codes that IBKR did not accept in this JVM.
    totp_rejections: u8,
    /// When a "connecting to server" window was last visible: a code prompt
    /// that stays open meanwhile waits for the server, not a rejection.
    connecting_seen: Option<Instant>,
    /// When the agent sent its last reconnect shortcut; only then is the
    /// hotkey confirmation answered.
    shortcut_sent: Option<Instant>,
    /// When the current run of failed UI passes began.
    failing_since: Option<Instant>,
}

impl Automation {
    pub fn new(launch: AgentLaunch, restarting: bool) -> Result<Self> {
        platform::private_dir(&launch.config.state_dir.join("agents"))?;
        let credentials = launch.config.credentials()?;
        let totp = launch.config.totp_key()?;
        Ok(Self {
            launch,
            credentials,
            state: State::Starting,
            result: StartResult::new(ErrorCode::None),
            handled: HashSet::new(),
            reported: HashSet::new(),
            second_factor: HashMap::new(),
            attempts: 0,
            retry_at: None,
            configured: false,
            config_opened: false,
            main_window: None,
            restart_requested: false,
            export_requested: false,
            view_logs: None,
            export_finished: false,
            restarting,
            export_stage: ExportStage::Idle,
            authentication_started: Some(Instant::now()),
            takeovers: Vec::new(),
            export_deadline: None,
            totp,
            totp_step: None,
            unknown_seen: HashMap::new(),
            announced: HashSet::new(),
            login_log: None,
            max_heap_mb: None,
            restart_login_since: None,
            totp_rejections: 0,
            connecting_seen: None,
            shortcut_sent: None,
            failing_since: None,
        })
    }

    /// Record a failed UI pass. A short run of failures (a disabled or
    /// absent button while Gateway redraws, a busy file system) is retried;
    /// one that lasts 10 seconds fails the session.
    pub fn tick_failed(&mut self, error: &anyhow::Error) {
        let since = *self.failing_since.get_or_insert_with(Instant::now);
        let _ = self.emit(EventKind::Output {
            message: format!(
                "A UI operation failed: {error}. The agent retries for 10 seconds; if it keeps failing, resolve the Gateway dialog manually, use a supported Gateway release, and check the dialog report."
            ),
        });
        if since.elapsed() >= Duration::from_secs(10) {
            self.fail(ErrorCode::JavaException);
        }
    }

    pub fn tick_succeeded(&mut self) {
        self.failing_since = None;
    }

    /// End this session so that the controller starts a new login.
    fn cold_restart(&mut self, code: ErrorCode) -> Result<Option<UiAction>> {
        self.result = StartResult::new(code);
        self.set_state(State::Exited)?;
        self.heartbeat()?;
        self.emit(EventKind::Exited { code: Some(0) })?;
        Ok(self.main_window.clone().map(UiAction::Close))
    }

    pub fn emit(&self, mut kind: EventKind) -> Result<()> {
        match &mut kind {
            EventKind::Output { message } => *message = self.credentials.redact(message),
            EventKind::Dialog { title, .. } => *title = self.credentials.redact(title),
            _ => {}
        }
        let event = Event {
            at: Utc::now(),
            session: self.launch.session.clone(),
            pid: std::process::id(),
            kind,
        };
        let line = serde_json::to_string(&event)?;
        let path = self
            .launch
            .config
            .state_dir
            .join("agents")
            .join(format!("{}.events", std::process::id()));
        if fs::metadata(&path).is_ok_and(|meta| meta.len() > 10 * 1024 * 1024) {
            fs::rename(&path, path.with_extension("events.1"))?;
        }
        let mut file =
            platform::private_file(OpenOptions::new().create(true).append(true)).open(path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    fn set_state(&mut self, state: State) -> Result<()> {
        if self.state != state {
            self.state = state;
            self.emit(EventKind::StateChanged { state })?;
        }
        Ok(())
    }

    fn begin_authentication(&mut self) -> Result<()> {
        self.authentication_started = Some(Instant::now());
        self.configured = false;
        self.config_opened = false;
        self.restarting = false;
        self.set_state(State::LoggingIn)
    }

    pub fn fail(&mut self, code: ErrorCode) {
        if self.state == State::Failed {
            return;
        }
        self.result = StartResult::new(code);
        let _ = self.set_state(State::Failed);
        let _ = self.emit(EventKind::Error {
            result: self.result.clone(),
        });
        let _ = self.heartbeat();
    }

    fn heartbeat(&self) -> Result<()> {
        write_report(
            &self.launch,
            &AgentStatus {
                session: self.launch.session.clone(),
                pid: std::process::id(),
                at: Utc::now(),
                state: self.state,
                result: self.result.clone(),
                max_heap_mb: self.max_heap_mb,
            },
        )
    }

    pub fn tick(&mut self, env: &mut JNIEnv<'_>) -> Result<Option<UiAction>> {
        if self.max_heap_mb.is_none() {
            let runtime = env
                .call_static_method(
                    "java/lang/Runtime",
                    "getRuntime",
                    "()Ljava/lang/Runtime;",
                    &[],
                )?
                .l()?;
            let bytes = env.call_method(&runtime, "maxMemory", "()J", &[])?.j()?;
            self.max_heap_mb = Some(bytes as u64 / (1024 * 1024));
        }
        self.heartbeat()?;
        let command_path = self
            .launch
            .config
            .state_dir
            .join(format!("command-{}.json", std::process::id()));
        if let Ok(bytes) = fs::read(&command_path) {
            fs::remove_file(&command_path)?;
            let command = match serde_json::from_slice::<Command>(&bytes) {
                Ok(command) => Some(command),
                Err(error) => {
                    self.emit(EventKind::Output {
                        message: format!(
                            "Ignored an unreadable command file {}: {error}. Send commands with the ibkit CLI or the control API.",
                            command_path.display()
                        ),
                    })?;
                    None
                }
            };
            if let Some(command) = command
                && command.session == self.launch.session
            {
                match command.control {
                    Control::Stop | Control::Restart | Control::Pause => {
                        self.set_state(State::Stopping)?;
                        if let Some(window) = &self.main_window {
                            return Ok(Some(UiAction::Close(window.clone())));
                        }
                    }
                    Control::SoftRestart => {
                        self.restart_requested = true;
                        self.config_opened = false;
                        self.set_state(State::Restarting)?;
                    }
                    Control::ExportLogs => self.export_requested = true,
                    // Handled by the controller.
                    Control::Reload => {}
                    Control::ReconnectAccount | Control::ReconnectData => {
                        if let Some(window) = &self.main_window
                            && self.state == State::Ready
                        {
                            let key = if matches!(command.control, Control::ReconnectAccount) {
                                'R'
                            } else {
                                'F'
                            };
                            self.emit(EventKind::Output {
                                message: format!("Reconnect requested ({key} shortcut)."),
                            })?;
                            self.shortcut_sent = Some(Instant::now());
                            return Ok(Some(UiAction::Shortcut(window.clone(), key)));
                        }
                    }
                }
            }
        }
        if matches!(self.state, State::Failed | State::Exited) {
            // A cold restart closes the main window, and Gateway asks
            // whether to exit; answer it so the JVM ends.
            return self.confirm_exit(env);
        }
        if self.state != State::Stopping
            && self
                .export_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.fail(ErrorCode::LogExportTimeout);
            return Ok(None);
        }
        if self.state != State::Stopping
            && self.authentication_started.is_some_and(|at| {
                at.elapsed() > Duration::from_secs(self.launch.config.initialization_timeout_secs)
            })
        {
            self.fail(ErrorCode::InitializationTimeout);
            return Ok(None);
        }
        let windows = JObjectArray::from(
            env.call_static_method("java/awt/Window", "getWindows", "()[Ljava/awt/Window;", &[])?
                .l()?,
        );
        let mut visible = HashSet::new();
        let mut factor_visible = false;
        let mut configuration_visible = false;
        if self.export_finished {
            self.export_finished = false;
            if let Some(window) = self.view_logs.take() {
                let window = env.new_local_ref(window.as_obj())?;
                let nodes = components(env, &window)?;
                return click(env, &nodes, "Cancel");
            }
        }
        for index in 0..env.get_array_length(&windows)? {
            let window = env.get_object_array_element(&windows, index)?;
            if !env.call_method(&window, "isShowing", "()Z", &[])?.z()? {
                continue;
            }
            let id = env
                .call_static_method(
                    "java/lang/System",
                    "identityHashCode",
                    "(Ljava/lang/Object;)I",
                    &[JValue::Object(&window)],
                )?
                .i()?;
            visible.insert(id);
            let nodes = components(env, &window)?;
            let snapshot = snapshot(env, &window, &nodes)?;
            let kind = snapshot.classify();
            if !self.launch.config.auto_login && self.reported.insert(id) {
                self.emit(EventKind::Output {
                    message: format!(
                        "Observed window [{}]; buttons: {:?}; text: {}",
                        snapshot.title, snapshot.buttons, snapshot.text
                    ),
                })?;
            }
            configuration_visible |= kind == Dialog::Configuration;
            factor_visible |= kind == Dialog::SecondFactorCode;
            if kind == Dialog::Connecting {
                self.connecting_seen = Some(Instant::now());
            }
            if self.state == State::Stopping {
                if kind == Dialog::ExitConfirmation {
                    return click(env, &nodes, "Yes");
                }
                continue;
            }
            if self.authentication_started.is_none() && kind.is_authentication() {
                self.begin_authentication()?;
            }
            // The main window has File/Close and Configure/Settings menu items.
            if snapshot.buttons.iter().any(|label| label == "Close")
                && snapshot.buttons.iter().any(|label| label == "Settings")
            {
                self.main_window = Some(env.new_global_ref(&window)?);
                if (!self.configured || self.restart_requested) && !self.config_opened {
                    // Gateway can show the main window before the second
                    // factor, with Settings disabled until the login completes.
                    let Some(settings) = find_button(env, &nodes, "Settings")? else {
                        continue;
                    };
                    if !env.call_method(&settings, "isEnabled", "()Z", &[])?.z()? {
                        continue;
                    }
                    // A startup splash or warning can precede functional menus.
                    let mut blocked = false;
                    for i in 0..env.get_array_length(&windows)? {
                        let other = env.get_object_array_element(&windows, i)?;
                        if !env.call_method(&other, "isShowing", "()Z", &[])?.z()? {
                            continue;
                        }
                        let other_nodes = components(env, &other)?;
                        let other_snapshot = self::snapshot(env, &other, &other_nodes)?;
                        if other_snapshot
                            .title
                            .to_lowercase()
                            .contains("starting application")
                            || matches!(
                                other_snapshot.classify(),
                                Dialog::PaperWarning | Dialog::Connecting
                            )
                            || other_snapshot.classify().is_authentication()
                        {
                            blocked = true;
                            break;
                        }
                    }
                    if blocked {
                        continue;
                    }
                    self.config_opened = true;
                    self.set_state(State::Configuring)?;
                    return Ok(Some(UiAction::Click(env.new_global_ref(settings)?)));
                }
                if self.export_stage == ExportStage::ApiPending {
                    self.export_stage = ExportStage::Idle;
                    self.export_deadline = None;
                    if find_button(env, &nodes, "API Logs")?.is_some() {
                        self.export_stage = ExportStage::Api;
                        self.export_deadline = Some(Instant::now() + Duration::from_secs(60));
                        return click(env, &nodes, "API Logs");
                    }
                }
                if self.export_requested
                    || (self.launch.config.export_logs
                        && self.configured
                        && self.view_logs.is_none()
                        && !self.handled.contains(&(id, Dialog::ViewLogs)))
                {
                    self.export_requested = false;
                    self.handled.insert((id, Dialog::ViewLogs));
                    if find_button(env, &nodes, "Gateway Logs")?.is_some() {
                        self.export_stage = ExportStage::Gateway;
                        self.export_deadline = Some(Instant::now() + Duration::from_secs(60));
                        return click(env, &nodes, "Gateway Logs");
                    }
                }
                continue;
            }
            if kind == Dialog::SecondFactor {
                factor_visible = true;
                let opened = *self.second_factor.entry(id).or_insert_with(Instant::now);
                if !self.handled.contains(&(id, kind)) {
                    if self.attempts >= self.launch.config.second_factor_attempts {
                        self.fail(ErrorCode::TwoFactorConfirmationTimeout);
                        return click(env, &nodes, "Cancel");
                    }
                    self.attempts += 1;
                    self.set_state(State::AwaitingSecondFactor)?;
                }
                if opened.elapsed()
                    >= Duration::from_secs(self.launch.config.second_factor_timeout_secs)
                {
                    if self.attempts >= self.launch.config.second_factor_attempts {
                        self.fail(ErrorCode::TwoFactorConfirmationTimeout);
                    } else {
                        self.retry_at = Some(
                            Instant::now() + Duration::from_secs(10 * u64::from(self.attempts)),
                        );
                    }
                    self.second_factor.remove(&id);
                    return click(env, &nodes, "Cancel");
                }
            }
            // A code prompt still open 15 s after a submission, with no
            // server progress window meanwhile, rejected the code. A second
            // rejection stops: more wrong codes can lock the account.
            if kind == Dialog::SecondFactorCode
                && self
                    .second_factor
                    .get(&id)
                    .is_some_and(|at| at.elapsed() >= Duration::from_secs(15))
                && self
                    .connecting_seen
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(3))
            {
                self.second_factor.remove(&id);
                self.handled.remove(&(id, kind));
                self.totp_rejections += 1;
                if self.totp_rejections >= 2 {
                    self.fail(ErrorCode::TotpRejected);
                    return click(env, &nodes, "Cancel");
                }
            }
            if self.handled.contains(&(id, kind))
                && (kind != Dialog::Login || self.retry_at.is_none())
            {
                continue;
            }
            if kind != Dialog::Unknown && self.announced.insert((id, kind)) {
                self.emit(EventKind::Dialog {
                    kind,
                    title: snapshot.title.clone(),
                })?;
            }
            match kind.action(&self.launch.config) {
                Action::Login => {
                    self.main_window = Some(env.new_global_ref(&window)?);
                    if !self.launch.config.auto_login {
                        self.handled.insert((id, kind));
                        continue;
                    }
                    if self.restarting {
                        // A restart child restores its session from a token
                        // and must not enter credentials. A login screen that
                        // stays means IBKR refused the token (weekly expiry).
                        let since = *self.restart_login_since.get_or_insert_with(Instant::now);
                        if since.elapsed() >= Duration::from_secs(120) {
                            self.emit(EventKind::Output {
                                message: "The restarted Gateway shows the login screen: its token was refused. Starting a new login.".into(),
                            })?;
                            return self.cold_restart(ErrorCode::AuthenticationRequired);
                        }
                        continue;
                    }
                    if (self.handled.contains(&(id, kind)) && self.retry_at.is_none())
                        || self.retry_at.is_some_and(|at| at > Instant::now())
                    {
                        continue;
                    }
                    if self.retry_at.take().is_some() {
                        self.handled.remove(&(id, kind));
                    }
                    if let Some(button) = find_button(env, &nodes, "IB API")?
                        && !env.call_method(&button, "isSelected", "()Z", &[])?.z()?
                    {
                        return Ok(Some(UiAction::Click(env.new_global_ref(button)?)));
                    }
                    let mode = if self.launch.config.trading_mode == TradingMode::Live {
                        "Live Trading"
                    } else {
                        "Paper Trading"
                    };
                    if let Some(button) = find_button(env, &nodes, mode)? {
                        if !env.call_method(&button, "isSelected", "()Z", &[])?.z()? {
                            return Ok(Some(UiAction::Click(env.new_global_ref(button)?)));
                        }
                    } else {
                        bail!("The selected trading mode button is absent");
                    }
                    let fields = text_fields(env, &nodes)?;
                    if fields.len() < 2 {
                        bail!("The login screen must have a username and a password field");
                    }
                    set_text(env, &fields[0], &self.credentials.username)?;
                    set_text(env, &fields[1], &self.credentials.password)?;
                    set_selected(env, &nodes, "Use SSL", true, false)?;
                    // Gateway enables Log In only once both fields have text,
                    // so look for it after filling them. A pass that cannot
                    // click marks nothing; the next pass fills and tries again.
                    let submit = click(
                        env,
                        &nodes,
                        if self.launch.config.trading_mode == TradingMode::Live {
                            "Log In"
                        } else {
                            "Paper Log In"
                        },
                    )?;
                    self.handled.insert((id, kind));
                    self.login_log = Some((
                        fs::metadata(self.launch.config.settings_dir.join("launcher.log"))
                            .map_or(0, |meta| meta.len()),
                        0,
                    ));
                    self.authentication_started.get_or_insert_with(Instant::now);
                    self.set_state(State::LoggingIn)?;
                    return Ok(submit);
                }
                Action::Configure => {
                    // Also when the dialog opened another way (an operator
                    // during login): closing it then reports Ready.
                    if self.state != State::Ready {
                        self.set_state(State::Configuring)?;
                    }
                    let action = self.configure(env, &window, &nodes)?;
                    self.handled.insert((id, kind));
                    return Ok(action);
                }
                Action::SelectDevice => {
                    select_device(
                        env,
                        &nodes,
                        if self.totp.is_some() {
                            "Mobile Authenticator"
                        } else {
                            "IB Key"
                        },
                    )?;
                    // OK can stay disabled until a device is selected.
                    let submit = click(env, &nodes, "OK")?;
                    self.handled.insert((id, kind));
                    return Ok(submit);
                }
                Action::EnterTotp => {
                    let Some(key) = &self.totp else {
                        self.fail(ErrorCode::SecurityDialogDetected);
                        return Ok(None);
                    };
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs();
                    let (code, valid_for) = ibkit_gateway_core::totp::code(key, now);
                    // Never resend a code IBKR has seen, and do not send one
                    // that can expire in transit; the next tick retries.
                    let step = now / ibkit_gateway_core::totp::PERIOD;
                    if self.totp_step == Some(step) || valid_for < 3 {
                        continue;
                    }
                    if self.attempts >= self.launch.config.second_factor_attempts {
                        self.fail(ErrorCode::TwoFactorConfirmationTimeout);
                        return click(env, &nodes, "Cancel");
                    }
                    let fields = text_fields(env, &nodes)?;
                    let field = fields
                        .first()
                        .context("The Mobile Authenticator code field is absent")?;
                    set_text(env, field, &code)?;
                    // OK can stay disabled until the field has a code. A code
                    // that is typed but not submitted is not spent.
                    let submit = click(env, &nodes, "OK")?;
                    self.attempts += 1;
                    self.totp_step = Some(step);
                    self.second_factor.insert(id, Instant::now());
                    self.handled.insert((id, kind));
                    self.set_state(State::AwaitingSecondFactor)?;
                    return Ok(submit);
                }
                Action::Wait => {
                    self.handled.insert((id, kind));
                }
                Action::Fail(code) => {
                    self.fail(code);
                    return Ok(None);
                }
                Action::ColdRestart => {
                    return self.cold_restart(if kind == Dialog::TokenExpired {
                        ErrorCode::AuthenticationRequired
                    } else {
                        ErrorCode::ConnectionLost
                    });
                }
                Action::Click(label) => {
                    if kind == Dialog::HotkeyConfirmation
                        && self
                            .shortcut_sent
                            .is_none_or(|at| at.elapsed() >= Duration::from_secs(10))
                    {
                        // Not ours: leave it for the operator.
                        self.handled.insert((id, kind));
                        continue;
                    }
                    let label = if kind == Dialog::ExistingSession {
                        // Take the session over each time, but at most three
                        // times an hour: more means two sessions fight.
                        self.takeovers
                            .retain(|at| at.elapsed() < Duration::from_secs(3600));
                        if self.takeovers.len() >= 3 {
                            self.emit(EventKind::Output {
                                message: "Another session took the IBKR session back three times in one hour; the agent stops taking it over. Close the other TWS, Gateway or IBKR Mobile session.".into(),
                            })?;
                            self.fail(ErrorCode::ExistingSessionDetected);
                            return Ok(None);
                        }
                        ["OK", "Continue Login", "Reconnect This Session"]
                            .into_iter()
                            .find(|label| snapshot.buttons.iter().any(|button| button == label))
                            .unwrap_or(label)
                    } else if matches!(kind, Dialog::IncomingConnection | Dialog::NewerVersion)
                        && label == "OK"
                        && !snapshot.buttons.iter().any(|button| button == "OK")
                    {
                        if kind == Dialog::NewerVersion {
                            "No"
                        } else {
                            "Yes"
                        }
                    } else {
                        label
                    };
                    // Find the button before any state changes: a pass that
                    // cannot click changes nothing and the next one retries.
                    let action = click(env, &nodes, label)?;
                    if kind == Dialog::ExistingSession {
                        self.takeovers.push(Instant::now());
                    }
                    if kind == Dialog::Relogin {
                        if self.attempts >= self.launch.config.second_factor_attempts {
                            self.fail(ErrorCode::TwoFactorConfirmationTimeout);
                            return Ok(None);
                        }
                        if self.retry_at.is_some_and(|at| at > Instant::now()) {
                            continue;
                        }
                        self.handled.retain(|(_, value)| *value != Dialog::Login);
                        self.begin_authentication()?;
                    }
                    if kind == Dialog::ViewLogs {
                        self.view_logs = Some(env.new_global_ref(&window)?);
                    }
                    if kind == Dialog::ExportFinished {
                        self.export_finished = true;
                        self.export_stage = if self.export_stage == ExportStage::Gateway {
                            ExportStage::ApiPending
                        } else {
                            ExportStage::Idle
                        };
                        if self.export_stage == ExportStage::Idle {
                            self.export_deadline = None;
                        }
                    }
                    if kind == Dialog::OrderWarning {
                        set_selected(
                            env,
                            &nodes,
                            "Don't display this message again.",
                            true,
                            false,
                        )?;
                        set_selected(env, &nodes, "Don't display this message again", true, false)?;
                    }
                    self.handled.insert((id, kind));
                    return Ok(action);
                }
                Action::Report => {
                    if kind != Dialog::Unknown {
                        // Known windows for an operator, such as a manual
                        // incoming-connection prompt: announce them once.
                        self.handled.insert((id, kind));
                    }
                    if snapshot.modal
                        && !snapshot.buttons.is_empty()
                        && !snapshot
                            .title
                            .to_lowercase()
                            .contains("starting application")
                    {
                        if self.reported.insert(id) {
                            self.emit(EventKind::Output {
                                message: format!(
                                    "Unhandled dialog: {}. {}",
                                    snapshot.title, snapshot.text
                                ),
                            })?;
                        }
                        // Never click an unknown dialog: a generic "Gateway"
                        // OK dialog can be the login progress indicator.
                        // Fail only if it blocks authentication for long.
                        let seen = *self.unknown_seen.entry(id).or_insert_with(Instant::now);
                        if kind == Dialog::Unknown
                            && self.state != State::Ready
                            && seen.elapsed()
                                >= Duration::from_secs(
                                    self.launch.config.unknown_dialog_timeout_secs,
                                )
                        {
                            self.fail(ErrorCode::UnknownMessageWindowDetected);
                            return Ok(None);
                        }
                    }
                }
            }
        }
        self.handled.retain(|(id, _)| visible.contains(id));
        self.reported.retain(|id| visible.contains(id));
        self.second_factor.retain(|id, _| visible.contains(id));
        self.unknown_seen.retain(|id, _| visible.contains(id));
        self.announced.retain(|(id, _)| visible.contains(id));
        if matches!(self.state, State::LoggingIn | State::AwaitingSecondFactor)
            && self.check_login_log()?
        {
            return Ok(None);
        }
        if !factor_visible && self.state == State::AwaitingSecondFactor && self.retry_at.is_none() {
            self.set_state(State::LoggingIn)?;
        }
        if self.configured && !configuration_visible && self.state == State::Configuring {
            self.set_state(if self.restart_requested {
                State::Restarting
            } else {
                State::Ready
            })?;
            self.attempts = 0;
            self.authentication_started = None;
            self.login_log = None;
            self.totp_rejections = 0;
            self.restart_login_since = None;
        }
        Ok(None)
    }

    /// Click Yes on Gateway's "Are you sure you want to exit?" dialog.
    fn confirm_exit(&mut self, env: &mut JNIEnv<'_>) -> Result<Option<UiAction>> {
        let windows = JObjectArray::from(
            env.call_static_method("java/awt/Window", "getWindows", "()[Ljava/awt/Window;", &[])?
                .l()?,
        );
        for index in 0..env.get_array_length(&windows)? {
            let window = env.get_object_array_element(&windows, index)?;
            if !env.call_method(&window, "isShowing", "()Z", &[])?.z()? {
                continue;
            }
            let nodes = components(env, &window)?;
            if snapshot(env, &window, &nodes)?.classify() == Dialog::ExitConfirmation {
                return click(env, &nodes, "Yes");
            }
        }
        Ok(None)
    }

    /// Gateway logs `AuthTimeoutMonitor-CCP: Timeout!` when IBKR's server
    /// does not answer the login, typically because another session holds
    /// the user's slot. One timeout can be a network delay that Gateway
    /// retries; a second one in the same login fails it. Returns true when
    /// the login failed.
    fn check_login_log(&mut self) -> Result<bool> {
        use std::io::{Read, Seek, SeekFrom};
        let Some((offset, timeouts)) = self.login_log else {
            return Ok(false);
        };
        let Ok(mut file) = fs::File::open(self.launch.config.settings_dir.join("launcher.log"))
        else {
            return Ok(false);
        };
        let length = file.metadata()?.len();
        if length < offset {
            self.login_log = Some((0, timeouts));
            return Ok(false);
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut buffer = Vec::new();
        file.take(1024 * 1024).read_to_end(&mut buffer)?;
        let Some(end) = buffer.iter().rposition(|byte| *byte == b'\n') else {
            return Ok(false);
        };
        let marker = b"AuthTimeoutMonitor-CCP: Timeout!";
        let found = buffer[..end]
            .windows(marker.len())
            .filter(|window| *window == marker)
            .count();
        let timeouts = timeouts.saturating_add(found.min(255) as u8);
        self.login_log = Some((offset + end as u64 + 1, timeouts));
        if found > 0 && timeouts < 2 {
            self.emit(EventKind::Output {
                message: "IBKR did not answer the login request in time; Gateway retries once before the agent reports AuthenticationNotAnswered.".into(),
            })?;
        }
        if timeouts >= 2 {
            self.fail(ErrorCode::AuthenticationNotAnswered);
            return Ok(true);
        }
        Ok(false)
    }

    fn configure<'a>(
        &mut self,
        env: &mut JNIEnv<'a>,
        window: &JObject<'a>,
        nodes: &[JObject<'a>],
    ) -> Result<Option<UiAction>> {
        let tree = nodes
            .iter()
            .find(|node| is(env, node, "javax/swing/JTree").unwrap_or(false))
            .context("The configuration tree is absent")?;
        select_tree(env, tree, &["API", "Settings"])?;
        let nodes = components(env, window)?;
        set_selected(
            env,
            &nodes,
            "Read-Only API",
            self.launch.config.read_only_api,
            true,
        )?;
        set_selected(
            env,
            &nodes,
            "Create API message log file",
            self.launch.config.api_log,
            true,
        )?;
        set_selected(
            env,
            &nodes,
            "Allow connections from localhost only",
            !self.launch.config.allow_remote_api,
            false,
        )?;
        set_selected(
            env,
            &nodes,
            "Use Account Groups with Allocation Methods",
            false,
            false,
        )?;
        // Gateway names the panels of these fields; the first text field is
        // the port in older layouts without names.
        let port = match named_text_field(env, &nodes, "Socket port")? {
            Some(field) => field,
            None => text_fields(env, &nodes)?
                .into_iter()
                .next()
                .context("The API port field is absent")?,
        };
        set_text(env, &port, &self.launch.config.api_port.to_string())?;
        if let Some(id) = self.launch.config.master_client_id {
            let field = named_text_field(env, &nodes, "Master API client ID")?
                .context("The Master API client ID field is absent")?;
            set_text(env, &field, &id.to_string())?;
        }
        // Remote clients must be allowed for a Kubernetes Service. Gateway still
        // asks for approval unless its trusted-IP list contains the client.
        select_tree(env, tree, &["API", "Precautions"])?;
        let nodes = components(env, window)?;
        for node in &nodes {
            if is(env, node, "javax/swing/JCheckBox")?
                && string_method(env, node, "getText")?.starts_with("Bypass")
            {
                env.call_method(
                    node,
                    "setSelected",
                    "(Z)V",
                    &[JValue::Bool(
                        self.launch.config.bypass_order_precautions.into(),
                    )],
                )?;
            }
        }
        select_tree(env, tree, &["Lock and Exit"])?;
        let nodes = components(env, window)?;
        set_selected(env, &nodes, "Auto restart", true, true)?;
        set_selected(env, &nodes, "Never lock Trader Workstation", true, false)?;
        let (hour, minute) = if self.restart_requested {
            let now = Local::now() + chrono::Duration::minutes(2);
            (now.hour(), now.minute())
        } else {
            (
                self.launch.config.restart_time[..2].parse::<u32>()?,
                self.launch.config.restart_time[3..].parse::<u32>()?,
            )
        };
        let fields = text_fields(env, &nodes)?;
        set_text(
            env,
            fields.first().context("The restart time field is absent")?,
            &format!(
                "{:02}:{minute:02}",
                if hour % 12 == 0 { 12 } else { hour % 12 }
            ),
        )?;
        set_selected(env, &nodes, if hour < 12 { "AM" } else { "PM" }, true, true)?;
        self.configured = true;
        self.emit(EventKind::Output {
            message: "Configuration settings updated.".into(),
        })?;
        click(env, &nodes, "OK")
    }
}

/// Class references cached for the life of the JVM: `is_instance_of` with a
/// class name performs a `FindClass` lookup on every call.
static CLASSES: OnceLock<Mutex<HashMap<&'static str, GlobalRef>>> = OnceLock::new();

fn is(env: &mut JNIEnv<'_>, node: &JObject<'_>, name: &'static str) -> jni::errors::Result<bool> {
    let cache = CLASSES.get_or_init(Default::default);
    let cached = cache.lock().ok().and_then(|map| map.get(name).cloned());
    let class = match cached {
        Some(class) => class,
        None => {
            let class = env.find_class(name)?;
            let global = env.new_global_ref(class)?;
            if let Ok(mut map) = cache.lock() {
                map.insert(name, global.clone());
            }
            global
        }
    };
    env.is_instance_of(node, <&JClass>::from(class.as_obj()))
}

fn components<'local>(
    env: &mut JNIEnv<'local>,
    root: &JObject<'local>,
) -> Result<Vec<JObject<'local>>> {
    let mut nodes = vec![env.new_local_ref(root)?];
    let mut index = 0;
    while index < nodes.len() {
        if nodes.len() > 4096 {
            bail!("The UI tree exceeds 4096 components. Use a supported Gateway release.");
        }
        let node = &nodes[index];
        let method = if is(env, node, "javax/swing/JMenu")? {
            Some("getMenuComponents")
        } else if is(env, node, "java/awt/Container")? {
            Some("getComponents")
        } else {
            None
        };
        if let Some(method) = method {
            let children = JObjectArray::from(
                env.call_method(node, method, "()[Ljava/awt/Component;", &[])?
                    .l()?,
            );
            for i in 0..env.get_array_length(&children)? {
                nodes.push(env.get_object_array_element(&children, i)?);
            }
        }
        index += 1;
    }
    Ok(nodes)
}

fn string_method(env: &mut JNIEnv<'_>, node: &JObject<'_>, method: &str) -> Result<String> {
    let value = env
        .call_method(node, method, "()Ljava/lang/String;", &[])?
        .l()?;
    if value.is_null() {
        return Ok(String::new());
    }
    Ok(env.get_string(&JString::from(value))?.into())
}

fn snapshot(
    env: &mut JNIEnv<'_>,
    window: &JObject<'_>,
    nodes: &[JObject<'_>],
) -> Result<WindowSnapshot> {
    let title = if is(env, window, "java/awt/Frame")? || is(env, window, "java/awt/Dialog")? {
        string_method(env, window, "getTitle")?
    } else {
        String::new()
    };
    let mut result = WindowSnapshot {
        title,
        modal: is(env, window, "java/awt/Dialog")?,
        ..Default::default()
    };
    for node in nodes {
        if is(env, node, "javax/swing/AbstractButton")? {
            result.buttons.push(string_method(env, node, "getText")?);
        }
        // Exclude ALL editable fields, including non-password credential fields.
        else if is(env, node, "javax/swing/JLabel")?
            || is(env, node, "javax/swing/JTextArea")?
            || is(env, node, "javax/swing/JEditorPane")?
        {
            let text = string_method(env, node, "getText")?;
            if result.text.len() < 32768 {
                result.text.push(' ');
                result.text.extend(text.chars().take(4096));
            }
        } else if is(env, node, "javax/swing/JOptionPane")? {
            let message = env
                .call_method(node, "getMessage", "()Ljava/lang/Object;", &[])?
                .l()?;
            if !message.is_null() && result.text.len() < 32768 {
                result
                    .text
                    .push_str(&string_method(env, &message, "toString")?);
            }
        }
    }
    Ok(result)
}

fn find_button<'local>(
    env: &mut JNIEnv<'local>,
    nodes: &[JObject<'local>],
    label: &str,
) -> Result<Option<JObject<'local>>> {
    for node in nodes {
        if is(env, node, "javax/swing/AbstractButton")?
            && string_method(env, node, "getText")? == label
        {
            return Ok(Some(env.new_local_ref(node)?));
        }
    }
    Ok(None)
}

fn click<'a>(env: &mut JNIEnv<'a>, nodes: &[JObject<'a>], label: &str) -> Result<Option<UiAction>> {
    let button = find_button(env, nodes, label)?
        .with_context(|| format!("The required [{label}] button is absent"))?;
    if !env.call_method(&button, "isEnabled", "()Z", &[])?.z()? {
        bail!("The required [{label}] button is disabled");
    }
    Ok(Some(UiAction::Click(env.new_global_ref(button)?)))
}

fn set_selected<'a>(
    env: &mut JNIEnv<'a>,
    nodes: &[JObject<'a>],
    label: &str,
    selected: bool,
    required: bool,
) -> Result<()> {
    if let Some(button) = find_button(env, nodes, label)? {
        env.call_method(
            button,
            "setSelected",
            "(Z)V",
            &[JValue::Bool(selected.into())],
        )?;
    } else if required {
        bail!("The required [{label}] setting is absent");
    }
    Ok(())
}

fn text_fields<'local>(
    env: &mut JNIEnv<'local>,
    nodes: &[JObject<'local>],
) -> Result<Vec<JObject<'local>>> {
    let mut fields = Vec::new();
    for node in nodes {
        if is(env, node, "javax/swing/JTextField")?
            && env.call_method(node, "isShowing", "()Z", &[])?.z()?
        {
            fields.push(env.new_local_ref(node)?);
        }
    }
    Ok(fields)
}

/// First text field inside the component that Gateway names `name`.
fn named_text_field<'local>(
    env: &mut JNIEnv<'local>,
    nodes: &[JObject<'local>],
    name: &str,
) -> Result<Option<JObject<'local>>> {
    for node in nodes {
        if string_method(env, node, "getName")?.eq_ignore_ascii_case(name) {
            for child in components(env, node)? {
                if is(env, &child, "javax/swing/JTextField")? {
                    return Ok(Some(child));
                }
            }
        }
    }
    Ok(None)
}

fn set_text(env: &mut JNIEnv<'_>, node: &JObject<'_>, value: &str) -> Result<()> {
    let value = env.new_string(value)?;
    env.call_method(
        node,
        "setText",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&value)],
    )?;
    Ok(())
}

fn select_tree(env: &mut JNIEnv<'_>, tree: &JObject<'_>, labels: &[&str]) -> Result<()> {
    let model = env
        .call_method(tree, "getModel", "()Ljavax/swing/tree/TreeModel;", &[])?
        .l()?;
    let mut current = env
        .call_method(&model, "getRoot", "()Ljava/lang/Object;", &[])?
        .l()?;
    let path = env.new_object_array(
        (labels.len() + 1) as i32,
        "java/lang/Object",
        JObject::null(),
    )?;
    env.set_object_array_element(&path, 0, &current)?;
    for (index, label) in labels.iter().enumerate() {
        let count = env
            .call_method(
                &model,
                "getChildCount",
                "(Ljava/lang/Object;)I",
                &[JValue::Object(&current)],
            )?
            .i()?;
        let mut found = None;
        for i in 0..count {
            let child = env
                .call_method(
                    &model,
                    "getChild",
                    "(Ljava/lang/Object;I)Ljava/lang/Object;",
                    &[JValue::Object(&current), JValue::Int(i)],
                )?
                .l()?;
            if string_method(env, &child, "toString")? == *label {
                found = Some(child);
                break;
            }
        }
        current = found.with_context(|| format!("The configuration node [{label}] is absent"))?;
        env.set_object_array_element(&path, (index + 1) as i32, &current)?;
    }
    let path = env.new_object(
        "javax/swing/tree/TreePath",
        "([Ljava/lang/Object;)V",
        &[JValue::Object(&path)],
    )?;
    env.call_method(
        tree,
        "setSelectionPath",
        "(Ljavax/swing/tree/TreePath;)V",
        &[JValue::Object(&path)],
    )?;
    Ok(())
}

/// Select the list entry that contains `device` (case-insensitive).
fn select_device(env: &mut JNIEnv<'_>, nodes: &[JObject<'_>], device: &str) -> Result<()> {
    let wanted = device.to_lowercase();
    for node in nodes {
        if !is(env, node, "javax/swing/JList")? {
            continue;
        }
        let model = env
            .call_method(node, "getModel", "()Ljavax/swing/ListModel;", &[])?
            .l()?;
        let count = env.call_method(&model, "getSize", "()I", &[])?.i()?;
        for index in 0..count {
            let value = env
                .call_method(
                    &model,
                    "getElementAt",
                    "(I)Ljava/lang/Object;",
                    &[JValue::Int(index)],
                )?
                .l()?;
            if string_method(env, &value, "toString")?
                .to_lowercase()
                .contains(&wanted)
            {
                env.call_method(node, "setSelectedIndex", "(I)V", &[JValue::Int(index)])?;
                return Ok(());
            }
        }
    }
    bail!(
        "The second factor list has no [{device}] entry. Enable that method for the user in Client Portal, or remove the TOTP secret to use IB Key, and start again."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ibkit_gateway_core::Config;

    #[test]
    fn a_second_unanswered_login_fails_with_its_own_code() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            state_dir: dir.path().join("state"),
            settings_dir: dir.path().join("settings"),
            auto_login: false,
            ..Default::default()
        };
        fs::create_dir_all(&config.settings_dir).unwrap();
        let log = config.settings_dir.join("launcher.log");
        fs::write(&log, "earlier AuthTimeoutMonitor-CCP: Timeout!\n").unwrap();
        let mut automation = Automation::new(
            AgentLaunch {
                session: "s".into(),
                config,
            },
            false,
        )
        .unwrap();
        // The login starts after the earlier timeout; it does not count.
        automation.login_log = Some((fs::metadata(&log).unwrap().len(), 0));
        automation.state = State::LoggingIn;
        let mut file = OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(file, "x - AuthTimeoutMonitor-CCP: Timeout!").unwrap();
        assert!(
            !automation.check_login_log().unwrap(),
            "one timeout is retried"
        );
        assert_ne!(automation.state, State::Failed);
        write!(file, "y - AuthTimeoutMonitor-CCP: Time").unwrap();
        assert!(
            !automation.check_login_log().unwrap(),
            "a partial line waits"
        );
        writeln!(file, "out!").unwrap();
        assert!(automation.check_login_log().unwrap());
        assert_eq!(
            automation.result.error_code,
            ErrorCode::AuthenticationNotAnswered
        );
    }
}
