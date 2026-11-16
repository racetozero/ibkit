// These tests start shell-script launchers and check Unix process groups and
// file modes; tests/windows.rs covers the Windows platform layer.
#![cfg(unix)]
use ibkit_gateway_client::{ConfigUpdate, Control, EventKind, GatewayClient, State};
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

const TOKEN: &str = "test-token-0123456789";

#[tokio::test]
async fn control_api_serves_status_commands_settings_and_events() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = dir.path().join("launcher");
    fs::write(&launcher, "#!/bin/sh\nexec sleep 60\n").unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700)).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = dir.path().join("config.toml");
    fs::write(
        &config,
        format!(
            "executable = {launcher:?}\nagent = {launcher:?}\nstate_dir = {:?}\nsettings_dir = {:?}\nauto_login = false\nshutdown_timeout_secs = 1\ncontrol_listen = \"127.0.0.1:{port}\"\n",
            dir.path().join("state"),
            dir.path().join("settings"),
        ),
    )
    .unwrap();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ibkit"))
        .arg("gateway")
        .arg("--config")
        .arg(&config)
        .args(["run", "--recovery-delay-secs", "5"])
        .env("IBKIT_CONTROL_TOKEN", TOKEN)
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let client = GatewayClient::new(&base, TOKEN).unwrap();
    let health = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(health) = client.health().await
                && health.live
            {
                return health;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(health.controller_pid, child.id().unwrap());
    assert!(
        !client.ready().await.unwrap().ready,
        "no Gateway API answers"
    );

    let metrics = client.metrics().await.unwrap();
    for line in [
        "ibkit_gateway_up 1",
        "ibkit_gateway_ready 0",
        "# TYPE ibkit_gateway_gateway_launches_total counter",
    ] {
        assert!(metrics.contains(line), "{line} is absent from:\n{metrics}");
    }
    // Every documented operation is routed: public ones answer, protected
    // ones refuse a request without a token (an unknown route gives 404).
    let document = client.openapi().await.unwrap();
    assert_eq!(document, ibkit_gateway::openapi::document());
    let http = reqwest::Client::new();
    for (path, operations) in document["paths"].as_object().unwrap() {
        for (method, operation) in operations.as_object().unwrap() {
            let response = http
                .request(
                    method.to_uppercase().parse().unwrap(),
                    format!("{base}{path}"),
                )
                .send()
                .await
                .unwrap();
            let expected: &[u16] = if operation.get("security").is_some() {
                &[401]
            } else {
                &[200, 503]
            };
            assert!(
                expected.contains(&response.status().as_u16()),
                "{method} {path} answered {}",
                response.status()
            );
        }
    }

    let wrong = GatewayClient::new(&base, "wrong-token-0123456789").unwrap();
    let error = wrong.status().await.unwrap_err().to_string();
    assert!(
        error.contains("401") && error.contains("bearer token"),
        "{error}"
    );

    let session = client.status().await.unwrap().session;
    let mut events = client.events().await.unwrap();

    // Settings: checks run before anything is saved.
    for (key, value, expected) in [
        ("no_such_key", toml::Value::Integer(1), "unknown setting"),
        ("state_dir", toml::Value::String("/tmp".into()), "state_dir"),
        (
            "recovery_delay_secs",
            toml::Value::Integer(9),
            "--recovery-delay-secs",
        ),
        ("api_port", toml::Value::Integer(0), "api_port"),
        // The API never chooses the programs that run or its own access.
        ("executable", toml::Value::String("/bin/sh".into()), "403"),
        (
            "control_listen",
            toml::Value::String("0.0.0.0:5898".into()),
            "403",
        ),
        ("control_api_full_access", toml::Value::Boolean(true), "403"),
        // Risky values need control_api_full_access.
        (
            "trading_mode",
            toml::Value::String("live".into()),
            "control_api_full_access",
        ),
        (
            "allow_remote_api",
            toml::Value::Boolean(true),
            "control_api_full_access",
        ),
    ] {
        let error = client
            .update_config(&ConfigUpdate::from([(key.into(), Some(value))]))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{key}: {error}");
    }
    let error = client
        .set_totp_secret("123456")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("400") && error.contains("base32"), "{error}");

    let updated = client
        .update_config(&ConfigUpdate::from([(
            "api_port".into(),
            Some(toml::Value::Integer(4100)),
        )]))
        .await
        .unwrap();
    assert_eq!(updated.api_port, 4100);
    assert_eq!(client.config().await.unwrap().api_port, 4100);
    // A safe value of a guarded key is accepted.
    client
        .update_config(&ConfigUpdate::from([(
            "trading_mode".into(),
            Some(toml::Value::String("paper".into())),
        )]))
        .await
        .unwrap();
    // The reload starts a new session and the stream reports it.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = events.next().await.unwrap().unwrap();
            if matches!(event.kind, EventKind::Started) && event.session != session {
                return;
            }
        }
    })
    .await
    .unwrap();

    client
        .set_totp_secret("GEZD GNBV GY3T QOJQ GEZD GNBV GY3T QOJQ")
        .await
        .unwrap();
    client
        .set_credentials("fixture-user", "fixture-password")
        .await
        .unwrap();
    let saved = client.config().await.unwrap();
    for file in [
        saved.totp_secret_file.unwrap(),
        saved.username_file.unwrap(),
        saved.password_file.unwrap(),
    ] {
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let text = fs::read_to_string(dir.path().join("state/api-settings.toml")).unwrap();
    assert!(
        !text.contains("fixture-password") && !text.contains("GEZD"),
        "{text}"
    );
    client.clear_totp_secret().await.unwrap();
    assert!(client.config().await.unwrap().totp_secret_file.is_none());

    tokio::time::timeout(Duration::from_secs(10), async {
        while client.status().await.unwrap().state == State::Exited {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    client.control(Control::Stop).await.unwrap();
    let exit = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(exit.success());
    // The next controller keeps the settings saved through the API.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ibkit"))
        .arg("gateway")
        .arg("--config")
        .arg(&config)
        .arg("config")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("api_port = 4100"));
}

#[test]
fn the_checked_in_openapi_document_is_current() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../ibkit-gateway/openapi.json");
    let current = serde_json::to_string_pretty(&ibkit_gateway::openapi::document()).unwrap() + "\n";
    if std::env::var_os("UPDATE_OPENAPI").is_some() {
        fs::write(path, &current).unwrap();
    }
    assert!(
        fs::read_to_string(path).is_ok_and(|saved| saved == current),
        "crates/ibkit-gateway/openapi.json is stale. Run `UPDATE_OPENAPI=1 cargo test -p ibkit-cli --test control_api` and commit the result."
    );
}
