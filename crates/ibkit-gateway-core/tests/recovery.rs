use chrono::{Duration, TimeZone, Utc};
use ibkit_gateway_core::{
    Config, ErrorCode,
    recovery::{Recovery, RecoveryState},
};

#[test]
fn recovery_has_a_budget_backoff_and_maintenance_barrier() {
    let config = Config {
        auto_recover: true,
        recovery_attempts: 2,
        recovery_delay_secs: 10,
        ..Default::default()
    };
    let now = Utc.with_ymd_and_hms(2026, 11, 3, 17, 0, 0).unwrap();
    let mut recovery = Recovery::default();
    let (_, first) = recovery
        .schedule(&config, ErrorCode::ConnectionLost, now)
        .unwrap();
    assert_eq!(first, now + Duration::seconds(10));
    assert!(
        recovery
            .schedule(
                &config,
                ErrorCode::ConnectionLost,
                now + Duration::seconds(1)
            )
            .is_none(),
        "A pending retry must keep its deadline and budget"
    );
    assert_eq!(
        recovery.state,
        RecoveryState::Waiting {
            retry_at: first,
            reason: ErrorCode::ConnectionLost,
        }
    );
    assert!(!recovery.can_retry(&config, now));
    assert!(recovery.can_retry(&config, first));
    recovery.retry_started();
    assert_eq!(recovery.state, RecoveryState::Running);
    let (_, second) = recovery
        .schedule(&config, ErrorCode::AuthenticationRequired, first)
        .unwrap();
    assert_eq!(second, first + Duration::seconds(20));
    recovery.retry_started();
    assert!(
        recovery
            .schedule(&config, ErrorCode::ConnectionLost, second)
            .is_none()
    );
    assert_eq!(recovery.state, RecoveryState::Exhausted);
    assert!(
        recovery
            .schedule(
                &config,
                ErrorCode::ConnectionLost,
                second + Duration::hours(1)
            )
            .is_some()
    );
    let saturday = Utc.with_ymd_and_hms(2026, 11, 7, 17, 0, 0).unwrap();
    recovery = Recovery::default();
    recovery
        .schedule(&config, ErrorCode::ConnectionLost, saturday)
        .unwrap();
    assert!(!recovery.can_retry(&config, saturday + Duration::minutes(1)));
}

#[test]
fn recovery_never_retries_credentials_or_manual_account_tasks() {
    let now = Utc::now();
    let config = Config {
        auto_recover: true,
        ..Default::default()
    };
    for reason in [
        ErrorCode::None,
        ErrorCode::LoginFailed,
        ErrorCode::SecurityDialogDetected,
        ErrorCode::UnsupportedVersion,
        ErrorCode::ApiSupportNotAvailable,
        ErrorCode::LoginFailedAccountTasksRequired,
        ErrorCode::InvalidConfiguration,
        ErrorCode::JavaNotFound,
    ] {
        let mut recovery = Recovery::default();
        assert!(
            recovery.schedule(&config, reason, now).is_none(),
            "{reason:?}"
        );
        assert_eq!(
            recovery.state,
            if reason == ErrorCode::None {
                RecoveryState::Idle
            } else {
                RecoveryState::Blocked { reason }
            }
        );
    }
    let disabled = Config {
        auto_recover: false,
        ..Default::default()
    };
    assert!(
        Recovery::default()
            .schedule(&disabled, ErrorCode::ConnectionLost, now)
            .is_none()
    );
}

#[test]
fn default_recovery_retries_without_limit_with_capped_doubling() {
    let config = Config::default();
    assert!(config.auto_recover && config.recovery_attempts == 0);
    let mut now = Utc.with_ymd_and_hms(2026, 11, 3, 17, 0, 0).unwrap();
    let mut recovery = Recovery::default();
    let mut delays = Vec::new();
    for _ in 0..12 {
        let (_, retry_at) = recovery
            .schedule(&config, ErrorCode::ConnectionLost, now)
            .expect("transient faults always retry by default");
        delays.push((retry_at - now).num_seconds());
        recovery.retry_started();
        now = retry_at;
    }
    assert_eq!(&delays[..6], [60, 120, 240, 480, 960, 1800]);
    assert!(delays[6..].iter().all(|delay| *delay == 1800));
    assert_eq!(recovery.attempts(), 12);
    recovery.validate().unwrap();

    // A held session, a missed phone approval and a refused existing session
    // wait longer than the first back-off step.
    for (reason, minimum) in [
        (ErrorCode::AuthenticationNotAnswered, 1200),
        (ErrorCode::TwoFactorConfirmationTimeout, 300),
        (ErrorCode::ExistingSessionDetected, 3600),
    ] {
        let mut recovery = Recovery::default();
        let (_, retry_at) = recovery.schedule(&config, reason, now).unwrap();
        assert_eq!((retry_at - now).num_seconds(), minimum, "{reason:?}");
        recovery.validate().unwrap();
    }
}
