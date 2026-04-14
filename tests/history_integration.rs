use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn bluback() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bluback"))
}

fn temp_db() -> PathBuf {
    let dir = std::env::temp_dir().join("bluback_integration_test");
    std::fs::create_dir_all(&dir).unwrap();
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    dir.join(format!("test_{}_{}.db", std::process::id(), id))
}

#[test]
fn test_history_list_empty() {
    let db_path = temp_db();
    let _ = std::fs::remove_file(&db_path);
    let out = bluback()
        .args(["history", "list"])
        .env("BLUBACK_HISTORY_PATH", &db_path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("No sessions") || stdout.contains("ID"));
    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn test_history_stats_empty() {
    let db_path = temp_db();
    let _ = std::fs::remove_file(&db_path);
    let out = bluback()
        .args(["history", "stats"])
        .env("BLUBACK_HISTORY_PATH", &db_path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Sessions:"));
    assert!(stdout.contains("0"));
    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn test_history_no_subcommand_defaults_to_list() {
    let db_path = temp_db();
    let _ = std::fs::remove_file(&db_path);
    let out = bluback()
        .args(["history"])
        .env("BLUBACK_HISTORY_PATH", &db_path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn test_history_export_json_empty() {
    let db_path = temp_db();
    let _ = std::fs::remove_file(&db_path);
    let out = bluback()
        .args(["history", "export"])
        .env("BLUBACK_HISTORY_PATH", &db_path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "[]");
    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn test_clap_regression_existing_flags() {
    // --check should still parse even with history subcommand available
    let out = bluback().args(["--check"]).output().unwrap();
    // --check exits 0 or 2
    assert!(out.status.code().unwrap() <= 2);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("unexpected argument"),
        "stderr: {}",
        stderr
    );
}

#[test]
fn test_clap_regression_title_flag() {
    // --title should not be confused with the history subcommand
    let out = bluback()
        .args([
            "--title",
            "history",
            "--movie",
            "--no-tui",
            "--list-playlists",
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Should fail because no disc, not because of arg parsing
    assert!(
        !stderr.contains("unexpected argument"),
        "stderr: {}",
        stderr
    );
}

#[test]
fn test_history_help() {
    let out = bluback().args(["history", "--help"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("show"));
    assert!(stdout.contains("stats"));
    assert!(stdout.contains("delete"));
    assert!(stdout.contains("clear"));
    assert!(stdout.contains("export"));
}

#[test]
fn test_history_clear_requires_confirmation() {
    let db_path = temp_db();
    let _ = std::fs::remove_file(&db_path);
    // Without --yes, clear should prompt
    let out = bluback()
        .args(["history", "clear"])
        .env("BLUBACK_HISTORY_PATH", &db_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Should prompt and cancel
    assert!(
        stdout.contains("Clear ALL history?") && stdout.contains("Cancelled"),
        "stdout: {}",
        stdout
    );
    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn test_history_show_nonexistent() {
    let db_path = temp_db();
    let _ = std::fs::remove_file(&db_path);
    let out = bluback()
        .args(["history", "show", "999"])
        .env("BLUBACK_HISTORY_PATH", &db_path)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found"), "stderr: {}", stderr);
    let _ = std::fs::remove_file(&db_path);
}
