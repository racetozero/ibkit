mod form;
mod log;
// The TUI tests check Unix file modes of saved credentials.
#[cfg(all(test, unix))]
mod tests;
mod ui;

use anyhow::{Context, Result, bail};
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers,
};
use form::{Form, Kind};
use futures_util::StreamExt;
use ibkit_gateway::{
    Config, Control, Credentials, EventKind, Gateway, State, Status, TradingMode, read_status,
    send_control,
};
use log::{Level, Log};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader, Lines},
    process::{Child, ChildStderr, ChildStdout, Command},
    time::Instant,
};

pub(super) struct Editor {
    pub field: usize,
    pub text: String,
    pub cursor: usize,
}
pub(super) enum Modal {
    Edit(Editor),
    Live,
    Quit,
    Keys,
    /// The selected log entry in full.
    Entry,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Overview,
    Settings,
}
enum Phase {
    Running,
    /// `sent`: the controller has the stop command. The queue keeps it, so it
    /// is sent once.
    Stopping {
        deadline: Instant,
        sent: bool,
    },
}

struct Session {
    child: Child,
    pid: u32,
    config: Config,
    credentials: Credentials,
    _private: tempfile::TempDir,
    stdout: Lines<BufReader<ChildStdout>>,
    stderr: Lines<BufReader<ChildStderr>>,
    stdout_closed: bool,
    stderr_closed: bool,
    phase: Phase,
}

impl Session {
    fn start(form: &Form) -> Result<Self> {
        let private = ibkit_gateway::platform::private_tempdir("ibkit-session-", None)?;
        let config = form.session_config(private.path())?;
        if !config.executable.is_file() {
            bail!(
                "No Gateway launcher at {}. Install the standalone IB Gateway (it goes to ~/Applications on macOS, ~/Jts on Linux), or open the Gateway section and set Gateway launcher, then press r again.",
                config.executable.display()
            );
        }
        if !config.agent.is_file() {
            bail!(
                "No native agent at {}. It ships next to the ibkit binary (build it with `cargo build -p ibkit-cli`), or open the Gateway section and set Native agent, then press r again.",
                config.agent.display()
            );
        }
        let credentials = config.credentials()?;
        let path = private.path().join("config.toml");
        ibkit_gateway::gateway::write_private(&path, toml::to_string(&config)?.as_bytes())?;
        let mut child = Command::new(std::env::current_exe()?).arg("gateway").arg("--config").arg(path).arg("run")
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).spawn()
            .context("Cannot start the controller. Use an executable ibkit binary and correct its permissions.")?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .context("The controller has no event output")?,
        )
        .lines();
        let stderr = BufReader::new(
            child
                .stderr
                .take()
                .context("The controller has no error output")?,
        )
        .lines();
        Ok(Self {
            pid: child.id().context("The new controller has no process ID")?,
            child,
            config,
            credentials,
            _private: private,
            stdout,
            stderr,
            stdout_closed: false,
            stderr_closed: false,
            phase: Phase::Running,
        })
    }

    fn control(&mut self, control: Control) -> Result<()> {
        let status = read_status(&self.config.state_dir)?;
        if status.controller_pid != self.pid {
            bail!(
                "The selected controller has not published its session. Wait for startup before you send this command."
            );
        }
        send_control(&self.config.state_dir, control)
    }

    fn stop(&mut self) {
        if matches!(self.phase, Phase::Running) {
            self.phase = Phase::Stopping {
                deadline: Instant::now()
                    + Duration::from_secs(self.config.shutdown_timeout_secs + 20),
                sent: false,
            };
        }
    }

    /// Queue one stop command. A send fails until the controller has
    /// published its session, so the caller repeats this until it succeeds.
    fn send_stop(&mut self) {
        if matches!(self.phase, Phase::Stopping { sent: false, .. })
            && self.control(Control::Stop).is_ok()
            && let Phase::Stopping { sent, .. } = &mut self.phase
        {
            *sent = true;
        }
    }

    /// The next line of the controller and the level of a line that is not
    /// an event: the controller writes its faults to standard error.
    async fn output(&mut self) -> Result<Option<(String, Level)>> {
        tokio::select! {
            line = self.stdout.next_line(), if !self.stdout_closed => {
                let line = line?;
                self.stdout_closed = line.is_none();
                Ok(line.map(|line| (line, Level::Info)))
            }
            line = self.stderr.next_line(), if !self.stderr_closed => {
                let line = line?;
                self.stderr_closed = line.is_none();
                Ok(line.map(|line| (line, Level::Warn)))
            }
            else => std::future::pending().await,
        }
    }

    async fn cleanup(self) -> Result<()> {
        let cleanup = match read_status(&self.config.state_dir) {
            Ok(status) if status.controller_pid == self.pid && status.state != State::Exited => {
                match Gateway::new(self.config.clone()) {
                    Ok(mut gateway) => gateway.stop_existing().await,
                    Err(error) => Err(error),
                }
            }
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        };
        if let Err(error) = cleanup {
            let directory = self._private.keep();
            bail!(
                "Gateway shutdown could not be confirmed: {} Session credential files remain private at {}. Close the Gateway session manually before you remove that directory or reset recovery.",
                self.credentials.redact(&error.to_string()),
                directory.display()
            );
        }
        Ok(())
    }
}

pub(super) struct App {
    pub form: Form,
    pub path: PathBuf,
    pub modal: Option<Modal>,
    pub message: String,
    pub error: bool,
    pub log: Log,
    pub view: View,
    /// Scroll offset of the settings list, kept between frames.
    pub fields: ratatui::widgets::ListState,
    pub status: Option<Status>,
    /// The state in `status` and when the TUI first saw it.
    pub since: Option<(State, Instant)>,
    /// The controller reported that the local API does not answer.
    pub api_down: bool,
    session: Option<Session>,
    quitting: bool,
}

impl App {
    fn new(form: Form, path: PathBuf) -> Self {
        Self {
            form,
            path,
            modal: None,
            message: "Enter credentials and settings, then press r to start. s saves them; ? lists every key.".into(),
            error: false,
            log: Log::default(),
            view: View::Settings,
            fields: Default::default(),
            status: None,
            since: None,
            api_down: false,
            session: None,
            quitting: false,
        }
    }

    fn message(&mut self, result: Result<&str>) {
        match result {
            Ok(message) => {
                self.error = false;
                self.message = message.into();
            }
            Err(error) => {
                self.error = true;
                self.message = self.form.redact(&format!("{error:#}"));
            }
        }
    }

    pub fn active(&self) -> bool {
        self.session.is_some()
    }
    pub fn active_mode(&self) -> Option<TradingMode> {
        self.session
            .as_ref()
            .map(|session| session.config.trading_mode)
    }
    pub fn stopping(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| matches!(s.phase, Phase::Stopping { .. }))
    }

    fn start(&mut self) {
        if self.active() {
            self.message(Err(anyhow::anyhow!(
                "Gateway is already active. Press x to stop it before you apply settings with r."
            )));
            return;
        }
        match Session::start(&self.form) {
            Ok(session) => {
                self.log.push(
                    chrono::Local::now(),
                    Level::Info,
                    format!("New session in {:?} mode.", session.config.trading_mode),
                    None,
                );
                self.session = Some(session);
                self.status = None;
                self.since = None;
                self.api_down = false;
                self.view = View::Overview;
                self.message(Ok(
                    "Starting Gateway. Approve IB Key on your phone when requested.",
                ));
            }
            Err(error) => self.message(Err(error)),
        }
    }

    fn key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.modal = None;
            self.quitting = true;
            if let Some(session) = &mut self.session {
                session.stop();
            }
            return;
        }
        if let Some(modal) = self.modal.take() {
            match modal {
                Modal::Edit(mut editor) => {
                    match key.code {
                        KeyCode::Esc => return,
                        KeyCode::Enter => {
                            self.form.fields[editor.field].value = editor.text;
                            self.form.dirty = true;
                            return;
                        }
                        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            editor.text.clear();
                            editor.cursor = 0;
                        }
                        KeyCode::Char(ch)
                            if !key
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                                && !ch.is_control() =>
                        {
                            if editor.text.len() + ch.len_utf8() > 4096 {
                                self.modal = Some(Modal::Edit(editor));
                                return;
                            }
                            let at = editor
                                .text
                                .char_indices()
                                .nth(editor.cursor)
                                .map_or(editor.text.len(), |(at, _)| at);
                            editor.text.insert(at, ch);
                            editor.cursor += 1;
                        }
                        KeyCode::Backspace if editor.cursor > 0 => {
                            let at = editor.text.char_indices().nth(editor.cursor - 1).unwrap().0;
                            editor.text.remove(at);
                            editor.cursor -= 1;
                        }
                        KeyCode::Delete if editor.cursor < editor.text.chars().count() => {
                            let at = editor.text.char_indices().nth(editor.cursor).unwrap().0;
                            editor.text.remove(at);
                        }
                        KeyCode::Left => editor.cursor = editor.cursor.saturating_sub(1),
                        KeyCode::Right => {
                            editor.cursor = (editor.cursor + 1).min(editor.text.chars().count())
                        }
                        KeyCode::Home => editor.cursor = 0,
                        KeyCode::End => editor.cursor = editor.text.chars().count(),
                        _ => {}
                    }
                    self.modal = Some(Modal::Edit(editor));
                }
                Modal::Live => match key.code {
                    KeyCode::Enter => self.start(),
                    KeyCode::Esc => {}
                    _ => self.modal = Some(Modal::Live),
                },
                Modal::Quit => match key.code {
                    KeyCode::Enter => {
                        self.quitting = true;
                        if let Some(session) = &mut self.session {
                            session.stop();
                        }
                    }
                    KeyCode::Esc => {}
                    _ => self.modal = Some(Modal::Quit),
                },
                // Any key closes a view-only window.
                Modal::Keys | Modal::Entry => {}
            }
            return;
        }
        if self.view == View::Overview && self.log.typing {
            self.log.key(key);
            return;
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return;
        }
        match (self.view, key.code) {
            (View::Overview, KeyCode::Enter) if self.log.selected().is_some() => {
                self.log.pin();
                self.modal = Some(Modal::Entry);
            }
            (View::Overview, _) if self.log.key(key) => {}
            (View::Settings, KeyCode::Left | KeyCode::Right) => {
                let count = ibkit_gateway::settings::SECTIONS.len();
                let step = if key.code == KeyCode::Right {
                    1
                } else {
                    count - 1
                };
                self.form.section = (self.form.section + step) % count;
                self.form.selected = 0;
                self.fields = Default::default();
            }
            (View::Settings, KeyCode::Up | KeyCode::Char('k')) => {
                self.form.selected = self.form.selected.saturating_sub(1)
            }
            (View::Settings, KeyCode::Down | KeyCode::Char('j')) => {
                self.form.selected = (self.form.selected + 1).min(self.form.rows().len() - 1)
            }
            (View::Settings, KeyCode::Enter | KeyCode::Char(' ')) => {
                let index = self.form.current();
                let field = &mut self.form.fields[index];
                match field.kind {
                    Kind::Choice(choices) => {
                        let next = (choices
                            .iter()
                            .position(|value| *value == field.value)
                            .unwrap_or(0)
                            + 1)
                            % choices.len();
                        field.value = choices[next].into();
                        self.form.dirty = true;
                    }
                    _ => {
                        self.modal = Some(Modal::Edit(Editor {
                            field: index,
                            cursor: field.value.chars().count(),
                            text: field.value.clone(),
                        }))
                    }
                }
            }
            (_, KeyCode::Tab | KeyCode::BackTab) => {
                self.view = match self.view {
                    View::Overview => View::Settings,
                    View::Settings => View::Overview,
                }
            }
            (_, KeyCode::Char('1')) => self.view = View::Overview,
            (_, KeyCode::Char('2')) => self.view = View::Settings,
            (_, KeyCode::Char('?')) => self.modal = Some(Modal::Keys),
            (_, KeyCode::Char('s')) => {
                let result = self.form.save(&self.path);
                self.message(result.map(|()| "Settings saved. Changes apply on the next Start."));
            }
            (_, KeyCode::Char('r')) => {
                if self.active() {
                    self.start();
                } else if self
                    .form
                    .config()
                    .is_ok_and(|config| config.trading_mode == TradingMode::Live)
                {
                    self.modal = Some(Modal::Live);
                } else {
                    self.start();
                }
            }
            (_, KeyCode::Char('x')) => {
                if let Some(session) = &mut self.session {
                    session.stop();
                    self.message(Ok(
                        "Stopping Gateway. Please wait for the controller to exit.",
                    ));
                }
            }
            (_, KeyCode::Char('c' | 't' | 'l' | 'n')) => {
                let control = match key.code {
                    KeyCode::Char('c') => Control::Restart,
                    KeyCode::Char('t') => Control::SoftRestart,
                    KeyCode::Char('n') => Control::ReconnectAccount,
                    _ => Control::ExportLogs,
                };
                if let Some(session) = &mut self.session {
                    let result = session.control(control);
                    self.message(result.map(|()| "Command sent to the active controller."));
                }
            }
            (_, KeyCode::Char('q') | KeyCode::Esc) => {
                if self.active() || self.form.dirty {
                    self.modal = Some(Modal::Quit);
                } else {
                    self.quitting = true;
                }
            }
            _ => {}
        }
    }

    fn paste(&mut self, value: &str) {
        if let Some(Modal::Edit(editor)) = &mut self.modal {
            // Reject control bytes as a unit: multiline passwords must not be
            // silently changed into a different credential.
            if value.chars().any(char::is_control) {
                self.message(Err(anyhow::anyhow!("You pasted control characters or multiple lines. Paste one line without control characters, then press Enter.")));
                return;
            }
            if editor.text.len().saturating_add(value.len()) > 4096 {
                self.message(Err(anyhow::anyhow!("You pasted more than the field limit. Use a single value of at most 4096 UTF-8 bytes and paste again.")));
                return;
            }
            let at = editor
                .text
                .char_indices()
                .nth(editor.cursor)
                .map_or(editor.text.len(), |(at, _)| at);
            editor.text.insert_str(at, value);
            editor.cursor += value.chars().count();
        }
    }

    async fn tick(&mut self) -> Result<()> {
        let Some(session) = &mut self.session else {
            return Ok(());
        };
        if let Ok(status) = read_status(&session.config.state_dir)
            && Some(status.controller_pid) == session.child.id()
        {
            if self.since.is_none_or(|(state, _)| state != status.state) {
                self.since = Some((status.state, Instant::now()));
                // The controller reports API loss again for the new state.
                self.api_down = false;
            }
            self.status = Some(status);
        }
        if let Phase::Stopping { deadline, .. } = session.phase {
            session.send_stop();
            if Instant::now() >= deadline {
                session.child.start_kill()?;
            }
        }
        if let Some(exit) = session.child.try_wait()? {
            // Pipes are drained before releasing the session credential files.
            if (!session.stdout_closed || !session.stderr_closed)
                && !matches!(session.phase, Phase::Stopping { deadline, .. } if Instant::now() >= deadline)
            {
                return Ok(());
            }
            self.status = None;
            self.since = None;
            self.session.take().unwrap().cleanup().await?;
            if exit.success() {
                self.message(Ok(
                    "Gateway stopped. Edit settings or press r for a new run.",
                ));
            } else {
                self.message(Err(anyhow::anyhow!("The controller stopped with {exit}. Read the event log and correct the reported cause before you start again.")));
            }
        }
        Ok(())
    }

    fn output(&mut self, line: String, plain: Level) {
        let clean = self
            .session
            .as_ref()
            .map_or(line.clone(), |session| session.credentials.redact(&line));
        let (at, level, text, detail) = match serde_json::from_str::<ibkit_gateway::Event>(&clean) {
            Ok(event) => {
                let detail = serde_json::to_string_pretty(&event).ok();
                let (level, text) = match event.kind {
                    EventKind::StateChanged { state } => (
                        match state {
                            State::Ready => Level::Good,
                            State::Failed | State::Halted => Level::Error,
                            State::Recovering => Level::Warn,
                            _ => Level::Info,
                        },
                        format!("State → {}", ui::state_name(state)),
                    ),
                    EventKind::Dialog { kind, title } => {
                        (Level::Info, format!("Gateway dialog {kind:?}: {title}"))
                    }
                    EventKind::Error { result } => (Level::Error, result.error_message),
                    EventKind::Output { message } => (Level::Info, message),
                    EventKind::Halted { message, .. } => (Level::Error, message),
                    EventKind::RecoveryScheduled {
                        attempt,
                        retry_at,
                        reason,
                    } => (
                        Level::Warn,
                        format!(
                            "Retry {attempt} at {}: {}",
                            retry_at.with_timezone(&chrono::Local).format("%H:%M:%S"),
                            reason.message()
                        ),
                    ),
                    EventKind::Started => (Level::Info, "Controller started.".into()),
                    EventKind::Exited { code } => match code {
                        Some(0) | None => (Level::Info, "Gateway exited.".into()),
                        Some(code) => (Level::Warn, format!("Gateway exited with code {code}.")),
                    },
                    EventKind::Restarted => (Level::Good, "Gateway restart complete.".into()),
                    EventKind::ApiUnavailable => {
                        self.api_down = true;
                        (
                            Level::Warn,
                            "The local API does not answer; the failure grace period runs.".into(),
                        )
                    }
                    EventKind::ApiRestored => {
                        self.api_down = false;
                        (Level::Good, "The local API answers again.".into())
                    }
                };
                (event.at.with_timezone(&chrono::Local), level, text, detail)
            }
            Err(_) => (chrono::Local::now(), plain, clean, None),
        };
        let text = self
            .form
            .redact(&text)
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        let detail = detail.map(|detail| self.form.redact(&detail));
        self.log.push(at, level, text, detail);
    }
}

struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = crossterm::execute!(io::stdout(), DisableBracketedPaste);
        ratatui::restore();
    }
}

pub async fn run(path: PathBuf) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!(
            "You opened the TUI without a terminal. Run ibkit gateway tui in an interactive terminal, or use the existing config-file commands."
        );
    }
    let path = std::path::absolute(path)?;
    let form = Form::load(&path)?;
    let mut app = App::new(form, path);
    let mut signals = ibkit_gateway::platform::Signals::new()?;
    let mut terminal = ratatui::try_init()?;
    let _restore = Restore;
    crossterm::execute!(io::stdout(), EnableBracketedPaste)?;
    let mut events = EventStream::new();
    let mut timer = tokio::time::interval(Duration::from_millis(150));
    let result: Result<()> = async {
        loop {
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
            if app.quitting && !app.active() { return Ok(()); }
            tokio::select! {
                _ = timer.tick() => app.tick().await?,
                signal = signals.next() => if !matches!(signal, ibkit_gateway::platform::Signal::Reload) {
                    app.quitting = true;
                    if let Some(session) = &mut app.session { session.stop(); }
                },
                output = async { match &mut app.session { Some(session) => session.output().await, None => std::future::pending().await } } => {
                    if let Some((line, level)) = output? { app.output(line, level); }
                },
                event = events.next() => match event {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        if terminal.size()?.width >= ui::MIN_WIDTH && terminal.size()?.height >= ui::MIN_HEIGHT
                            || matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
                            || key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
                        { app.key(key); }
                    }
                    Some(Ok(Event::Paste(value))) => app.paste(&value),
                    Some(Err(error)) => return Err(error.into()),
                    None => { app.quitting = true; if let Some(session) = &mut app.session { session.stop(); } },
                    _ => {},
                },
            }
        }
    }.await;
    if let Some(mut session) = app.session.take() {
        session.stop();
        let deadline =
            Instant::now() + Duration::from_secs(session.config.shutdown_timeout_secs + 20);
        while session.child.try_wait()?.is_none() && Instant::now() < deadline {
            session.send_stop();
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if session.child.try_wait()?.is_none() {
            session.child.kill().await?;
        }
        session.cleanup().await?;
    }
    result
}
