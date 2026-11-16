use chrono::{TimeZone, Utc};
use ibkit_gateway_core::{
    Action, Config, Credentials, Dialog, ErrorCode, ExistingSession, WindowSnapshot,
    schedule::{self, Region},
};

#[test]
fn gateway_dialog_contracts_and_explicit_session_policy() {
    let cases = [
        (
            "API",
            "Accept incoming connection",
            vec!["Yes", "No"],
            Dialog::IncomingConnection,
        ),
        (
            "Trading Login Handoff",
            "",
            vec!["Disconnect Other Session"],
            Dialog::LoginHandoff,
        ),
        (
            "Exit",
            "Are you sure you want to exit?",
            vec!["Yes"],
            Dialog::ExitConfirmation,
        ),
        (
            "Login failed",
            "Soft token=0 received instead of expected permanent",
            vec!["OK"],
            Dialog::TokenExpired,
        ),
        (
            "Gateway",
            "Connection to server failed: authentication service unavailable",
            vec!["OK"],
            Dialog::ServerDisconnected,
        ),
        (
            "IBKR Gateway",
            "",
            vec!["IB API", "Paper Log In"],
            Dialog::Login,
        ),
        (
            "IB Gateway Configuration",
            "",
            vec![],
            Dialog::Configuration,
        ),
        (
            "Second Factor Authentication",
            "Select second factor device",
            vec![],
            Dialog::SecondFactorSelection,
        ),
        (
            "Second Factor Authentication",
            "Approve on your phone",
            vec![],
            Dialog::SecondFactor,
        ),
        (
            "Security Code Card Authentication",
            "",
            vec![],
            Dialog::SecurityCode,
        ),
        (
            "Unrecognized Username or Password",
            "",
            vec![],
            Dialog::LoginFailed,
        ),
        (
            "",
            "Connection to server failed: Server disconnected, please try again",
            vec![],
            Dialog::ServerDisconnected,
        ),
        (
            "",
            "Too many failed login attempts",
            vec![],
            Dialog::TooManyAttempts,
        ),
        ("Password Notice", "", vec![], Dialog::PasswordNotice),
        ("Login Messages", "", vec![], Dialog::AccountTasks),
        (
            "",
            "This is not a brokerage account",
            vec![],
            Dialog::PaperWarning,
        ),
        (
            "",
            "This version is no longer supported",
            vec![],
            Dialog::UnsupportedVersion,
        ),
        (
            "",
            "The minimum supported version is 10.52",
            vec![],
            Dialog::UnsupportedVersion,
        ),
        (
            "Existing session detected",
            "",
            vec![],
            Dialog::ExistingSession,
        ),
        ("Re-login is required", "", vec![], Dialog::Relogin),
        (
            "Financial Advisor Warning",
            "",
            vec![],
            Dialog::AdvisorWarning,
        ),
        ("Exit Session Setting", "", vec![], Dialog::ExitSettings),
        (
            "Notice",
            "API support is not available for accounts that support free trading.",
            vec![],
            Dialog::ApiUnavailable,
        ),
        (
            "",
            "You have elected to have your trading platform restart automatically",
            vec![],
            Dialog::RestartConfirmation,
        ),
        (
            "",
            "Soft token=0 received instead of expected permanent",
            vec![],
            Dialog::TokenExpired,
        ),
        (
            "",
            "Would you like to restart now?",
            vec![],
            Dialog::RestartNow,
        ),
        (
            "",
            "Bid, Ask and Last Size Display Update",
            vec![],
            Dialog::MarketData,
        ),
        ("Use SSL encryption", "", vec![], Dialog::Ssl),
        (
            "Cash Quantity Order Warning",
            "",
            vec!["Accept and Continue"],
            Dialog::OrderWarning,
        ),
        ("View Logs", "", vec![], Dialog::ViewLogs),
        ("Enter export filename", "", vec![], Dialog::ExportFilename),
        (
            "",
            "Finished exporting logs",
            vec![],
            Dialog::ExportFinished,
        ),
        (
            "A new vendor dialog",
            "Manual review is required",
            vec!["OK"],
            Dialog::Unknown,
        ),
    ];
    for (title, text, buttons, expected) in cases {
        let snapshot = WindowSnapshot {
            title: title.into(),
            text: text.into(),
            buttons: buttons.into_iter().map(String::from).collect(),
            modal: true,
        };
        assert_eq!(snapshot.classify(), expected, "{title}: {text}");
    }
    let mut config = Config::default();
    assert_eq!(
        Dialog::ExistingSession.action(&config),
        Action::Fail(ErrorCode::ExistingSessionDetected)
    );
    config.existing_session = ExistingSession::Takeover;
    assert_eq!(Dialog::ExistingSession.action(&config), Action::Click("OK"));
    config.accept_order_warnings = false;
    assert_eq!(Dialog::OrderWarning.action(&config), Action::Report);
    assert_eq!(Dialog::AdvisorWarning.action(&config), Action::Report);
    assert_eq!(Dialog::IncomingConnection.action(&config), Action::Report);
    config.incoming_connection = ibkit_gateway_core::IncomingConnection::Reject;
    assert_eq!(
        Dialog::IncomingConnection.action(&config),
        Action::Click("No")
    );
}

#[test]
fn configuration_rejects_invalid_inputs_and_preserves_secret_bytes() {
    let mut config = Config::default();
    for time in ["", "24:00", "12:60", "1:00", "é:00", "ab:cd"] {
        config.restart_time = time.into();
        assert!(config.validate().is_err(), "{time}");
    }
    config.restart_time = "00:00".into();
    assert!(config.validate().is_ok());
    for address in ["127.0.0.1:0", "127.0.0.1:4002"] {
        config.api_listen = Some(address.parse().unwrap());
        assert!(config.validate().is_err());
    }
    config.api_listen = Some("127.0.0.1:4004".parse().unwrap());
    assert!(config.validate().is_ok());
    config.recovery_delay_secs = u64::MAX;
    assert!(config.validate().is_err());
    config.recovery_delay_secs = 60;
    config.api_port = 0;
    assert!(config.validate().is_err());
    let dir = tempfile::tempdir().unwrap();
    let username = dir.path().join("username");
    let password = dir.path().join("password");
    std::fs::write(&username, "fixture-user\n").unwrap();
    std::fs::write(&password, " password with spaces \r\n").unwrap();
    config.username_file = Some(username);
    config.password_file = Some(password);
    let credentials = config.credentials().unwrap();
    assert_eq!(credentials.password, " password with spaces ");
    assert_eq!(format!("{credentials:?}"), "Credentials([REDACTED])");
    assert!(
        !credentials
            .redact("fixture-user used password:  password with spaces ")
            .contains("fixture-user")
    );
    let credentials = Credentials {
        username: "R".into(),
        password: "R秘密\"\\".into(),
    };
    assert_eq!(
        credentials.redact("R秘密\"\\ and R"),
        "[REDACTED] and [REDACTED]"
    );
}

#[test]
fn maintenance_windows_follow_dst_and_regional_resets() {
    // New York switches from UTC-4 to UTC-5 on 2026-11-01.
    for (month, day, hour) in [(10, 30, 23), (11, 6, 23)] {
        let local = chrono_tz::America::New_York
            .with_ymd_and_hms(2026, month, day, hour, 0, 0)
            .unwrap();
        assert!(schedule::is_within_weekend_server_reset_times(
            local.with_timezone(&Utc)
        ));
    }
    let weekday = Utc.with_ymd_and_hms(2026, 11, 3, 5, 0, 0).unwrap();
    assert!(schedule::is_within_scheduled_server_reset_times(
        weekday,
        Region::America
    ));
    assert!(schedule::is_within_scheduled_server_reset_times(
        weekday,
        Region::Europe
    ));
    assert!(!schedule::is_within_scheduled_server_reset_times(
        weekday,
        Region::Asia
    ));
    assert_eq!(Region::from_peer("zdc1_hb1.ibllc.com:4000"), Region::Europe);
    assert_eq!(Region::from_peer("mcgw1.ibllc.com.cn:4000"), Region::Asia);
    assert_eq!(
        Region::from_peer("new-server.ibllc.com:4000"),
        Region::America
    );
}
