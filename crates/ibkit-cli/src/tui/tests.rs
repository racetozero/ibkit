use super::*;
use ratatui::{Terminal, backend::TestBackend};
use std::{fs, os::unix::fs::PermissionsExt};

fn app(path: PathBuf) -> App {
    App::new(Form::load(&path).unwrap(), path)
}

/// The screen as text, one string per row.
fn screen(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                + "\n"
        })
        .collect()
}

fn key(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn terminal_forms_mask_credentials_and_edit_unicode_without_committing_cancelled_input() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path().join("config.toml"));
    key(&mut app, KeyCode::Enter);
    app.paste("fixture-user");
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    app.paste("秘密-password");
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Backspace);
    key(&mut app, KeyCode::Char('x'));
    let display = screen(&mut app, 110, 34);
    assert!(display.contains("Password"));
    assert!(display.contains("•••••"));
    assert!(
        !display.contains("秘密")
            && !display.contains("-password")
            && !display.contains("fixture-user")
    );
    key(&mut app, KeyCode::Esc);
    assert!(
        app.form.fields[1].value.is_empty(),
        "Escape must discard the edited secret"
    );
    key(&mut app, KeyCode::Enter);
    app.paste("秘密-password");
    for ch in "srxctlq".chars() {
        key(&mut app, KeyCode::Char(ch));
    }
    app.paste("\nunsafe");
    assert!(app.error);
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.form.credentials().unwrap().unwrap().password,
        "秘密-passwordsrxctlq"
    );
    let display = screen(&mut app, 40, 12);
    assert!(display.contains("Enlarge"));
    assert!(!display.contains("秘密") && !display.contains("fixture-user"));
}

#[test]
fn saved_settings_exclude_secrets_and_opt_in_creates_private_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut form = Form::load(&path).unwrap();
    form.fields[0].value = "fixture-user".into();
    form.fields[1].value = " fixture-password ".into();
    form.save(&path).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("fixture-user") && !text.contains("fixture-password"));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let config = Config::load(&path).unwrap();
    assert!(config.username_file.is_none() && config.password_file.is_none());
    let private = tempfile::tempdir().unwrap();
    let session = form.session_config(private.path()).unwrap();
    assert_eq!(
        session.credentials().unwrap().password,
        " fixture-password "
    );
    assert_eq!(Config::load(&path).unwrap().password_file, None);
    form.fields
        .iter_mut()
        .find(|field| field.key == "_remember")
        .unwrap()
        .value = "true".into();
    form.save(&path).unwrap();
    let config = Config::load(&path).unwrap();
    assert_eq!(config.credentials().unwrap().password, " fixture-password ");
    let file = config.password_file.unwrap();
    form.save(&path).unwrap();
    assert_eq!(
        Config::load(&path).unwrap().password_file,
        Some(file.clone()),
        "Unchanged credentials must reuse their saved files"
    );
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(file.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(
        !fs::read_to_string(&path)
            .unwrap()
            .contains("fixture-password")
    );
}

#[test]
fn a_stopped_workspace_never_displays_a_previous_ready_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path().join("config.toml"));
    app.status = Some(Status {
        session: "previous".into(),
        controller_pid: 123,
        at: chrono::Utc::now(),
        state: State::Ready,
        agents: vec![],
        last_start_result: Default::default(),
        recovery: None,
        process_group: None,
    });
    for view in [View::Settings, View::Overview] {
        app.view = view;
        let display = screen(&mut app, 110, 34);
        assert!(display.contains("Stopped"));
        assert!(!display.contains("Ready"), "{display}");
    }
}

#[tokio::test]
async fn unknown_status_preserves_session_credentials_and_reports_incomplete_cleanup() {
    for malformed in [false, true] {
        let state = tempfile::tempdir().unwrap();
        if malformed {
            fs::write(state.path().join("status.json"), b"{broken").unwrap();
        }
        let private = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let retained = private.path().to_owned();
        fs::write(private.path().join("password"), b"fixture-secret").unwrap();
        let mut child = Command::new("true")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        let stderr = BufReader::new(child.stderr.take().unwrap()).lines();
        child.wait().await.unwrap();
        let session = Session {
            child,
            pid,
            config: Config {
                state_dir: state.path().into(),
                auto_login: false,
                ..Default::default()
            },
            credentials: Credentials {
                username: "fixture-user".into(),
                password: "fixture-secret".into(),
            },
            _private: private,
            stdout,
            stderr,
            stdout_closed: true,
            stderr_closed: true,
            phase: Phase::Running,
        };
        let error = session.cleanup().await.unwrap_err().to_string();
        assert!(error.contains("could not be confirmed"));
        assert!(!error.contains("fixture-secret"));
        assert_eq!(
            fs::read(retained.join("password")).unwrap(),
            b"fixture-secret"
        );
        fs::remove_dir_all(retained).unwrap();
    }
}

#[test]
fn form_round_trip_preserves_settings_and_failed_save_preserves_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    // Explicit paths: the TUI presets the launcher and agent found on this
    // machine only in place of the defaults.
    let config = Config {
        executable: dir.path().join("launcher"),
        agent: dir.path().join("agent.so"),
        api_port: 15555,
        api_listen: Some("127.0.0.1:15556".parse().unwrap()),
        auto_recover: true,
        shutdown_timeout_secs: 4,
        ..Default::default()
    };
    fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    let mut form = Form::load(&path).unwrap();
    assert_eq!(
        toml::to_string(&form.config().unwrap()).unwrap(),
        toml::to_string(&config).unwrap()
    );
    let original = fs::read(&path).unwrap();
    let port = form
        .fields
        .iter()
        .position(|field| field.key == "api_port")
        .unwrap();
    form.fields[port].value = "0".into();
    assert!(form.save(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    form.fields[port].value = "15555".into();
    fs::write(&path, "# external edit\n").unwrap();
    assert!(form.save(&path).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "# external edit\n");
    fs::write(&path, "invalid = [").unwrap();
    assert!(Form::load(&path).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "invalid = [");
}

#[test]
fn live_start_needs_confirmation_and_events_never_show_entered_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path().join("config.toml"));
    app.form
        .fields
        .iter_mut()
        .find(|field| field.key == "trading_mode")
        .unwrap()
        .value = "live".into();
    app.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert!(app.modal.is_none() && !app.active());
    key(&mut app, KeyCode::Char('r'));
    assert!(matches!(app.modal, Some(Modal::Live)));
    assert!(!app.active());
    key(&mut app, KeyCode::Esc);
    assert!(app.modal.is_none() && !app.active());
    app.form.fields[0].value = "fixture-user".into();
    app.form.fields[1].value = "fixture-secret".into();
    for round in 0..log::CAPACITY + 10 {
        app.output(
            format!("{round} fixture-user used fixture-secret"),
            Level::Info,
        );
    }
    assert_eq!(app.log.len(), log::CAPACITY);
    assert!(app.log.view().iter().all(
        |entry| !entry.text.contains("fixture-user") && !entry.text.contains("fixture-secret")
    ));
}

/// The log follows new events until the operator scrolls up, then stays on
/// the selected event while new ones arrive and old ones are dropped.
#[test]
fn the_event_log_scrolls_filters_and_keeps_its_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path().join("config.toml"));
    app.view = View::Overview;
    for round in 0..log::CAPACITY {
        app.output(format!("line {round}"), Level::Info);
    }
    app.output("line repeated".into(), Level::Info);
    app.output("line repeated".into(), Level::Info);
    assert_eq!(
        app.log.selected().unwrap().repeats,
        2,
        "identical lines merge"
    );
    assert!(app.log.following());
    key(&mut app, KeyCode::Up);
    key(&mut app, KeyCode::PageUp);
    assert!(!app.log.following());
    let selected = app.log.selected().unwrap().text.clone();
    app.output("line newer".into(), Level::Info);
    assert_eq!(
        app.log.selected().unwrap().text,
        selected,
        "a new event must not move the selection"
    );
    let display = screen(&mut app, 110, 34);
    assert!(
        display.contains(&selected) && display.contains("End follows"),
        "{display}"
    );
    key(&mut app, KeyCode::Enter);
    assert!(matches!(app.modal, Some(Modal::Entry)));
    assert!(screen(&mut app, 110, 34).contains(" Event "));
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::End);
    assert!(app.log.following());
    // Filter by text, then show problems only.
    key(&mut app, KeyCode::Char('/'));
    for ch in "newer".chars() {
        key(&mut app, KeyCode::Char(ch));
    }
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.log.view().len(), 1, "{}", screen(&mut app, 110, 34));
    app.output(r#"{"at":"2026-01-01T00:00:00Z","session":"s","pid":1,"event":"halted","reason":"InvalidConfiguration","exhausted":false,"message":"broken"}"#.into(), Level::Info);
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('e'));
    let view = app.log.view();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].text, "broken");
    assert!(
        view[0]
            .detail
            .as_ref()
            .unwrap()
            .contains("InvalidConfiguration")
    );
    drop(view);
    // Esc clears the filter before it opens the quit window.
    key(&mut app, KeyCode::Esc);
    assert!(app.modal.is_none() && !app.log.problems);
    assert!(screen(&mut app, 76, 22).contains("Events"));
}

#[test]
fn totp_secret_is_checked_and_kept_out_of_the_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut form = Form::load(&path).unwrap();
    let field = |form: &mut Form, key: &str| -> usize {
        form.fields
            .iter()
            .position(|field| field.key == key)
            .unwrap()
    };
    let totp = field(&mut form, "_totp_secret");
    form.fields[totp].value = "123456".into();
    let error = form.save(&path).unwrap_err().to_string();
    assert!(error.contains("base32"), "{error}");
    // "12345678901234567890" in base32.
    form.fields[totp].value = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".into();
    let private = tempfile::tempdir().unwrap();
    let session = form.session_config(private.path()).unwrap();
    let file = session.totp_secret_file.clone().unwrap();
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        session.totp_key().unwrap().unwrap(),
        b"12345678901234567890"
    );
    let remember = field(&mut form, "_remember");
    form.fields[remember].value = "true".into();
    form.save(&path).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("GEZDGNBV"), "the secret must not enter TOML");
    assert!(Config::load(&path).unwrap().totp_secret_file.is_some());
}

/// Credentials from files or the environment are not in the form; the
/// session card must still hide them in the controller's error text.
#[tokio::test]
async fn the_session_card_hides_credentials_that_are_not_in_the_form() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path().join("config.toml"));
    let mut child = Command::new("sleep")
        .arg("30")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap()).lines();
    let stderr = BufReader::new(child.stderr.take().unwrap()).lines();
    app.session = Some(Session {
        child,
        pid,
        config: Config::default(),
        credentials: Credentials {
            username: "fixture-user".into(),
            password: "fixture-secret".into(),
        },
        _private: tempfile::tempdir().unwrap(),
        stdout,
        stderr,
        stdout_closed: true,
        stderr_closed: true,
        phase: Phase::Running,
    });
    app.status = Some(Status {
        session: "session".into(),
        controller_pid: pid,
        at: chrono::Utc::now(),
        state: State::Halted,
        agents: vec![],
        last_start_result: ibkit_gateway::StartResult {
            error_code: ibkit_gateway::ErrorCode::InvalidConfiguration,
            error_message: "Gateway rejected fixture-user with fixture-secret.".into(),
        },
        recovery: None,
        process_group: None,
    });
    app.view = View::Overview;
    let display = screen(&mut app, 110, 34);
    assert!(
        display.contains("Halted") && display.contains("Gateway rejected"),
        "{display}"
    );
    assert!(!display.contains("fixture-user") && !display.contains("fixture-secret"));
}
