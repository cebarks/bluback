use std::path::Path;
use std::process::Command;

#[test]
fn folder_input_list_playlists_runs() {
    let fixture_path = Path::new("tests/fixtures/bdmv_sample")
        .canonicalize()
        .expect("fixture exists");
    let output = Command::new(env!("CARGO_BIN_EXE_bluback"))
        .args(["--list-playlists", "-d", fixture_path.to_str().unwrap()])
        .output()
        .expect("failed to run bluback");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("does not contain a BDMV structure"),
        "folder should be detected as valid BDMV: {}",
        stderr
    );
}

#[test]
fn folder_without_bdmv_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bluback"))
        .args(["--list-playlists", "-d", dir.path().to_str().unwrap()])
        .output()
        .expect("failed to run bluback");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not contain a BDMV structure"),
        "expected BDMV structure error, got: {}",
        stderr
    );
    assert!(!output.status.success());
}

#[test]
fn folder_input_check_skips_aacs() {
    let fixture_path = Path::new("tests/fixtures/bdmv_sample")
        .canonicalize()
        .expect("fixture exists");
    let output = Command::new(env!("CARGO_BIN_EXE_bluback"))
        .args(["--check", "-d", fixture_path.to_str().unwrap()])
        .output()
        .expect("failed to run bluback");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("AACS checks skipped") || stdout.contains("BDMV folder"),
        "expected AACS skip message, got: {}",
        stdout
    );
}

#[test]
fn batch_flag_with_folder_input_is_rejected() {
    let fixture_path = std::path::Path::new("tests/fixtures/bdmv_sample")
        .canonicalize()
        .expect("fixture exists");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bluback"))
        .args(["--batch", "-d", fixture_path.to_str().unwrap(), "--no-tui"])
        .output()
        .expect("failed to run bluback");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("batch mode is not supported with folder input"),
        "expected batch+folder error, got: {}",
        stderr
    );
    assert!(!output.status.success());
}
