// These tests start shell-script launchers and check Unix process groups and
// file modes; tests/windows.rs covers the Windows platform layer.
#![cfg(unix)]
//! Supervision faults found in review: a heartbeat gap, a hung JVM outside
//! the launcher, a crashed JVM, operator commands that fail, and pause.
use ibkit_gateway::{AgentStatus, Config, Control, Gateway, StartResult, State, send_control};
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

fn config(dir: &std::path::Path, script: &str) -> Config {
    let launcher = dir.join("launcher");
    fs::write(&launcher, script).unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700)).unwrap();
    Config {
        executable: launcher.clone(),
        agent: launcher,
        auto_login: false,
        state_dir: dir.join("state"),
        settings_dir: dir.join("settings"),
        shutdown_timeout_secs: 1,
        soft_restart_timeout_secs: 120,
        api_probe_interval_secs: 3600,
        api_failure_timeout_secs: 3600,
        ..Default::default()
    }
}

/// Write a Ready heartbeat of `pid` that is `age_secs` old.
fn beat(config: &Config, session: &str, pid: u32, age_secs: i64) {
    let status = AgentStatus {
        session: session.into(),
        pid,
        at: chrono::Utc::now() - chrono::Duration::seconds(age_secs),
        state: State::Ready,
        result: StartResult::default(),
        max_heap_mb: None,
    };
    fs::write(
        config.state_dir.join(format!("agents/{pid}.json")),
        serde_json::to_vec(&status).unwrap(),
    )
    .unwrap();
}

fn alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// A healthy JVM that misses one heartbeat window (GC pause, host suspend)
/// is not cold-restarted when it reports again. Runs for 125 s.
#[tokio::test]
async fn one_heartbeat_gap_does_not_fail_a_healthy_gateway() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path(), "#!/bin/sh\nexec sleep 300\n");
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.launch().await.unwrap();
    let session = gateway.status().session.clone();
    let jvm = std::process::id();
    beat(&config, &session, jvm, 0);
    assert_eq!(gateway.poll().await.unwrap(), State::Ready);
    beat(&config, &session, jvm, 4);
    assert_eq!(gateway.poll().await.unwrap(), State::Restarting);
    for _ in 0..125 {
        beat(&config, &session, jvm, 0);
        assert_eq!(
            gateway.poll().await.unwrap(),
            State::Ready,
            "{:?}",
            gateway.last_start_result()
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    fs::remove_file(config.state_dir.join(format!("agents/{jvm}.json"))).unwrap();
    gateway.stop().await.unwrap();
}

/// A JVM that crashed (nothing of the session runs) is reported after the
/// grace period, not after soft_restart_timeout_secs. Runs for 125 s.
#[tokio::test]
async fn a_crashed_jvm_is_reported_within_the_grace_period() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        soft_restart_timeout_secs: 3600,
        ..config(dir.path(), "#!/bin/sh\nexec sleep 300\n")
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.launch().await.unwrap();
    let session = gateway.status().session.clone();
    let mut jvm = std::process::Command::new("sleep")
        .arg("300")
        .spawn()
        .unwrap();
    beat(&config, &session, jvm.id(), 0);
    assert_eq!(gateway.poll().await.unwrap(), State::Ready);
    // The JVM and the launcher die.
    jvm.kill().unwrap();
    jvm.wait().unwrap();
    let group = gateway.status().process_group.unwrap();
    let _ = std::process::Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{group}")])
        .status();
    let started = std::time::Instant::now();
    let state = loop {
        let state = gateway.poll().await.unwrap();
        if state == State::Failed || started.elapsed() > Duration::from_secs(130) {
            break state;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    assert_eq!(state, State::Failed);
    assert_eq!(
        gateway.last_start_result().error_code,
        ibkit_gateway::ErrorCode::AgentDisconnected
    );
    gateway.stop().await.unwrap();
}

/// A soft restart before Ready is refused when it is sent; a command that
/// fails inside the controller is an event, not a controller error.
#[tokio::test]
async fn failed_operator_commands_do_not_stop_the_controller() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path(), "#!/bin/sh\nexec sleep 300\n");
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.launch().await.unwrap();
    gateway.poll().await.unwrap();
    let refused = send_control(&config.state_dir, Control::SoftRestart).unwrap_err();
    assert!(refused.to_string().contains("soft restart"), "{refused}");
    // A restart whose launch fails: the launcher disappears.
    send_control(&config.state_dir, Control::Restart).unwrap();
    fs::remove_file(&config.executable).unwrap();
    let mut events = gateway.events().subscribe();
    assert!(
        gateway.poll().await.is_ok(),
        "a failed command is no poll error"
    );
    let mut reported = false;
    while let Ok(event) = events.try_recv() {
        if let ibkit_gateway::EventKind::Output { message } = event.kind {
            reported |= message.contains("Restart command failed");
        }
    }
    assert!(reported, "the failed command was not reported");
    gateway.stop().await.unwrap();
}

/// A hung JVM that is not the direct child (the launcher exited, as after
/// Gateway's first daily restart) is killed by stop().
#[tokio::test]
async fn stop_kills_a_hung_jvm_outside_the_launcher() {
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("jvm.pid");
    let script = format!(
        "#!/bin/sh\nsleep 300 >/dev/null 2>&1 &\necho $! > {}\nexit 0\n",
        pidfile.display()
    );
    let config = config(dir.path(), &script);
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.launch().await.unwrap();
    while !pidfile.exists() || fs::read_to_string(&pidfile).unwrap().trim().is_empty() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let jvm: u32 = fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let session = gateway.status().session.clone();
    beat(&config, &session, jvm, 0);
    assert_eq!(gateway.poll().await.unwrap(), State::Ready);
    beat(&config, &session, jvm, 10);
    assert_eq!(gateway.poll().await.unwrap(), State::Restarting);
    gateway.stop().await.unwrap();
    assert_eq!(gateway.status().state, State::Exited);
    let survived = alive(jvm);
    if survived {
        let _ = std::process::Command::new("/bin/kill")
            .arg(jvm.to_string())
            .status();
    }
    assert!(!survived, "stop() left the hung JVM {jvm} running");
}

/// `pause` stops Gateway and holds across a controller restart until
/// `restart`.
#[tokio::test]
async fn pause_holds_across_controller_restarts_until_restart() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("launches");
    let config = config(
        dir.path(),
        &format!(
            "#!/bin/sh\nprintf 'started\\n' >> '{}'\nexec sleep 300\n",
            marker.display()
        ),
    );
    let launches = || {
        fs::read_to_string(&marker)
            .map(|text| text.lines().count())
            .unwrap_or(0)
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut recovery = ibkit_gateway::recovery::Recovery::default();
    gateway.resume(&mut recovery).await.unwrap();
    send_control(&config.state_dir, Control::Pause).unwrap();
    gateway.poll().await.unwrap();
    assert_eq!(gateway.status().state, State::Paused);
    assert!(!gateway.is_running());
    drop(gateway);
    let mut gateway = Gateway::new(config.clone()).unwrap();
    assert_eq!(gateway.resume(&mut recovery).await.unwrap(), State::Paused);
    let before = launches();
    send_control(&config.state_dir, Control::Restart).unwrap();
    gateway.poll_paused(&mut recovery).await.unwrap();
    assert_eq!(gateway.status().state, State::Starting);
    tokio::time::timeout(Duration::from_secs(5), async {
        while launches() == before {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("restart did not log in again");
    gateway.stop().await.unwrap();
    gateway.cancel_recovery(&mut recovery).unwrap();
}

/// A reload that changes only controller settings needs no new login.
#[tokio::test]
async fn a_controller_only_reload_keeps_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path(), "#!/bin/sh\nexec sleep 300\n");
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut recovery = ibkit_gateway::recovery::Recovery::default();
    gateway.resume(&mut recovery).await.unwrap();
    let session = gateway.status().session.clone();
    gateway
        .reconfigure(
            Config {
                alert_webhook_url: Some("https://hooks.example.com/ibkit".into()),
                recovery_delay_secs: 120,
                ..config.clone()
            },
            &mut recovery,
        )
        .await
        .unwrap();
    assert_eq!(gateway.status().session, session, "no new login");
    assert_eq!(gateway.config().recovery_delay_secs, 120);
    gateway
        .reconfigure(
            Config {
                api_port: 4100,
                ..config.clone()
            },
            &mut recovery,
        )
        .await
        .unwrap();
    assert_ne!(
        gateway.status().session,
        session,
        "a Gateway key logs in again"
    );
    gateway.stop().await.unwrap();
    gateway.cancel_recovery(&mut recovery).unwrap();
}

/// The agent needs Gateway's English texts: an existing jts.ini with another
/// language is switched to English, and other lines stay.
#[test]
fn an_existing_settings_file_gets_the_english_locale() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path(), "#!/bin/sh\nexec sleep 300\n");
    fs::create_dir_all(&config.settings_dir).unwrap();
    fs::write(
        config.settings_dir.join("jts.ini"),
        "[IBGateway]\nApiOnly=true\n\n[Logon]\nLocale=de\nUseSSL=true\n",
    )
    .unwrap();
    drop(Gateway::new(config.clone()).unwrap());
    let text = fs::read_to_string(config.settings_dir.join("jts.ini")).unwrap();
    assert!(
        text.contains("Locale=en") && !text.contains("Locale=de"),
        "{text}"
    );
    assert!(text.contains("ApiOnly=true"), "{text}");
}

/// A reload logs in from `halted` even when no key changed: the operator
/// corrected the cause (PUT /v1/credentials keeps the file paths).
#[tokio::test]
async fn a_reload_from_halted_logs_in_again() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        auto_recover: false,
        ..config(dir.path(), "#!/bin/sh\nexit 1\n")
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut recovery = ibkit_gateway::recovery::Recovery::default();
    gateway.resume(&mut recovery).await.unwrap();
    let state = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let state = gateway.poll_recovery(&mut recovery).await.unwrap();
            if state == State::Failed {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(state, State::Failed);
    assert!(!gateway.recover(&mut recovery).await.unwrap());
    gateway.halt(&recovery).await.unwrap();
    assert_eq!(gateway.status().state, State::Halted);
    let session = gateway.status().session.clone();
    gateway
        .reconfigure(config.clone(), &mut recovery)
        .await
        .unwrap();
    assert_ne!(
        gateway.status().session,
        session,
        "the reload logged in again"
    );
    gateway.stop().await.unwrap();
}
