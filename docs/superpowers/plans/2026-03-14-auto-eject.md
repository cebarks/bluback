# Auto-Eject Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automatically eject the Blu-ray disc tray after all playlists rip successfully, controlled by config and CLI flags.

**Architecture:** Add `eject` field to Config, `--eject`/`--no-eject` CLI flags, `eject_disc()` shell-out in disc.rs, and integrate into both CLI and TUI completion paths. Only ejects on full success (no failures, no dry-run, no user abort). TUI mode runs eject in a background thread with mpsc channel to avoid freezing the UI.

**Tech Stack:** Rust, clap (derive), std::process::Command, toml/serde

**Spec:** `docs/superpowers/specs/2026-03-14-auto-eject-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/config.rs` | Modify | Add `eject` field, `should_eject()` method, tests |
| `src/main.rs` | Modify | Add `--eject`/`--no-eject` flags to `Args` |
| `src/disc.rs` | Modify | Add `eject_disc()` function |
| `src/cli.rs` | Modify | Track failures, call eject on success |
| `src/tui/mod.rs` | Modify | Add `eject` field to `App` struct, initialize it |
| `src/tui/dashboard.rs` | Modify | Call eject on all-success completion |

---

## Task 1: Config — eject field and should_eject()

**Files:**
- Modify: `src/config.rs:14-20` (Config struct)
- Modify: `src/config.rs:41-81` (impl Config)
- Modify: `src/config.rs:94-192` (tests)

- [ ] **Step 1: Write failing tests for eject config parsing**

Add these tests to the existing `mod tests` block in `src/config.rs`:

```rust
#[test]
fn test_parse_eject_true() {
    let config: Config = toml::from_str("eject = true").unwrap();
    assert_eq!(config.eject, Some(true));
}

#[test]
fn test_parse_eject_false() {
    let config: Config = toml::from_str("eject = false").unwrap();
    assert_eq!(config.eject, Some(false));
}

#[test]
fn test_parse_eject_absent() {
    let config: Config = toml::from_str("").unwrap();
    assert!(config.eject.is_none());
}

#[test]
fn test_should_eject_cli_true_overrides_config() {
    let config = Config { eject: Some(false), ..Default::default() };
    assert!(config.should_eject(Some(true)));
}

#[test]
fn test_should_eject_cli_false_overrides_config() {
    let config = Config { eject: Some(true), ..Default::default() };
    assert!(!config.should_eject(Some(false)));
}

#[test]
fn test_should_eject_no_cli_uses_config() {
    let config = Config { eject: Some(true), ..Default::default() };
    assert!(config.should_eject(None));
}

#[test]
fn test_should_eject_no_cli_no_config_defaults_false() {
    let config = Config::default();
    assert!(!config.should_eject(None));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_parse_eject test_should_eject`
Expected: compilation errors — `eject` field and `should_eject()` don't exist yet.

- [ ] **Step 3: Add eject field to Config struct**

In `src/config.rs`, add to the `Config` struct:

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    pub tmdb_api_key: Option<String>,
    pub preset: Option<String>,
    pub tv_format: Option<String>,
    pub movie_format: Option<String>,
    pub eject: Option<bool>,
}
```

- [ ] **Step 4: Add should_eject() method to impl Config**

In `src/config.rs`, add to the `impl Config` block (after `resolve_format`):

```rust
pub fn should_eject(&self, cli_eject: Option<bool>) -> bool {
    cli_eject.unwrap_or_else(|| self.eject.unwrap_or(false))
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -- test_parse_eject test_should_eject`
Expected: all 7 new tests PASS.

- [ ] **Step 6: Commit**

```
feat: add eject config field and should_eject() resolution
```

---

## Task 2: CLI flags — --eject / --no-eject

**Files:**
- Modify: `src/main.rs:15-57` (Args struct)

- [ ] **Step 1: Add --eject and --no-eject flags to Args**

In `src/main.rs`, add to the `Args` struct:

```rust
/// Eject disc after successful rip
#[arg(long, conflicts_with = "no_eject")]
eject: bool,

/// Don't eject disc after rip (overrides config)
#[arg(long, conflicts_with = "eject")]
no_eject: bool,
```

- [ ] **Step 2: Add helper method to resolve CLI eject to Option<bool>**

Add an `impl Args` block in `src/main.rs`:

```rust
impl Args {
    pub fn cli_eject(&self) -> Option<bool> {
        if self.eject {
            Some(true)
        } else if self.no_eject {
            Some(false)
        } else {
            None
        }
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles without errors.

- [ ] **Step 4: Commit**

```
feat: add --eject/--no-eject CLI flags
```

---

## Task 3: eject_disc() function

**Files:**
- Modify: `src/disc.rs` (add function at end, before tests)

- [ ] **Step 1: Add eject_disc() function**

Add to `src/disc.rs` before the `#[cfg(test)]` block (or at end if no tests):

```rust
pub fn eject_disc(device: &str) -> anyhow::Result<()> {
    let status = std::process::Command::new("eject")
        .arg(device)
        .status()?;

    if !status.success() {
        anyhow::bail!(
            "eject exited with code {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles without errors.

- [ ] **Step 3: Commit**

```
feat: add eject_disc() function in disc.rs
```

---

## Task 4: CLI mode integration

**Files:**
- Modify: `src/cli.rs:255-352` (rip loop and completion)

- [ ] **Step 1: Add had_failure tracking to the rip loop**

In `src/cli.rs`, add `let mut had_failure = false;` before the rip loop (before line 255). Then set it to `true` in two places:

1. When ffmpeg exits with non-success (around line 323-341, inside `if !status.success()`):
   Add `had_failure = true;` before the `continue;`

2. When stream probing fails (around line 276-284, inside the `None` arm):
   Add `had_failure = true;` before the `continue;`

Note: the "already exists" skip path (lines 261-268) does NOT set `had_failure` — existing files count as success.

- [ ] **Step 2: Add eject call after completion**

In `src/cli.rs`, after the "All done!" println (line 347-351), before the final `Ok(())`, add:

```rust
if !had_failure && config.should_eject(args.cli_eject()) {
    println!("Ejecting disc...");
    if let Err(e) = disc::eject_disc(&device) {
        println!("Warning: failed to eject disc: {}", e);
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles without errors.

- [ ] **Step 4: Commit**

```
feat: CLI mode auto-eject on successful completion
```

---

## Task 5: TUI mode integration

**Files:**
- Modify: `src/tui/mod.rs:29-79` (App struct)
- Modify: `src/tui/mod.rs:82-119` (App::new)
- Modify: `src/tui/mod.rs:136-246` (run_app function)
- Modify: `src/tui/dashboard.rs:211-225` (tick function)
- [ ] **Step 1: Add eject and eject_rx fields to App struct**

In `src/tui/mod.rs`, add to the `App` struct (after `status_message`):

```rust
pub eject: bool,
pub eject_rx: Option<mpsc::Receiver<anyhow::Result<()>>>,
```

And initialize them in `App::new()`:

```rust
eject: false,
eject_rx: None,
```

- [ ] **Step 2: Set the eject field in run_app()**

In `src/tui/mod.rs` `run_app()`, after `app.config = config.clone();` (line 138), add:

```rust
app.eject = config.should_eject(args.cli_eject());
```

- [ ] **Step 3: Spawn non-blocking eject and poll in dashboard tick()**

In `src/tui/dashboard.rs` `tick()`, replace the `if all_done && !app.rip_jobs.is_empty()` block (lines 216-225) with:

```rust
if all_done && !app.rip_jobs.is_empty() {
    // Clean up child if somehow still around
    if let Some(ref mut child) = app.rip_child {
        let _ = child.wait();
    }
    app.rip_child = None;
    app.progress_rx = None;

    // Poll for eject completion if already in progress
    if let Some(ref rx) = app.eject_rx {
        match rx.try_recv() {
            Ok(Ok(())) => {
                app.eject_rx = None;
                app.status_message.clear();
                app.screen = Screen::Done;
            }
            Ok(Err(e)) => {
                app.eject_rx = None;
                app.status_message = format!("Warning: failed to eject disc: {}", e);
                app.screen = Screen::Done;
            }
            Err(mpsc::TryRecvError::Empty) => {
                // Still ejecting, keep waiting
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                app.eject_rx = None;
                app.status_message = "Warning: eject thread terminated unexpectedly".into();
                app.screen = Screen::Done;
            }
        }
        return Ok(());
    }

    let all_succeeded = app.rip_jobs.iter().all(|j| matches!(j.status, PlaylistStatus::Done(_)));

    // If eject enabled and all succeeded, spawn eject thread
    if app.eject && all_succeeded {
        let device = app.args.device.to_string_lossy().to_string();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(crate::disc::eject_disc(&device));
        });
        app.eject_rx = Some(rx);
        app.status_message = "Ejecting disc...".into();
        return Ok(());
    }

    app.screen = Screen::Done;
    return Ok(());
}
```

This keeps all eject logic inside the `all_done` block. On first entry it spawns the thread; on subsequent ticks it polls the channel; only when the channel delivers a result (or no eject needed) does it transition to `Screen::Done`.

- [ ] **Step 4: Add `use std::thread;` import to dashboard.rs if not present**

Check top of `src/tui/dashboard.rs` for `use std::thread;`. The file already imports `std::sync::{mpsc, Arc, Mutex}` and `std::thread` at lines 5-6. Verify these are present — no change needed if they are.

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: compiles without errors.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: all tests pass (including new config tests from Task 1).

- [ ] **Step 7: Run clippy**

Run: `cargo clippy`
Expected: no warnings.

- [ ] **Step 8: Commit**

```
feat: TUI mode non-blocking auto-eject on successful completion
```
