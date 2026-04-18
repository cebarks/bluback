use std::process::Command;

fn bluback() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bluback"))
}

#[test]
fn generate_completions_bash() {
    let out = bluback()
        .args(["generate", "completions", "bash"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.is_empty(), "bash completions should not be empty");
    assert!(stdout.contains("bluback"), "bash completions should reference the binary name");
}

#[test]
fn generate_completions_zsh() {
    let out = bluback()
        .args(["generate", "completions", "zsh"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.is_empty(), "zsh completions should not be empty");
    assert!(stdout.contains("bluback"), "zsh completions should reference the binary name");
}

#[test]
fn generate_completions_fish() {
    let out = bluback()
        .args(["generate", "completions", "fish"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.is_empty(), "fish completions should not be empty");
    assert!(stdout.contains("bluback"), "fish completions should reference the binary name");
}

#[test]
fn generate_man_page() {
    let out = bluback()
        .args(["generate", "man"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.is_empty(), "man page should not be empty");
    assert!(stdout.contains("bluback"), "man page should reference the binary name");
}

#[test]
fn generate_completions_invalid_shell() {
    let out = bluback()
        .args(["generate", "completions", "invalid"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "invalid shell should fail");
}

#[test]
fn generate_no_subcommand_shows_help() {
    let out = bluback()
        .args(["generate"])
        .output()
        .unwrap();
    assert!(!out.status.success());
}
