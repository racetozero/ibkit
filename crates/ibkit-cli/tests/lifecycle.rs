// These tests start shell-script launchers and check Unix process groups and
// file modes; tests/windows.rs covers the Windows platform layer.
#![cfg(unix)]
use ibkit_gateway::{Config, Gateway, State, api_handshake};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::Command,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn session_config(dir: &std::path::Path) -> Config {
    let launcher = dir.join("launcher");
    fs::write(&launcher, "#!/bin/sh\nexec sleep 60\n").unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700)).unwrap();
    Config {
        executable: launcher.clone(),
        agent: launcher,
        auto_login: false,
        state_dir: dir.join("state"),
        settings_dir: dir.join("settings"),
        shutdown_timeout_secs: 1,
        ..Default::default()
    }
}

/// A launcher that leaves a JVM-like process behind in its group when the
/// launcher itself is killed, as a controller crash leaves Gateway. It
/// touches `forked` once the background process exists.
fn surviving_jvm_config(dir: &std::path::Path) -> Config {
    let config = session_config(dir);
    fs::write(
        &config.executable,
        format!(
            "#!/bin/sh\nsleep 60 &\ntouch '{}'\nexec sleep 60\n",
            dir.join("forked").display()
        ),
    )
    .unwrap();
    config
}

/// Wait until `condition` holds (5 s at most).
async fn eventually(condition: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("condition not reached in 5 s");
}

/// Kill the launcher group that the last controller saved.
fn kill_saved_group(config: &Config) {
    if let Ok(status) = ibkit_gateway::read_status(&config.state_dir)
        && let Some(group) = status.process_group
    {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{group}")])
            .status();
    }
}

#[tokio::test]
async fn cli_queues_shutdown_during_display_start_and_flushes_exit_event() {
    let dir = tempfile::tempdir().unwrap();
    let config = session_config(dir.path());
    let display = dir.path().join("Xvfb");
    fs::write(
        &display,
        "#!/bin/sh\ntouch \"$IBKIT_TEST_DISPLAY_STARTED\"\nwhile [ ! -e \"$IBKIT_TEST_DISPLAY_RELEASE\" ]; do sleep 0.01; done\nprintf '90\\n'\nexec sleep 60\n",
    )
    .unwrap();
    fs::set_permissions(&display, fs::Permissions::from_mode(0o700)).unwrap();
    let config_path = dir.path().join("config.toml");
    fs::write(
        &config_path,
        format!(
            "executable = {:?}\nagent = {:?}\nstate_dir = {:?}\nsettings_dir = {:?}\nauto_login = false\nmanage_display = true\nshutdown_timeout_secs = 1\n",
            config.executable, config.agent, config.state_dir, config.settings_dir,
        ),
    )
    .unwrap();
    let marker = dir.path().join("display-started");
    let release = dir.path().join("display-release");
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ibkit"))
        .arg("gateway")
        .arg("--config")
        .arg(config_path)
        .arg("run")
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
        .env("IBKIT_TEST_DISPLAY_STARTED", &marker)
        .env("IBKIT_TEST_DISPLAY_RELEASE", &release)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !marker.exists() {
            assert!(child.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        Command::new("/bin/kill")
            .args(["-TERM", &child.id().unwrap().to_string()])
            .status()
            .unwrap()
            .success()
    );
    fs::write(release, []).unwrap();
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        ibkit_gateway::read_status(&config.state_dir).unwrap().state,
        State::Exited
    );
    assert!(output.stdout.split(|byte| *byte == b'\n').any(|line| {
        serde_json::from_slice::<ibkit_gateway::Event>(line)
            .is_ok_and(|event| matches!(event.kind, ibkit_gateway::EventKind::Exited { .. }))
    }));
}

#[tokio::test]
async fn readiness_requires_a_valid_framed_ib_api_greeting() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut magic = [0; 4];
        socket.read_exact(&mut magic).await.unwrap();
        assert_eq!(&magic, b"API\0");
        let size = socket.read_u32().await.unwrap();
        let mut version = vec![0; size as usize];
        socket.read_exact(&mut version).await.unwrap();
        assert_eq!(&version, b"v100..178");
        let greeting = b"178\x0020261108 12:00:00 EST\0";
        socket.write_u32(greeting.len() as u32).await.unwrap();
        socket.write_all(greeting).await.unwrap();
    });
    assert_eq!(api_handshake(port).await.unwrap(), 178);
    server.await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        socket.write_all(b"HTTP/1.0 200 OK\r\n").await.unwrap();
    });
    assert!(api_handshake(port).await.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn controller_lock_and_bounded_stop_leave_other_processes_running() {
    let dir = tempfile::Builder::new()
        .prefix("ibkit paths with spaces ")
        .tempdir()
        .unwrap();
    let launcher = dir.path().join("launcher");
    fs::write(&launcher, "#!/bin/sh\nexec sleep 60\n").unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700)).unwrap();
    let user = dir.path().join("user");
    let password = dir.path().join("password");
    fs::write(&user, "fixture-user").unwrap();
    fs::write(&password, "fixture-secret").unwrap();
    let config = Config {
        executable: launcher.clone(),
        agent: launcher,
        state_dir: dir.path().join("state"),
        settings_dir: dir.path().join("settings"),
        username_file: Some(user),
        password_file: Some(password),
        shutdown_timeout_secs: 1,
        ..Default::default()
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    assert!(
        Gateway::new(config).is_err(),
        "Two controllers must not share one state path"
    );
    let mut unrelated = Command::new("sleep").arg("60").spawn().unwrap();
    gateway.launch().await.unwrap();
    assert!(gateway.is_running());
    let started = Instant::now();
    gateway.stop().await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(gateway.status().state, State::Exited);
    assert_eq!(gateway.poll().await.unwrap(), State::Exited);
    assert!(
        unrelated.try_wait().unwrap().is_none(),
        "Stop killed an unrelated process"
    );
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
}

#[tokio::test]
async fn failed_sessions_stay_failed_and_invalid_settings_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = dir.path().join("launcher");
    fs::write(&launcher, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700)).unwrap();
    let config = Config {
        executable: launcher.clone(),
        agent: launcher,
        auto_login: false,
        state_dir: dir.path().join("state"),
        settings_dir: dir.path().join("settings"),
        ..Default::default()
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.launch().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while gateway.poll().await.unwrap() != State::Failed {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // A late heartbeat must not make a failed session active again.
    let status = ibkit_gateway::AgentStatus {
        session: gateway.status().session.clone(),
        pid: 12345,
        at: chrono::Utc::now(),
        state: State::Ready,
        result: ibkit_gateway::StartResult::default(),
        max_heap_mb: None,
    };
    fs::write(
        config.state_dir.join("agents/12345.json"),
        serde_json::to_vec(&status).unwrap(),
    )
    .unwrap();
    assert_eq!(gateway.poll().await.unwrap(), State::Failed);
    fs::remove_file(config.state_dir.join("agents/12345.json")).unwrap();
    gateway.stop().await.unwrap();
    drop(gateway);
    let ini = config.settings_dir.join("jts.ini");
    fs::write(&ini, [0xff, 0xfe]).unwrap();
    assert!(Gateway::new(config).is_err());
    assert_eq!(fs::read(ini).unwrap(), [0xff, 0xfe]);
}

#[tokio::test]
async fn restart_handoff_ignores_old_heartbeats_and_reads_partial_utf8_events() {
    let dir = tempfile::tempdir().unwrap();
    let config = session_config(dir.path());
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut events = gateway.events().subscribe();
    gateway.launch().await.unwrap();
    let session = gateway.status().session.clone();
    let mut first = ibkit_gateway::AgentStatus {
        session: session.clone(),
        pid: 12341,
        at: chrono::Utc::now(),
        state: State::Ready,
        result: ibkit_gateway::StartResult::default(),
        max_heap_mb: None,
    };
    let first_path = config.state_dir.join("agents/12341.json");
    fs::write(&first_path, serde_json::to_vec(&first).unwrap()).unwrap();
    assert_eq!(gateway.poll().await.unwrap(), State::Ready);
    let mut second = ibkit_gateway::AgentStatus {
        pid: 12342,
        state: State::Configuring,
        at: chrono::Utc::now(),
        ..first.clone()
    };
    let second_path = config.state_dir.join("agents/12342.json");
    fs::write(&second_path, serde_json::to_vec(&second).unwrap()).unwrap();
    assert_eq!(gateway.poll().await.unwrap(), State::Configuring);
    first.at = chrono::Utc::now();
    fs::write(&first_path, serde_json::to_vec(&first).unwrap()).unwrap();
    assert_eq!(gateway.poll().await.unwrap(), State::Configuring);
    second.state = State::Ready;
    second.at = chrono::Utc::now();
    fs::write(&second_path, serde_json::to_vec(&second).unwrap()).unwrap();
    assert_eq!(gateway.poll().await.unwrap(), State::Ready);
    first.at = chrono::Utc::now();
    fs::write(&first_path, serde_json::to_vec(&first).unwrap()).unwrap();
    assert_eq!(gateway.poll().await.unwrap(), State::Ready);
    let record = ibkit_gateway::Event {
        at: chrono::Utc::now(),
        session,
        pid: 12342,
        kind: ibkit_gateway::EventKind::Output {
            message: "Gateway says café".into(),
        },
    };
    let bytes = serde_json::to_vec(&record).unwrap();
    let split = bytes.iter().position(|byte| *byte == 0xc3).unwrap() + 1;
    let event_path = second_path.with_extension("events");
    fs::write(&event_path, &bytes[..split]).unwrap();
    gateway.poll().await.unwrap();
    let mut complete = bytes;
    complete.push(b'\n');
    fs::write(&event_path, complete).unwrap();
    gateway.poll().await.unwrap();
    gateway.poll().await.unwrap();
    let mut restarts = 0;
    let mut messages = 0;
    while let Ok(event) = events.try_recv() {
        match event.kind {
            ibkit_gateway::EventKind::Restarted => restarts += 1,
            ibkit_gateway::EventKind::Output { message } if message == "Gateway says café" => {
                messages += 1
            }
            _ => {}
        }
    }
    assert_eq!(restarts, 1);
    assert_eq!(messages, 1);
    fs::remove_file(first_path).unwrap();
    fs::remove_file(second_path).unwrap();
    gateway.stop().await.unwrap();
    assert_eq!(gateway.poll().await.unwrap(), State::Exited);
}

#[tokio::test]
async fn stop_during_start_cannot_return_success() {
    let dir = tempfile::tempdir().unwrap();
    let config = session_config(dir.path());
    let state_dir = config.state_dir.clone();
    let mut gateway = Gateway::new(config).unwrap();
    let controller = tokio::spawn(async move {
        let result = gateway.start().await.unwrap();
        (gateway, result)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if ibkit_gateway::read_status(&state_dir).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    ibkit_gateway::send_control(&state_dir, ibkit_gateway::Control::Stop).unwrap();
    let (mut gateway, result) = controller.await.unwrap();
    assert!(result.has_error());
    assert!(matches!(
        gateway.status().state,
        State::Exited | State::Failed
    ));
    gateway.stop().await.unwrap();
}

#[tokio::test]
async fn recovery_wait_can_be_cancelled_without_starting_a_new_session() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        auto_recover: true,
        ..session_config(dir.path())
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.launch().await.unwrap();
    let status = ibkit_gateway::AgentStatus {
        session: gateway.status().session.clone(),
        pid: 12343,
        at: chrono::Utc::now(),
        state: State::Exited,
        result: ibkit_gateway::StartResult::new(ibkit_gateway::ErrorCode::AuthenticationRequired),
        max_heap_mb: None,
    };
    let path = config.state_dir.join("agents/12343.json");
    fs::write(&path, serde_json::to_vec(&status).unwrap()).unwrap();
    assert_eq!(gateway.poll().await.unwrap(), State::Exited);
    fs::remove_file(path).unwrap();
    let mut recovery = ibkit_gateway::recovery::Recovery::default();
    assert!(gateway.recover(&mut recovery).await.unwrap());
    assert!(
        config.state_dir.join("recovery.json").is_file(),
        "A scheduled retry must be saved before another controller can start"
    );
    let saved = recovery.state;
    drop(gateway);
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut recovery = ibkit_gateway::recovery::Recovery::default();
    assert_eq!(
        gateway.resume(&mut recovery).await.unwrap(),
        State::Recovering
    );
    assert_eq!(
        recovery.state, saved,
        "A new controller changed the pending retry"
    );
    let session = gateway.status().session.clone();
    assert_eq!(gateway.status().state, State::Recovering);
    let pending = recovery.state;
    assert!(
        !gateway.recover(&mut recovery).await.unwrap(),
        "A pending recovery must not schedule another attempt"
    );
    assert_eq!(gateway.status().state, State::Recovering);
    assert_eq!(recovery.state, pending);
    ibkit_gateway::send_control(&config.state_dir, ibkit_gateway::Control::Stop).unwrap();
    assert_eq!(
        gateway.poll_recovery(&mut recovery).await.unwrap(),
        State::Exited
    );
    assert_eq!(gateway.status().session, session);
    assert!(
        !gateway.recover(&mut recovery).await.unwrap(),
        "An explicit stop must not schedule recovery again"
    );
}

#[tokio::test]
async fn interrupted_sessions_keep_the_budget_and_corrupt_journals_fail_closed() {
    use ibkit_gateway::recovery::{Recovery, RecoveryState};
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        auto_recover: true,
        recovery_attempts: 1,
        ..session_config(dir.path())
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut recovery = Recovery::default();
    assert_eq!(
        gateway.resume(&mut recovery).await.unwrap(),
        State::Starting
    );
    // Leave Running on disk, as a controller crash would do.
    gateway.stop().await.unwrap();
    drop(gateway);
    let mut gateway = Gateway::new(config.clone()).unwrap();
    assert_eq!(
        gateway.resume(&mut recovery).await.unwrap(),
        State::Recovering
    );
    assert!(matches!(
        recovery.state,
        RecoveryState::Waiting {
            reason: ibkit_gateway::ErrorCode::AgentDisconnected,
            ..
        }
    ));
    // Model a crash after the counted retry was saved but before exec.
    recovery.retry_started();
    let path = config.state_dir.join("recovery.json");
    fs::write(&path, serde_json::to_vec(&recovery).unwrap()).unwrap();
    drop(gateway);
    let mut gateway = Gateway::new(config.clone()).unwrap();
    assert_eq!(
        gateway.resume(&mut recovery).await.unwrap(),
        State::Halted,
        "An exhausted budget halts the controller instead of exiting"
    );
    assert_eq!(recovery.state, RecoveryState::Exhausted);
    assert_eq!(gateway.status().recovery, Some(RecoveryState::Exhausted));
    assert!(!gateway.is_running());
    // The halted controller resumes on its own once the hour has passed.
    let mut saved = serde_json::to_value(&recovery).unwrap();
    saved["last_failure"] = serde_json::json!(chrono::Utc::now() - chrono::Duration::hours(2));
    let mut freed: ibkit_gateway::recovery::Recovery = serde_json::from_value(saved).unwrap();
    assert_eq!(
        gateway.poll_halted(&mut freed).await.unwrap(),
        State::Recovering,
        "A freed budget schedules recovery from the halted state"
    );
    assert!(matches!(freed.state, RecoveryState::Waiting { .. }));
    gateway.stop().await.unwrap();
    drop(gateway);
    fs::write(&path, b"{broken").unwrap();
    let mut gateway = Gateway::new(config.clone()).unwrap();
    assert!(gateway.resume(&mut recovery).await.is_err());
    assert_eq!(fs::read(&path).unwrap(), b"{broken");
    assert!(
        Gateway::new(config).is_err(),
        "The reset must use the controller lock"
    );
    gateway.reset_recovery().unwrap();
    assert!(!path.exists());
    assert_eq!(
        gateway.resume(&mut recovery).await.unwrap(),
        State::Starting
    );
    assert!(gateway.reset_recovery().is_err());
    gateway.stop().await.unwrap();
    gateway.cancel_recovery(&mut recovery).unwrap();
}

#[tokio::test]
async fn interrupted_controller_preserves_a_known_credential_failure() {
    use ibkit_gateway::recovery::{Recovery, RecoveryState};
    for saved_by_controller in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            auto_recover: true,
            ..session_config(dir.path())
        };
        let mut gateway = Gateway::new(config.clone()).unwrap();
        let mut recovery = Recovery::default();
        gateway.resume(&mut recovery).await.unwrap();
        let status = ibkit_gateway::AgentStatus {
            session: gateway.status().session.clone(),
            pid: 12346,
            at: chrono::Utc::now()
                - if saved_by_controller {
                    chrono::Duration::zero()
                } else {
                    chrono::Duration::minutes(1)
                },
            state: State::Failed,
            result: ibkit_gateway::StartResult::new(ibkit_gateway::ErrorCode::LoginFailed),
            max_heap_mb: None,
        };
        let path = config.state_dir.join("agents/12346.json");
        fs::write(&path, serde_json::to_vec(&status).unwrap()).unwrap();
        if saved_by_controller {
            assert_eq!(gateway.poll().await.unwrap(), State::Failed);
            fs::remove_file(path).unwrap();
        }
        // The policy file is still Running if the controller dies before recover.
        gateway.stop().await.unwrap();
        drop(gateway);
        let mut gateway = Gateway::new(config).unwrap();
        assert_eq!(gateway.resume(&mut recovery).await.unwrap(), State::Halted);
        assert_eq!(
            recovery.state,
            RecoveryState::Blocked {
                reason: ibkit_gateway::ErrorCode::LoginFailed
            }
        );
    }
}

/// Wait until the controller `pid` reports Halted after `launches` launches.
async fn wait_for_halt(config: &Config, marker: &std::path::Path, pid: u32, launches: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let count = fs::read_to_string(marker)
                .map(|text| text.lines().count())
                .unwrap_or(0);
            if let Ok(status) = ibkit_gateway::read_status(&config.state_dir)
                && status.controller_pid == pid
                && status.state == State::Halted
                && matches!(
                    status.recovery,
                    Some(ibkit_gateway::recovery::RecoveryState::Blocked { .. })
                )
                && count == launches
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "the controller did not halt after {launches} launches: {:?}, launches {:?}",
            ibkit_gateway::read_status(&config.state_dir).map(|status| (
                status.state,
                status.recovery,
                status.controller_pid == pid
            )),
            fs::read_to_string(marker).map(|text| text.lines().count())
        )
    });
}

#[tokio::test]
async fn halted_controller_stays_up_and_logs_in_again_only_on_restart() {
    use ibkit_gateway::recovery::RecoveryState;
    let dir = tempfile::tempdir().unwrap();
    let config = session_config(dir.path());
    let marker = dir.path().join("launches");
    fs::write(
        &config.executable,
        format!(
            "#!/bin/sh\nprintf 'started\\n' >> '{}'\nexit 1\n",
            marker.display()
        ),
    )
    .unwrap();
    let config_path = dir.path().join("config.toml");
    fs::write(&config_path, format!("executable = {:?}\nagent = {:?}\nstate_dir = {:?}\nsettings_dir = {:?}\nauto_login = false\nauto_recover = false\nshutdown_timeout_secs = 1\n", config.executable, config.agent, config.state_dir, config.settings_dir)).unwrap();
    let command = |name: &str| {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_ibkit"));
        command
            .arg("gateway")
            .arg("--config")
            .arg(&config_path)
            .arg(name);
        command
    };
    for run in 0..2 {
        let mut child = command("run")
            .stdout(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        // The second run must not log in again: the block is saved.
        wait_for_halt(&config, &marker, pid, 1).await;
        assert!(
            child.try_wait().unwrap().is_none(),
            "A halted controller must stay up"
        );
        assert_eq!(
            ibkit_gateway::read_status(&config.state_dir)
                .unwrap()
                .recovery,
            Some(RecoveryState::Blocked {
                reason: ibkit_gateway::ErrorCode::ProcessStartFailed
            })
        );
        assert!(
            command("health")
                .arg("--kind")
                .arg("live")
                .status()
                .await
                .unwrap()
                .success()
        );
        assert!(!command("health").status().await.unwrap().success());
        if run == 1 {
            // An operator restart is the only way to log in again.
            assert!(command("restart").status().await.unwrap().success());
            wait_for_halt(&config, &marker, pid, 2).await;
        }
        assert!(command("stop").status().await.unwrap().success());
        let exit = tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(
            exit.success(),
            "A stop command ends a halted controller normally"
        );
    }
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 2);
}

#[tokio::test]
async fn replacement_controller_adopts_a_live_gateway() {
    let dir = tempfile::tempdir().unwrap();
    let config = session_config(dir.path());
    let marker = dir.path().join("launches");
    fs::write(
        &config.executable,
        format!(
            "#!/bin/sh\nprintf 'started\\n' >> '{}'\nexec sleep 60\n",
            marker.display()
        ),
    )
    .unwrap();
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut recovery = ibkit_gateway::recovery::Recovery::default();
    assert_eq!(
        gateway.resume(&mut recovery).await.unwrap(),
        State::Starting
    );
    let session = gateway.status().session.clone();
    // A real process stands in for the Gateway JVM, so cleanup signals only it.
    let mut jvm = tokio::process::Command::new("sleep")
        .arg("60")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let agent = ibkit_gateway::AgentStatus {
        session: session.clone(),
        pid: jvm.id().unwrap(),
        at: chrono::Utc::now(),
        state: State::Ready,
        result: ibkit_gateway::StartResult::default(),
        max_heap_mb: Some(1000),
    };
    let path = config.state_dir.join(format!("agents/{}.json", agent.pid));
    fs::write(&path, serde_json::to_vec(&agent).unwrap()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // The controller dies without stopping its Gateway. Drop releases the
    // lock as process exit would; the stand-in JVM ignores its stop command.
    drop(gateway);
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut events = gateway.events().subscribe();
    let mut recovery = ibkit_gateway::recovery::Recovery::default();
    assert_eq!(gateway.resume(&mut recovery).await.unwrap(), State::Ready);
    assert_eq!(gateway.status().session, session);
    assert_eq!(gateway.poll().await.unwrap(), State::Ready);
    assert_eq!(
        fs::read_to_string(&marker).unwrap().lines().count(),
        1,
        "Adoption must not start a second login"
    );
    let ibkit_gateway::EventKind::Output { message } = events.try_recv().unwrap().kind else {
        panic!("expected the adoption notice");
    };
    assert!(message.contains("Adopted"), "{message}");
    gateway.stop().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), jvm.wait())
            .await
            .is_ok(),
        "Stop must close the adopted Gateway"
    );
}

#[tokio::test]
async fn reload_applies_a_new_configuration_in_a_new_session() {
    let dir = tempfile::tempdir().unwrap();
    let config = session_config(dir.path());
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut recovery = ibkit_gateway::recovery::Recovery::default();
    gateway.resume(&mut recovery).await.unwrap();
    let session = gateway.status().session.clone();
    ibkit_gateway::send_control(&config.state_dir, ibkit_gateway::Control::Reload).unwrap();
    gateway.poll().await.unwrap();
    assert!(gateway.take_reload());
    assert!(!gateway.take_reload(), "a reload request is taken once");
    let moved = Config {
        state_dir: dir.path().join("other"),
        ..config.clone()
    };
    let error = gateway
        .reconfigure(moved, &mut recovery)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("state_dir"), "{error}");
    assert_eq!(
        gateway.status().session,
        session,
        "a rejected reload keeps the session"
    );
    // Two more settings changes while the first reload restarts Gateway,
    // then a stop: the reloads count as one, and the stop survives the new
    // session that the reload starts.
    ibkit_gateway::send_control(&config.state_dir, ibkit_gateway::Control::Reload).unwrap();
    ibkit_gateway::send_control(&config.state_dir, ibkit_gateway::Control::Reload).unwrap();
    ibkit_gateway::send_control(&config.state_dir, ibkit_gateway::Control::Stop).unwrap();
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
    assert_eq!(gateway.config().api_port, 4100);
    assert_ne!(gateway.status().session, session);
    gateway.poll().await.unwrap();
    assert!(
        gateway.take_reload(),
        "the queued reload survives the new session"
    );
    assert!(gateway.is_running(), "the stop waits for the next step");
    gateway.poll().await.unwrap();
    assert!(!gateway.take_reload(), "reloads in a row are taken once");
    assert!(!gateway.is_running());
    // A command left in the queue does not reach the next controller, even
    // with the same PID (a container restart).
    gateway.launch().await.unwrap();
    ibkit_gateway::send_control(&config.state_dir, ibkit_gateway::Control::Stop).unwrap();
    gateway.stop().await.unwrap();
    drop(gateway);
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.launch().await.unwrap();
    gateway.poll().await.unwrap();
    assert!(
        gateway.is_running(),
        "a stale stop must not stop the new controller"
    );
    gateway.stop().await.unwrap();
}

#[tokio::test]
async fn launcher_cannot_start_if_session_ownership_cannot_be_saved() {
    let dir = tempfile::tempdir().unwrap();
    let config = session_config(dir.path());
    let marker = dir.path().join("launched");
    fs::write(
        &config.executable,
        format!(
            "#!/bin/sh\nprintf started > '{}'\nexec sleep 60\n",
            marker.display()
        ),
    )
    .unwrap();
    let mut gateway = Gateway::new(config.clone()).unwrap();
    fs::create_dir(config.state_dir.join("status.json")).unwrap();
    assert!(gateway.launch().await.is_err());
    let launched = tokio::time::timeout(Duration::from_secs(1), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        launched.is_err(),
        "A JVM must not start before its session can be saved"
    );
}

#[tokio::test]
async fn managed_start_waits_for_a_late_agent_and_closes_it_before_retry() {
    use ibkit_gateway::recovery::{Recovery, RecoveryState};
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        auto_recover: true,
        ..surviving_jvm_config(dir.path())
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut recovery = Recovery::default();
    gateway.resume(&mut recovery).await.unwrap();
    let original = gateway.status().session.clone();
    eventually(|| dir.path().join("forked").exists()).await;
    drop(gateway);
    let mut gateway = Gateway::new(config.clone()).unwrap();
    assert_eq!(
        gateway.resume(&mut recovery).await.unwrap(),
        State::Recovering
    );
    assert_eq!(gateway.status().session, original);
    assert!(matches!(
        recovery.state,
        RecoveryState::AwaitingAgent { .. }
    ));
    assert_eq!(
        gateway.poll_recovery(&mut recovery).await.unwrap(),
        State::Recovering
    );
    let mut late = Command::new("sleep").arg("60").spawn().unwrap();
    let path = config.state_dir.join(format!("agents/{}.json", late.id()));
    fs::write(
        &path,
        serde_json::to_vec(&ibkit_gateway::AgentStatus {
            session: original.clone(),
            pid: late.id(),
            at: chrono::Utc::now(),
            state: State::Starting,
            result: Default::default(),
            max_heap_mb: None,
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        gateway.poll_recovery(&mut recovery).await.unwrap(),
        State::Recovering
    );
    assert!(
        late.try_wait().unwrap().is_some(),
        "The late agent was not stopped"
    );
    fs::remove_file(path).unwrap();
    assert!(matches!(recovery.state, RecoveryState::Waiting { .. }));
    let launch: ibkit_gateway::AgentLaunch =
        serde_json::from_slice(&fs::read(config.state_dir.join("launch.json")).unwrap()).unwrap();
    assert_eq!(
        launch.session, original,
        "A second session started before recovery was due"
    );
    gateway.stop().await.unwrap();
    gateway.cancel_recovery(&mut recovery).unwrap();
    kill_saved_group(&config);
}

#[tokio::test]
async fn an_unreported_interrupted_jvm_blocks_instead_of_starting_a_duplicate() {
    use ibkit_gateway::recovery::{Recovery, RecoveryState};
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        auto_recover: true,
        initialization_timeout_secs: 1,
        ..surviving_jvm_config(dir.path())
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut recovery = Recovery::default();
    gateway.resume(&mut recovery).await.unwrap();
    let group = gateway.status().process_group;
    eventually(|| dir.path().join("forked").exists()).await;
    drop(gateway);
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.resume(&mut recovery).await.unwrap();
    let saved = recovery.state;
    drop(gateway);
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.resume(&mut recovery).await.unwrap();
    assert_eq!(
        recovery.state, saved,
        "A replacement controller reset the no-report deadline"
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        gateway.poll_recovery(&mut recovery).await.unwrap(),
        State::Failed
    );
    assert_eq!(
        recovery.state,
        RecoveryState::Blocked {
            reason: ibkit_gateway::ErrorCode::AgentDisconnected
        }
    );
    if let Some(group) = group {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{group}")])
            .status();
    }
}

#[tokio::test]
async fn a_launcher_group_that_is_gone_is_retried_without_the_agent_wait() {
    use ibkit_gateway::recovery::{Recovery, RecoveryState};
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        auto_recover: true,
        ..session_config(dir.path())
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut recovery = Recovery::default();
    gateway.resume(&mut recovery).await.unwrap();
    // Dropping the controller kills the launcher, its whole group here: a
    // new container after a kill looks the same.
    let group = gateway.status().process_group.unwrap();
    drop(gateway);
    // The killed launcher stays a zombie of this test process until tokio
    // reaps it, which a new tokio child triggers; in a new container nothing
    // of the group is left.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let _ = tokio::process::Command::new("true").status().await;
            let members = std::process::Command::new("ps")
                .args(["-o", "pid=", "-g", &group.to_string()])
                .output()
                .unwrap()
                .stdout;
            if String::from_utf8_lossy(&members).trim().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the killed launcher was not reaped");
    let mut gateway = Gateway::new(config.clone()).unwrap();
    assert_eq!(
        gateway.resume(&mut recovery).await.unwrap(),
        State::Recovering
    );
    assert!(
        matches!(recovery.state, RecoveryState::Waiting { .. }),
        "no 15-minute agent wait when nothing of the old launcher runs: {:?}",
        recovery.state
    );
    gateway.cancel_recovery(&mut recovery).unwrap();
}

#[tokio::test]
async fn restart_during_recovery_cannot_ignore_a_terminal_agent_report() {
    use ibkit_gateway::recovery::{Recovery, RecoveryState};
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        auto_recover: true,
        ..session_config(dir.path())
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.launch().await.unwrap();
    let session = gateway.status().session.clone();
    let mut status = ibkit_gateway::AgentStatus {
        session: session.clone(),
        pid: 12347,
        at: chrono::Utc::now(),
        state: State::Exited,
        result: ibkit_gateway::StartResult::new(ibkit_gateway::ErrorCode::AuthenticationRequired),
        max_heap_mb: None,
    };
    let path = config.state_dir.join("agents/12347.json");
    fs::write(&path, serde_json::to_vec(&status).unwrap()).unwrap();
    gateway.poll().await.unwrap();
    fs::remove_file(&path).unwrap();
    let mut recovery = Recovery::default();
    assert!(gateway.recover(&mut recovery).await.unwrap());
    status.state = State::Failed;
    status.at = chrono::Utc::now() - chrono::Duration::minutes(1);
    status.result = ibkit_gateway::StartResult::new(ibkit_gateway::ErrorCode::LoginFailed);
    fs::write(&path, serde_json::to_vec(&status).unwrap()).unwrap();
    ibkit_gateway::send_control(&config.state_dir, ibkit_gateway::Control::Restart).unwrap();
    assert_eq!(
        gateway.poll_recovery(&mut recovery).await.unwrap(),
        State::Failed
    );
    assert_eq!(
        recovery.state,
        RecoveryState::Blocked {
            reason: ibkit_gateway::ErrorCode::LoginFailed
        }
    );
    let launch: ibkit_gateway::AgentLaunch =
        serde_json::from_slice(&fs::read(config.state_dir.join("launch.json")).unwrap()).unwrap();
    assert_eq!(launch.session, session);
}

#[tokio::test]
async fn stopped_controller_cleanup_closes_only_its_fresh_session_agents() {
    let dir = tempfile::tempdir().unwrap();
    let config = session_config(dir.path());
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.launch().await.unwrap();
    let session = gateway.status().session.clone();
    let mut agent = Command::new("sleep").arg("60").spawn().unwrap();
    let mut unrelated = Command::new("sleep").arg("60").spawn().unwrap();
    fs::write(
        config.state_dir.join(format!("agents/{}.json", agent.id())),
        serde_json::to_vec(&ibkit_gateway::AgentStatus {
            session,
            pid: agent.id(),
            at: chrono::Utc::now(),
            state: State::Ready,
            result: Default::default(),
            max_heap_mb: None,
        })
        .unwrap(),
    )
    .unwrap();
    drop(gateway);
    let mut gateway = Gateway::new(config.clone()).unwrap();
    gateway.stop_existing().await.unwrap();
    assert!(agent.try_wait().unwrap().is_some());
    assert!(unrelated.try_wait().unwrap().is_none());
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
    assert_eq!(gateway.status().state, State::Exited);
    assert_eq!(
        ibkit_gateway::read_status(&config.state_dir).unwrap().state,
        State::Exited
    );
}

#[tokio::test]
async fn tui_requires_a_terminal_without_creating_a_config_or_starting_gateway() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ibkit"))
        .arg("gateway")
        .arg("--config")
        .arg(&config)
        .arg("tui")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal"));
    assert!(!config.exists());
}

#[test]
fn settings_updates_preserve_unrelated_sections_and_are_repeatable() {
    let dir = tempfile::tempdir().unwrap();
    let config = session_config(dir.path());
    fs::create_dir_all(&config.settings_dir).unwrap();
    let path = config.settings_dir.join("jts.ini");
    fs::write(
        &path,
        "[Other]\nSaved=yes\n[Logon]\nPeer=zdc1.ibllc.com:4000\nUseSSL=false\n",
    )
    .unwrap();
    let gateway = Gateway::new(config.clone()).unwrap();
    drop(gateway);
    let once = fs::read_to_string(&path).unwrap();
    assert!(
        once.contains("Saved=yes")
            && once.contains("Peer=zdc1.ibllc.com:4000")
            && once.contains("UseSSL=true")
    );
    let gateway = Gateway::new(config).unwrap();
    drop(gateway);
    assert_eq!(fs::read_to_string(path).unwrap(), once);
}

#[tokio::test]
async fn api_probe_failure_has_a_grace_period_and_reports_restoration() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let config = Config {
        api_port: port,
        api_probe_interval_secs: 1,
        api_failure_timeout_secs: 180,
        ..session_config(dir.path())
    };
    let mut gateway = Gateway::new(config.clone()).unwrap();
    let mut events = gateway.events().subscribe();
    gateway.launch().await.unwrap();
    let mut status = ibkit_gateway::AgentStatus {
        session: gateway.status().session.clone(),
        pid: 12344,
        at: chrono::Utc::now(),
        state: State::Ready,
        result: ibkit_gateway::StartResult::default(),
        max_heap_mb: None,
    };
    let path = config.state_dir.join("agents/12344.json");
    fs::write(&path, serde_json::to_vec(&status).unwrap()).unwrap();
    gateway.poll().await.unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(gateway.poll().await.unwrap(), State::Ready);
    let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 17];
        socket.read_exact(&mut request).await.unwrap();
        let greeting = b"178\x0020261108 12:00:00 EST\0";
        socket.write_u32(greeting.len() as u32).await.unwrap();
        socket.write_all(greeting).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(1100)).await;
    status.at = chrono::Utc::now();
    fs::write(&path, serde_json::to_vec(&status).unwrap()).unwrap();
    assert_eq!(gateway.poll().await.unwrap(), State::Ready);
    server.await.unwrap();
    let mut unavailable = 0;
    let mut restored = 0;
    while let Ok(event) = events.try_recv() {
        match event.kind {
            ibkit_gateway::EventKind::ApiUnavailable => unavailable += 1,
            ibkit_gateway::EventKind::ApiRestored => restored += 1,
            _ => {}
        }
    }
    assert_eq!((unavailable, restored), (1, 1));
    fs::remove_file(path).unwrap();
    gateway.stop().await.unwrap();
}
