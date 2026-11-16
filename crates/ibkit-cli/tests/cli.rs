use std::process::Command;

fn config(dir: &std::path::Path, args: &[&str], envs: &[(&str, &str)]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ibkit"))
        .current_dir(dir)
        .arg("gateway")
        .args(args)
        .envs(envs.iter().copied())
        .env_remove("IBKIT_CONFIG")
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(if output.status.success() {
            &output.stdout
        } else {
            &output.stderr
        })
        .into_owned(),
    )
}

#[test]
fn flags_override_variables_which_override_the_file() {
    let dir = tempfile::tempdir().unwrap();
    // No file: defaults plus flags.
    let (ok, text) = config(dir.path(), &["config", "--api-port", "4001"], &[]);
    assert!(ok, "{text}");
    assert!(text.contains("api_port = 4001"), "{text}");
    std::fs::write(
        dir.path().join("config.toml"),
        "api_port = 4100\ntrading_mode = \"live\"\nrecovery_attempts = 5\n",
    )
    .unwrap();
    let (ok, text) = config(
        dir.path(),
        &["config", "--recovery-attempts", "7"],
        &[("IBKIT_API_PORT", "4200"), ("IBKIT_RECOVERY_ATTEMPTS", "6")],
    );
    assert!(ok, "{text}");
    assert!(
        text.contains("api_port = 4200"),
        "variable over file: {text}"
    );
    assert!(
        text.contains("trading_mode = \"live\""),
        "file kept: {text}"
    );
    assert!(
        text.contains("recovery_attempts = 7"),
        "flag over variable: {text}"
    );
    // Validation runs on the merged result and names the setting.
    let (ok, text) = config(dir.path(), &["config", "--java-heap-mb", "1"], &[]);
    assert!(!ok);
    assert!(text.contains("java_heap_mb"), "{text}");
    let (ok, text) = config(dir.path(), &["config"], &[("IBKIT_AUTO_LOGIN", "maybe")]);
    assert!(!ok);
    assert!(text.contains("IBKIT_AUTO_LOGIN"), "{text}");
}
