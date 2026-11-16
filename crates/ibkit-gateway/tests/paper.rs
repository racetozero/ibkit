#![cfg(feature = "paper-tests")]
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
use tokio::{net::TcpListener, process::Command, time::timeout};

#[tokio::test]
async fn paper_checker_rejects_live_or_writable_configs_and_preserves_existing_reports() {
    let dir = tempfile::tempdir().unwrap();
    let intents = dir.path().join("intents.json");
    fs::write(
        &intents,
        r#"{"expected_perm_ids":[],"unacknowledged_intents":0}"#,
    )
    .unwrap();
    let config = dir.path().join("config.toml");
    let report = dir.path().join("report.jsonl");
    for (mode, read_only, account) in [
        ("live", true, "DU123"),
        ("paper", false, "DU123"),
        ("paper", true, "U123"),
    ] {
        fs::write(
            &config,
            format!("trading_mode = {mode:?}\nread_only_api = {read_only}\n"),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ibkit-gateway-paper-check"))
            .arg("--config")
            .arg(&config)
            .arg("--intents")
            .arg(&intents)
            .arg("--report")
            .arg(&report)
            .arg("--paper-account")
            .arg(account)
            .output()
            .await
            .unwrap();
        assert!(!output.status.success());
        assert!(
            !report.exists(),
            "A rejected account selection must not start a report"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("dedicated DU paper account"));
    }
    fs::write(&config, "trading_mode = 'paper'\nread_only_api = true\n").unwrap();
    fs::write(&report, b"previous evidence").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ibkit-gateway-paper-check"))
        .arg("--config")
        .arg(config)
        .arg("--intents")
        .arg(intents)
        .arg("--report")
        .arg(&report)
        .arg("--paper-account")
        .arg("DU123")
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(report).unwrap(), b"previous evidence");
}

#[tokio::test]
async fn missing_paper_gateway_produces_a_private_failed_report_within_the_run_limit() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let config = dir.path().join("config.toml");
    let intents = dir.path().join("intents.json");
    let report = dir.path().join("report.jsonl");
    fs::write(
        &config,
        format!("trading_mode = 'paper'\nread_only_api = true\napi_port = {port}\n"),
    )
    .unwrap();
    fs::write(
        &intents,
        r#"{"expected_perm_ids":[],"unacknowledged_intents":0}"#,
    )
    .unwrap();
    let output = timeout(
        Duration::from_secs(5),
        Command::new(env!("CARGO_BIN_EXE_ibkit-gateway-paper-check"))
            .arg("--config")
            .arg(config)
            .arg("--intents")
            .arg(intents)
            .arg("--report")
            .arg(&report)
            .args(["--paper-account", "DU123", "--duration-secs", "1"])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        fs::metadata(&report).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let events: Vec<serde_json::Value> = fs::read_to_string(report)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let summary = events
        .iter()
        .find(|event| event["event"] == "summary")
        .unwrap();
    assert_eq!(summary["passed"], false);
    assert_eq!(summary["results"]["reconciled_sessions"], 0);
    assert_eq!(summary["results"]["weekly_renewal_observed"], false);
}

#[tokio::test]
async fn interrupt_during_connection_retry_records_an_incomplete_result() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let config = dir.path().join("config.toml");
    let intents = dir.path().join("intents.json");
    let report = dir.path().join("report.jsonl");
    fs::write(
        &config,
        format!("trading_mode = 'paper'\nread_only_api = true\napi_port = {port}\n"),
    )
    .unwrap();
    fs::write(
        &intents,
        r#"{"expected_perm_ids":[],"unacknowledged_intents":0}"#,
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ibkit-gateway-paper-check"))
        .arg("--config")
        .arg(config)
        .arg("--intents")
        .arg(intents)
        .arg("--report")
        .arg(&report)
        .args(["--paper-account", "DU123", "--duration-secs", "600"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    timeout(Duration::from_secs(5), async {
        while !fs::read_to_string(&report).is_ok_and(|text| text.contains("retrying")) {
            assert!(child.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        std::process::Command::new("/bin/kill")
            .args(["-INT", &child.id().unwrap().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let output = timeout(Duration::from_secs(3), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(!output.status.success());
    let events: Vec<serde_json::Value> = fs::read_to_string(report)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        events
            .iter()
            .any(|event| event["event"] == "summary" && event["passed"] == false)
    );
}
