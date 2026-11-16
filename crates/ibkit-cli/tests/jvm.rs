use ibkit_gateway_core::{AgentLaunch, AgentStatus, Command, Config, Control, State};
use std::{
    fs,
    path::PathBuf,
    process::{Command as Process, Stdio},
    thread,
    time::{Duration, Instant},
};

#[test]
#[ignore = "Requires JAVA_HOME, IBKIT_AGENT_LIBRARY, and a desktop display; run scripts/jvm-smoke.sh"]
fn native_agent_controls_real_swing_on_the_event_thread() {
    let Some(java_home) = std::env::var_os("JAVA_HOME") else {
        eprintln!(
            "JVM check requires JAVA_HOME and IBKIT_AGENT_LIBRARY. Run scripts/jvm-smoke.sh."
        );
        panic!("Set JAVA_HOME to a JDK and run scripts/jvm-smoke.sh.");
    };
    let Some(library) = std::env::var_os("IBKIT_AGENT_LIBRARY") else {
        eprintln!("JVM check requires IBKIT_AGENT_LIBRARY. Run scripts/jvm-smoke.sh.");
        panic!("Build the native agent and set IBKIT_AGENT_LIBRARY to its library path.");
    };
    let dir = tempfile::tempdir().unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/GatewayFixture.java");
    let compiler = Process::new(PathBuf::from(&java_home).join("bin/javac"))
        .args(["-d"])
        .arg(dir.path())
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        compiler.status.success(),
        "javac failed: {}",
        String::from_utf8_lossy(&compiler.stderr)
    );
    let config = Config {
        state_dir: dir.path().into(),
        api_port: 14567,
        incoming_connection: ibkit_gateway_core::IncomingConnection::Accept,
        existing_session: ibkit_gateway_core::ExistingSession::Takeover,
        ..Default::default()
    };
    let launch = AgentLaunch {
        session: "fixture-session".into(),
        config,
    };
    let launch_path = dir.path().join("launch.json");
    fs::write(&launch_path, serde_json::to_vec(&launch).unwrap()).unwrap();
    let output = fs::File::create(dir.path().join("output")).unwrap();
    let mut child = Process::new(PathBuf::from(java_home).join("bin/java"))
        .arg("-Xcheck:jni")
        .arg(format!(
            "-agentpath:{}=env",
            PathBuf::from(library).display()
        ))
        .env("IBKIT_AGENT_CONFIG", &launch_path)
        .arg("-cp")
        .arg(dir.path())
        .arg("GatewayFixture")
        .env("IB_USERNAME", "fixture-user")
        .env("IB_PASSWORD", "fixture-secret")
        .env("FIXTURE_PORT", "14567")
        .stdout(Stdio::from(output.try_clone().unwrap()))
        .stderr(Stdio::from(output))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut exported = false;
    let mut reconnected = false;
    let mut restarted = false;
    let mut stopped = false;
    loop {
        if let Some(exit) = child.try_wait().unwrap() {
            let output = fs::read_to_string(dir.path().join("output")).unwrap();
            assert!(exit.success(), "JVM exited with {exit}: {output}");
            assert!(
                output.contains("FIXTURE_COMPLETE"),
                "The fixture did not finish: {output}"
            );
            assert!(
                !output.contains("FATAL ERROR"),
                "JNI check failed: {output}"
            );
            // Linux OpenJDK 17 can report a JNI check warning in its own
            // FreetypeFontScaler. Keep checking every other native boundary.
            for warning in output.split("WARNING in native method:").skip(1) {
                let first_frame = warning.lines().nth(1).unwrap_or_default().trim();
                assert!(
                    first_frame.starts_with("at sun.font.FreetypeFontScaler."),
                    "JNI check failed: {}",
                    warning.lines().take(5).collect::<Vec<_>>().join("\n")
                );
            }
            break;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!(
                "JVM timed out: {}",
                fs::read_to_string(dir.path().join("output")).unwrap()
            );
        }
        if let Ok(entries) = fs::read_dir(dir.path().join("agents")) {
            for entry in entries.flatten() {
                if entry.path().extension().is_none_or(|ext| ext != "json") {
                    continue;
                }
                let status: AgentStatus =
                    serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap();
                if status.state == State::Failed {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!(
                        "Agent failed: {:?}. Events: {}",
                        status.result,
                        fs::read_to_string(entry.path().with_extension("events")).unwrap()
                    );
                }
                let events =
                    fs::read_to_string(entry.path().with_extension("events")).unwrap_or_default();
                let control = if status.state == State::Ready && !exported {
                    exported = true;
                    Some(Control::ExportLogs)
                } else if exported
                    && events.matches("\"kind\":\"export_finished\"").count() >= 2
                    && !reconnected
                {
                    reconnected = true;
                    Some(Control::ReconnectAccount)
                } else if reconnected && events.contains("Reconnect requested") && !restarted {
                    restarted = true;
                    Some(Control::SoftRestart)
                } else if restarted
                    && events.matches("Configuration settings updated.").count() >= 2
                    && !stopped
                {
                    stopped = true;
                    Some(Control::Stop)
                } else {
                    None
                };
                if let Some(control) = control {
                    fs::write(
                        dir.path().join(format!("command-{}.json", status.pid)),
                        serde_json::to_vec(&Command {
                            session: launch.session.clone(),
                            control,
                        })
                        .unwrap(),
                    )
                    .unwrap();
                }
                assert!(
                    !events.contains("fixture-secret") && !events.contains("fixture-user"),
                    "Credentials escaped into events"
                );
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(exported && reconnected && restarted && stopped);
}

#[test]
#[ignore = "Requires JAVA_HOME, IBKIT_AGENT_LIBRARY, and a desktop display; run scripts/jvm-smoke.sh"]
fn native_agent_bounds_approval_and_does_not_dump_unknown_editable_fields() {
    let (Some(java_home), Some(library)) = (
        std::env::var_os("JAVA_HOME"),
        std::env::var_os("IBKIT_AGENT_LIBRARY"),
    ) else {
        eprintln!(
            "JVM check requires JAVA_HOME and IBKIT_AGENT_LIBRARY. Run scripts/jvm-smoke.sh."
        );
        panic!("Set JAVA_HOME and IBKIT_AGENT_LIBRARY, then run scripts/jvm-smoke.sh.");
    };
    let classes = tempfile::tempdir().unwrap();
    let compiler = Process::new(PathBuf::from(&java_home).join("bin/javac"))
        .arg("-d")
        .arg(classes.path())
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/GatewayFixture.java"))
        .output()
        .unwrap();
    assert!(compiler.status.success());
    for (scenario, expected) in [
        (
            "timeout",
            ibkit_gateway_core::ErrorCode::TwoFactorConfirmationTimeout,
        ),
        (
            "unknown",
            ibkit_gateway_core::ErrorCode::UnknownMessageWindowDetected,
        ),
        (
            "reopen",
            ibkit_gateway_core::ErrorCode::TwoFactorConfirmationTimeout,
        ),
        (
            "account_tasks",
            ibkit_gateway_core::ErrorCode::LoginFailedAccountTasksRequired,
        ),
        (
            "security",
            ibkit_gateway_core::ErrorCode::SecurityDialogDetected,
        ),
        (
            "existing",
            ibkit_gateway_core::ErrorCode::ExistingSessionDetected,
        ),
        (
            "relogin_stall",
            ibkit_gateway_core::ErrorCode::InitializationTimeout,
        ),
        // The same Re-login prompt with the default refuse policy.
        (
            "relogin_refuse",
            ibkit_gateway_core::ErrorCode::ExistingSessionDetected,
        ),
        (
            "factor_stall",
            ibkit_gateway_core::ErrorCode::InitializationTimeout,
        ),
    ] {
        let dir = tempfile::Builder::new()
            .prefix("ibkit space ")
            .tempdir()
            .unwrap();
        let launch = AgentLaunch {
            session: scenario.into(),
            config: Config {
                state_dir: dir.path().into(),
                api_port: 14567,
                second_factor_timeout_secs: if scenario == "timeout" { 1 } else { 5 },
                // Re-login is a takeover; the stall needs it to proceed.
                existing_session: if scenario == "relogin_stall" {
                    ibkit_gateway_core::ExistingSession::Takeover
                } else {
                    ibkit_gateway_core::ExistingSession::Refuse
                },
                second_factor_attempts: 1,
                unknown_dialog_timeout_secs: 2,
                initialization_timeout_secs: if matches!(scenario, "relogin_stall" | "factor_stall")
                {
                    8
                } else {
                    30
                },
                ..Default::default()
            },
        };
        let launch_path = dir.path().join("launch.json");
        fs::write(&launch_path, serde_json::to_vec(&launch).unwrap()).unwrap();
        let mut child = Process::new(PathBuf::from(&java_home).join("bin/java"))
            .arg(format!(
                "-agentpath:{}={}",
                PathBuf::from(&library).display(),
                launch_path.display()
            ))
            .arg("-cp")
            .arg(classes.path())
            .arg("GatewayFixture")
            .env("IB_USERNAME", "fixture-user")
            .env("IB_PASSWORD", "fixture-secret")
            .env("FIXTURE_PORT", "14567")
            .env(
                "FIXTURE_SCENARIO",
                if scenario == "relogin_refuse" {
                    "relogin_stall"
                } else {
                    scenario
                },
            )
            .env("FIXTURE_REAUTH_REQUEST", dir.path().join("reauth-request"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(25);
        let status_path = dir
            .path()
            .join("agents")
            .join(format!("{}.json", child.id()));
        let mut reauth_requested = false;
        let failed = loop {
            if let Ok(bytes) = fs::read(&status_path) {
                let status: AgentStatus = serde_json::from_slice(&bytes).unwrap();
                if scenario == "factor_stall" && status.state == State::Ready && !reauth_requested {
                    fs::write(dir.path().join("reauth-request"), []).unwrap();
                    reauth_requested = true;
                }
                if status.state == State::Failed {
                    break Some(status);
                }
            }
            if Instant::now() > deadline {
                break None;
            }
            thread::sleep(Duration::from_millis(100));
        };
        let _ = child.kill();
        let _ = child.wait();
        if scenario == "factor_stall" {
            assert!(reauth_requested, "The initial login did not reach Ready");
        }
        let failed = failed.unwrap_or_else(|| {
            panic!("The {scenario} failure was not reported within the time limit. Events: {}. Status: {}", fs::read_to_string(status_path.with_extension("events")).unwrap_or_default(), fs::read_to_string(&status_path).unwrap_or_default())
        });
        assert_eq!(failed.result.error_code, expected);
        let events = fs::read_to_string(status_path.with_extension("events")).unwrap();
        assert!(!events.contains("fixture-secret") && !events.contains("fixture-user"));
    }
}

#[test]
#[ignore = "Requires JAVA_HOME, IBKIT_AGENT_LIBRARY, and a desktop display; run scripts/jvm-smoke.sh"]
fn native_agent_preserves_restart_authentication_and_emits_one_token_expiry() {
    let java_home = PathBuf::from(std::env::var_os("JAVA_HOME").expect("Set JAVA_HOME to a JDK."));
    let library = PathBuf::from(
        std::env::var_os("IBKIT_AGENT_LIBRARY")
            .expect("Set IBKIT_AGENT_LIBRARY to the built agent."),
    );
    let classes = tempfile::tempdir().unwrap();
    assert!(
        Process::new(java_home.join("bin/javac"))
            .arg("-d")
            .arg(classes.path())
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/GatewayFixture.java"))
            .status()
            .unwrap()
            .success()
    );
    for (scenario, expected) in [
        ("restore", State::Ready),
        ("expired", State::Exited),
        ("factor_reauth", State::Ready),
        ("login_reauth", State::Ready),
        ("progress", State::Ready),
        ("totp", State::Ready),
        ("port_change", State::Ready),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let launch = AgentLaunch {
            session: scenario.into(),
            config: Config {
                state_dir: dir.path().into(),
                api_port: 14567,
                ..Default::default()
            },
        };
        let path = dir.path().join("launch.json");
        fs::write(&path, serde_json::to_vec(&launch).unwrap()).unwrap();
        let mut command = Process::new(java_home.join("bin/java"));
        if scenario == "restore" {
            command.arg("-Drestart=fixture-token");
        }
        let mut child = command
            .arg(format!(
                "-agentpath:{}={}",
                library.display(),
                path.display()
            ))
            .arg("-cp")
            .arg(classes.path())
            .arg("GatewayFixture")
            .env("IB_USERNAME", "fixture-user")
            .env("IB_PASSWORD", "fixture-secret")
            .env("FIXTURE_PORT", "14567")
            .env("FIXTURE_SCENARIO", scenario)
            .env("FIXTURE_REAUTH_REQUEST", dir.path().join("reauth-request"))
            .env("FIXTURE_MARKER", dir.path().join("marker"))
            .env(
                "IB_TOTP_SECRET",
                if scenario == "totp" {
                    "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
                } else {
                    ""
                },
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let status_path = dir
            .path()
            .join("agents")
            .join(format!("{}.json", child.id()));
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut reauth_requested = false;
        let found = loop {
            if let Ok(bytes) = fs::read(&status_path) {
                let status: AgentStatus = serde_json::from_slice(&bytes).unwrap();
                let needs_reauth = matches!(scenario, "factor_reauth" | "login_reauth");
                if needs_reauth && status.state == State::Ready && !reauth_requested {
                    fs::write(dir.path().join("reauth-request"), []).unwrap();
                    reauth_requested = true;
                }
                let ready_count = fs::read_to_string(status_path.with_extension("events"))
                    .unwrap_or_default()
                    .lines()
                    .filter_map(|line| serde_json::from_str::<ibkit_gateway_core::Event>(line).ok())
                    .filter(|event| {
                        matches!(
                            event.kind,
                            ibkit_gateway_core::EventKind::StateChanged {
                                state: State::Ready
                            }
                        )
                    })
                    .count();
                let marked = scenario != "port_change" || dir.path().join("marker").exists();
                if status.state == expected && marked && (!needs_reauth || ready_count >= 2) {
                    break Some(status);
                }
                if status.state == State::Failed {
                    break None;
                }
            }
            if Instant::now() > deadline {
                break None;
            }
            thread::sleep(Duration::from_millis(100));
        };
        // Keep the expired-token dialog present across several native callbacks.
        thread::sleep(Duration::from_millis(1200));
        let _ = child.kill();
        let _ = child.wait();
        let events = fs::read_to_string(status_path.with_extension("events")).unwrap_or_default();
        let status = found.unwrap_or_else(|| {
            panic!("The {scenario} scenario did not reach {expected:?}: {events}")
        });
        if scenario == "restore" {
            assert!(!events.contains("\"state\":\"logging_in\""));
        } else if scenario == "expired" {
            assert_eq!(
                status.result.error_code,
                ibkit_gateway_core::ErrorCode::AuthenticationRequired
            );
            assert_eq!(events.matches("\"event\":\"exited\"").count(), 1);
        }
    }
}

/// Faults found in review: a password-expiry notice, rejected TOTP codes, a
/// modal dialog that a setter opens inside the agent's pass, two takeovers
/// in one session, and a malformed command file.
#[test]
#[ignore = "Requires JAVA_HOME, IBKIT_AGENT_LIBRARY, and a desktop display; run scripts/jvm-smoke.sh"]
fn native_agent_survives_review_scenarios() {
    let java_home = PathBuf::from(std::env::var_os("JAVA_HOME").expect("Set JAVA_HOME to a JDK."));
    let library = PathBuf::from(
        std::env::var_os("IBKIT_AGENT_LIBRARY")
            .expect("Set IBKIT_AGENT_LIBRARY to the built agent."),
    );
    let classes = tempfile::tempdir().unwrap();
    assert!(
        Process::new(java_home.join("bin/javac"))
            .arg("-d")
            .arg(classes.path())
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/GatewayFixture.java"))
            .status()
            .unwrap()
            .success()
    );
    for (scenario, failure) in [
        ("pwnotice", None),
        (
            "totp_wrong",
            Some(ibkit_gateway_core::ErrorCode::TotpRejected),
        ),
        ("reentrant", None),
        ("reentrant_known", None),
        ("existing2", None),
        ("badcmd", None),
        ("late_button", None),
        ("early_main", None),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let launch = AgentLaunch {
            session: scenario.into(),
            config: Config {
                state_dir: dir.path().into(),
                api_port: 14567,
                existing_session: ibkit_gateway_core::ExistingSession::Takeover,
                ..Default::default()
            },
        };
        let launch_path = dir.path().join("launch.json");
        fs::write(&launch_path, serde_json::to_vec(&launch).unwrap()).unwrap();
        let mut command = Process::new(java_home.join("bin/java"));
        command
            .arg(format!(
                "-agentpath:{}={}",
                library.display(),
                launch_path.display()
            ))
            .arg("-cp")
            .arg(classes.path())
            .arg("GatewayFixture")
            .env("IB_USERNAME", "fixture-user")
            .env("IB_PASSWORD", "fixture-secret")
            .env("FIXTURE_PORT", "14567")
            .env("FIXTURE_SCENARIO", scenario)
            .env("FIXTURE_REAUTH_REQUEST", dir.path().join("reauth-request"))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if scenario == "totp_wrong" {
            // A valid secret that the fixture never accepts.
            command.env("IB_TOTP_SECRET", "JBSWY3DPEHPK3PXP");
        }
        let mut child = command.spawn().unwrap();
        let status_path = dir
            .path()
            .join("agents")
            .join(format!("{}.json", child.id()));
        let read = || -> Option<AgentStatus> {
            serde_json::from_slice(&fs::read(&status_path).ok()?).ok()
        };
        let wait_for = |wanted: &dyn Fn(&AgentStatus) -> bool, secs: u64| {
            let deadline = Instant::now() + Duration::from_secs(secs);
            loop {
                if let Some(status) = read()
                    && wanted(&status)
                {
                    return Some(status);
                }
                if Instant::now() > deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(100));
            }
        };
        let events =
            || fs::read_to_string(status_path.with_extension("events")).unwrap_or_default();
        let outcome = match failure {
            Some(code) => wait_for(&|status| status.state == State::Failed, 90)
                .map(|status| status.result.error_code == code),
            None => wait_for(
                &|status| matches!(status.state, State::Ready | State::Failed),
                40,
            )
            .map(|status| status.state == State::Ready),
        };
        if outcome == Some(true) && scenario == "existing2" {
            fs::write(dir.path().join("reauth-request"), []).unwrap();
            let done = dir.path().join("reauth-request.done");
            let deadline = Instant::now() + Duration::from_secs(20);
            while !done.exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(100));
            }
            assert!(
                done.exists(),
                "existing2: the two conflicts were not answered: {}",
                events()
            );
        }
        if outcome == Some(true) && scenario == "late_button" {
            let done = dir.path().join("reauth-request.done");
            let deadline = Instant::now() + Duration::from_secs(10);
            while !done.exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(100));
            }
            assert!(
                done.exists(),
                "late_button: the enabled button was never clicked: {}",
                events()
            );
        }
        if outcome == Some(true) && scenario == "badcmd" {
            fs::write(
                dir.path().join(format!("command-{}.json", child.id())),
                b"{not json",
            )
            .unwrap();
            thread::sleep(Duration::from_secs(3));
        }
        let last = read();
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(outcome, Some(true), "{scenario}: {last:?} {}", events());
        if failure.is_none() {
            assert_eq!(
                last.map(|status| status.state),
                Some(State::Ready),
                "{scenario}: {}",
                events()
            );
        }
    }
}
