use std::process::Command;

fn bluback_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bluback"))
}

#[test]
fn test_batch_conflicts_with_dry_run() {
    let output = bluback_cmd()
        .args(["--batch", "--dry-run"])
        .output()
        .expect("failed to run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "expected conflict error, got: {}",
        stderr
    );
}

#[test]
fn test_batch_conflicts_with_no_eject() {
    let output = bluback_cmd()
        .args(["--batch", "--no-eject"])
        .output()
        .expect("failed to run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "expected conflict error, got: {}",
        stderr
    );
}

#[test]
fn test_batch_conflicts_with_list_playlists() {
    let output = bluback_cmd()
        .args(["--batch", "--list-playlists"])
        .output()
        .expect("failed to run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "expected conflict error, got: {}",
        stderr
    );
}

#[test]
fn test_batch_and_no_batch_conflict() {
    let output = bluback_cmd()
        .args(["--batch", "--no-batch"])
        .output()
        .expect("failed to run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "expected conflict error, got: {}",
        stderr
    );
}

#[test]
fn test_batch_conflicts_with_check() {
    let output = bluback_cmd()
        .args(["--batch", "--check"])
        .output()
        .expect("failed to run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "expected conflict error, got: {}",
        stderr
    );
}

#[test]
fn test_batch_conflicts_with_settings() {
    let output = bluback_cmd()
        .args(["--batch", "--settings"])
        .output()
        .expect("failed to run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "expected conflict error, got: {}",
        stderr
    );
}
