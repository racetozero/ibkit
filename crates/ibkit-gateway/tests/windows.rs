//! The Windows platform layer: a session's Job Object holds the launcher and
//! every process it starts, a replacement controller reopens it by name, and
//! stop ends it. The Unix behavior is in lifecycle.rs and supervision.rs.
#![cfg(windows)]
use ibkit_gateway::{Config, Gateway, State, platform};
use std::{collections::HashSet, fs, process::Stdio, time::Duration};

/// Wait until `condition` holds (10 s at most).
async fn eventually(condition: impl Fn() -> bool) -> bool {
    for _ in 0..200 {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// The launcher starts a child and exits, as Gateway's restart child
/// outlives the first JVM: the job keeps the child, and only ending the job
/// ends it.
#[tokio::test]
async fn a_session_job_holds_the_launcher_and_its_children() {
    let session = format!("test-{}", std::process::id());
    let mut command = tokio::process::Command::new("cmd");
    command
        .args(["/c", "start", "/b", "ping", "-n", "60", "127.0.0.1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    platform::isolate_launcher(&mut command);
    let mut launcher = command.spawn().unwrap();
    let group = platform::session_spawned(launcher.id().unwrap(), &session).unwrap();
    assert!(platform::group_alive(group, &session));
    // The launcher ends; its child runs on in the job.
    let _ = launcher.wait().await;
    assert!(
        platform::group_alive(group, &session),
        "the child left the job"
    );
    assert_eq!(platform::reopen_group(group, &session), Some(group));
    assert_eq!(platform::reopen_group(group, "another-session"), None);
    platform::end_groups(&HashSet::from([group]), &session, true);
    assert!(
        eventually(|| !platform::group_alive(group, &session)).await,
        "ending the job did not end its processes"
    );
}

/// A Gateway session with a .cmd launcher that does not end by itself:
/// stop kills it after shutdown_timeout_secs and reports it.
#[tokio::test]
async fn stop_ends_a_launcher_that_does_not_exit() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = dir.path().join("launcher.cmd");
    fs::write(&launcher, "@ping -n 120 127.0.0.1 >nul\r\n").unwrap();
    let config = Config {
        executable: launcher.clone(),
        agent: launcher,
        auto_login: false,
        state_dir: dir.path().join("state"),
        settings_dir: dir.path().join("settings"),
        shutdown_timeout_secs: 1,
        ..Default::default()
    };
    let mut gateway = Gateway::new(config).unwrap();
    gateway.launch().await.unwrap();
    let session = gateway.status().session.clone();
    let group = gateway
        .status()
        .process_group
        .expect("the launcher has a job");
    assert!(platform::group_alive(group, &session));
    assert_eq!(gateway.poll().await.unwrap(), State::Starting);
    gateway.stop().await.unwrap();
    assert_eq!(gateway.status().state, State::Exited);
    assert!(
        eventually(|| !platform::group_alive(group, &session)).await,
        "stop left the launcher running"
    );
}

/// State files are replaced while another handle reads them, as the
/// controller saves its status twice a second while clients read it.
#[test]
fn private_files_are_replaced_while_they_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("status.json");
    // The reader of the previous round stays open during each write.
    let mut reader = None;
    for round in 0..50 {
        ibkit_gateway::gateway::write_private(&path, format!("{round}").as_bytes()).unwrap();
        reader = Some(fs::File::open(&path).unwrap());
    }
    drop(reader);
    assert_eq!(fs::read_to_string(&path).unwrap(), "49");
}
