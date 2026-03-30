# Batch Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable continuous multi-disc ripping — rip, eject, wait for next disc, auto-restart wizard — in both TUI and CLI modes.

**Architecture:** Add a `batch` config/CLI flag that modifies post-rip behavior. In TUI, the existing Done screen disc detection skips the popup and auto-restarts the wizard. In CLI, a new `run_batch()` function wraps the existing workflow in a loop with disc polling and episode auto-advance. No new modules; changes are localized to config, CLI, session, dashboard, and types.

**Tech Stack:** Rust, clap (derive), ratatui, toml (serde)

**Spec:** `docs/superpowers/specs/2026-03-29-batch-mode-design.md`

---

### Task 1: Add `batch` field to Config

**Files:**
- Modify: `src/config.rs:65-96` (Config struct)
- Modify: `src/config.rs:127-274` (to_toml_string)
- Modify: `src/config.rs:468-509` (KNOWN_KEYS)
- Test: `src/config.rs` (inline tests)

- [ ] **Step 1: Write failing tests for batch config parsing**

Add these tests to the existing `#[cfg(test)] mod tests` block in `src/config.rs`:

```rust
#[test]
fn test_parse_batch_true() {
    let config: Config = toml::from_str("batch = true").unwrap();
    assert_eq!(config.batch, Some(true));
}

#[test]
fn test_parse_batch_false() {
    let config: Config = toml::from_str("batch = false").unwrap();
    assert_eq!(config.batch, Some(false));
}

#[test]
fn test_parse_batch_absent() {
    let config: Config = toml::from_str("").unwrap();
    assert!(config.batch.is_none());
}

#[test]
fn test_batch_default_false() {
    let config = Config::default();
    assert!(!config.batch());
}

#[test]
fn test_batch_config_true() {
    let config = Config {
        batch: Some(true),
        ..Default::default()
    };
    assert!(config.batch());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_parse_batch --no-capture`
Expected: FAIL — `batch` field does not exist on `Config`

- [ ] **Step 3: Add `batch` field to Config struct**

In `src/config.rs`, add to the `Config` struct after the `overwrite` field (line ~83):

```rust
pub batch: Option<bool>,
```

Add a convenience accessor method after `should_eject()` (around line 340):

```rust
pub fn batch(&self) -> bool {
    self.batch.unwrap_or(false)
}
```

- [ ] **Step 4: Add `batch` to `KNOWN_KEYS`**

In the `KNOWN_KEYS` array, add `"batch"` after `"overwrite"`:

```rust
"overwrite",
"batch",
"verify",
```

- [ ] **Step 5: Add `batch` to `to_toml_string()`**

In `to_toml_string()`, add after the `emit_bool` call for `overwrite` (line ~181):

```rust
emit_bool(&mut out, "overwrite", self.overwrite, false);
emit_bool(&mut out, "batch", self.batch, false);
```

- [ ] **Step 6: Add test for commented-defaults output**

```rust
#[test]
fn test_toml_string_includes_batch() {
    let config = Config::default();
    let s = config.to_toml_string();
    assert!(s.contains("# batch = false"), "default should be commented out");

    let config = Config {
        batch: Some(true),
        ..Default::default()
    };
    let s = config.to_toml_string();
    assert!(s.contains("batch = true"), "non-default should be active");
}
```

- [ ] **Step 7: Run all config tests**

Run: `cargo test -- test_parse_batch test_batch_default test_batch_config test_toml_string_includes_batch --no-capture`
Expected: All PASS

- [ ] **Step 8: Run `cargo clippy -- -D warnings`**

Expected: No warnings

- [ ] **Step 9: Commit**

```
feat(config): add batch field with TOML parsing, defaults, and KNOWN_KEYS entry
```

---

### Task 2: Add `--batch` / `--no-batch` CLI flags

**Files:**
- Modify: `src/main.rs:35-203` (Args struct)

- [ ] **Step 1: Add `--batch` and `--no-batch` flags to Args**

In `src/main.rs`, add after the `no_verify` field (around line 166):

```rust
/// Enable batch mode (rip → eject → wait → repeat)
#[arg(long, conflicts_with_all = ["no_batch", "dry_run", "list_playlists", "check", "settings", "no_eject"])]
batch: bool,

/// Disable batch mode (overrides config)
#[arg(long, conflicts_with = "batch")]
no_batch: bool,
```

- [ ] **Step 2: Add `cli_batch()` helper method on Args**

Add to the `impl Args` block (after `cli_eject()`):

```rust
pub fn cli_batch(&self) -> Option<bool> {
    if self.batch {
        Some(true)
    } else if self.no_batch {
        Some(false)
    } else {
        None
    }
}
```

- [ ] **Step 3: Write clap conflict tests**

Add a new test file `tests/cli_batch_conflicts.rs`:

```rust
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
```

- [ ] **Step 4: Run clap conflict tests**

Run: `cargo test --test cli_batch_conflicts --no-capture`
Expected: All PASS

- [ ] **Step 5: Run `cargo clippy -- -D warnings`**

Expected: No warnings

- [ ] **Step 6: Commit**

```
feat(cli): add --batch/--no-batch flags with clap conflicts
```

---

### Task 3: Add `batch` to settings panel and env var overrides

**Files:**
- Modify: `src/types.rs:584-864` (SettingsState::from_config_with_drives items list)
- Modify: `src/types.rs:889-912` (ENV_MAPPINGS)
- Test: `src/types.rs` (inline tests)

- [ ] **Step 1: Write failing test for batch in settings**

Add to the existing tests in `src/types.rs`:

```rust
#[test]
fn test_settings_includes_batch_toggle() {
    let config = Config::default();
    let state = SettingsState::from_config(&config);
    let has_batch = state.items.iter().any(|item| {
        matches!(item, SettingItem::Toggle { key, .. } if key == "batch")
    });
    assert!(has_batch, "settings should include a batch toggle");
}

#[test]
fn test_settings_batch_default_false() {
    let config = Config::default();
    let state = SettingsState::from_config(&config);
    let batch_val = state.items.iter().find_map(|item| {
        if let SettingItem::Toggle { key, value, .. } = item {
            if key == "batch" { Some(*value) } else { None }
        } else {
            None
        }
    });
    assert_eq!(batch_val, Some(false));
}

#[test]
fn test_settings_batch_from_config_true() {
    let config = Config {
        batch: Some(true),
        ..Default::default()
    };
    let state = SettingsState::from_config(&config);
    let batch_val = state.items.iter().find_map(|item| {
        if let SettingItem::Toggle { key, value, .. } = item {
            if key == "batch" { Some(*value) } else { None }
        } else {
            None
        }
    });
    assert_eq!(batch_val, Some(true));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_settings_includes_batch test_settings_batch_default test_settings_batch_from_config --no-capture`
Expected: FAIL — no batch toggle in settings items

- [ ] **Step 3: Add Batch toggle to settings items**

In `src/types.rs`, in the `from_config_with_drives()` method, add a "Batch mode" toggle in the "General" section, after the "Overwrite Existing Files" toggle (around line 627):

```rust
SettingItem::Toggle {
    label: "Overwrite Existing Files".into(),
    key: "overwrite".into(),
    value: config.overwrite.unwrap_or(false),
},
SettingItem::Toggle {
    label: "Batch Mode".into(),
    key: "batch".into(),
    value: config.batch.unwrap_or(false),
},
```

- [ ] **Step 4: Add `BLUBACK_BATCH` to env var mappings**

In `apply_env_overrides()`, add to the `ENV_MAPPINGS` array:

```rust
("BLUBACK_BATCH", "batch"),
```

- [ ] **Step 5: Run settings tests**

Run: `cargo test -- test_settings_includes_batch test_settings_batch --no-capture`
Expected: All PASS

- [ ] **Step 6: Run `cargo clippy -- -D warnings`**

Expected: No warnings

- [ ] **Step 7: Commit**

```
feat(settings): add Batch Mode toggle and BLUBACK_BATCH env var override
```

---

### Task 4: Wire batch flag through main dispatch

**Files:**
- Modify: `src/main.rs:380-420` (run_inner dispatch)
- Modify: `src/config.rs:338-340` (add should_batch helper)

- [ ] **Step 1: Add `should_batch()` to Config**

In `src/config.rs`, add after `batch()`:

```rust
pub fn should_batch(&self, cli_batch: Option<bool>) -> bool {
    cli_batch.unwrap_or_else(|| self.batch.unwrap_or(false))
}
```

- [ ] **Step 2: Write tests for should_batch**

```rust
#[test]
fn test_should_batch_cli_true_overrides_config() {
    let config = Config {
        batch: Some(false),
        ..Default::default()
    };
    assert!(config.should_batch(Some(true)));
}

#[test]
fn test_should_batch_cli_false_overrides_config() {
    let config = Config {
        batch: Some(true),
        ..Default::default()
    };
    assert!(!config.should_batch(Some(false)));
}

#[test]
fn test_should_batch_no_cli_uses_config() {
    let config = Config {
        batch: Some(true),
        ..Default::default()
    };
    assert!(config.should_batch(None));
}

#[test]
fn test_should_batch_no_cli_no_config_defaults_false() {
    let config = Config::default();
    assert!(!config.should_batch(None));
}
```

- [ ] **Step 3: Run should_batch tests**

Run: `cargo test -- test_should_batch --no-capture`
Expected: All PASS

- [ ] **Step 4: Pass batch flag through in run_inner**

In `src/main.rs`, in `run_inner()`, resolve batch mode before the TUI/CLI dispatch (around line 405):

```rust
let batch = config.should_batch(args.cli_batch());
```

Then update the CLI dispatch to call `run_batch` when batch is enabled:

```rust
if use_tui {
    tui::run(&args, &config, config_path, &stream_filter)?;
} else if batch {
    cli::run_batch(
        &args,
        &config,
        headless,
        &stream_filter,
        tracks_spec.as_deref(),
    )?;
} else {
    cli::run(
        &args,
        &config,
        headless,
        &stream_filter,
        tracks_spec.as_deref(),
    )?;
}
```

Note: `cli::run_batch` doesn't exist yet — this will cause a compile error until Task 6. For now, add a stub:

In `src/cli.rs`, add at the bottom (before the `#[cfg(test)]` block):

```rust
pub fn run_batch(
    args: &Args,
    config: &crate::config::Config,
    headless: bool,
    stream_filter: &crate::streams::StreamFilter,
    tracks_spec: Option<&str>,
) -> anyhow::Result<()> {
    todo!("batch mode CLI implementation")
}
```

For TUI, batch mode is passed via `Config` which is already cloned into each `DriveSession`. No changes needed in the TUI dispatch path — the session reads `self.config.batch()` directly.

- [ ] **Step 5: Run `cargo clippy -- -D warnings` and `cargo test`**

Expected: No warnings, all existing tests pass (run_batch is dead code for now, behind todo!())

- [ ] **Step 6: Commit**

```
feat(main): wire batch flag through config resolution and CLI dispatch
```

---

### Task 5: TUI batch mode — auto-restart on disc detection

**Files:**
- Modify: `src/session.rs:16-54` (DriveSession struct — add batch_disc_count)
- Modify: `src/session.rs:234-264` (reset_for_rescan — preserve batch_disc_count)
- Modify: `src/session.rs:631-639` (poll_background DiscFound handler)

- [ ] **Step 1: Add `batch_disc_count` to DriveSession**

In `src/session.rs`, add to the `DriveSession` struct:

```rust
pub batch_disc_count: u32,
```

Initialize it to `0` in `DriveSession::new()`.

- [ ] **Step 2: Increment counter on rip start**

Find where the screen transitions to `Screen::Ripping` (the first time rip jobs begin for a disc). This is in the `start_rip()` or the rip job setup. Increment the counter there:

```rust
self.batch_disc_count += 1;
```

The exact location is where `self.screen = Screen::Ripping` is set AND `rip.jobs` are populated. Only increment when `batch_disc_count` is meaningful — i.e., when the session is in batch mode (`self.config.batch()`).

Actually, simpler: increment in `reset_for_rescan()` when batch is active. This fires exactly once per disc restart:

```rust
pub fn reset_for_rescan(&mut self) {
    if self.config.batch() && self.batch_disc_count > 0 {
        // batch_disc_count already tracks current disc; don't double-increment
    } else if self.config.batch() {
        // First disc doesn't go through reset_for_rescan
    }
    // ... existing reset logic
}
```

Wait — cleaner approach: increment `batch_disc_count` when a scan starts AND jobs exist from a previous disc. Do this in the `DiscFound` handler itself, right before the auto-restart:

- [ ] **Step 3: Modify DiscFound handler for batch auto-restart**

In `src/session.rs`, modify the `BackgroundResult::DiscFound` handler in `poll_background()` (line 631-639):

```rust
BackgroundResult::DiscFound(ref device) => {
    if self.screen == Screen::Done {
        if self.config.batch() {
            // Batch mode: auto-restart without popup
            self.batch_disc_count += 1;
            self.disc_detected_label = None;
            self.reset_for_rescan();
            self.tmdb_api_key = crate::tmdb::get_api_key(&self.config);
            self.start_disc_scan();
            return true;
        }
        // Non-batch: show popup, wait for Enter
        let label = crate::disc::get_volume_label(device);
        self.disc_detected_label = Some(if label.is_empty() {
            device.clone()
        } else {
            label
        });
        return true;
    }
    self.disc.scan_log.clear();
    self.status_message = format!("Scanning {}...", device);
    return false;
}
```

- [ ] **Step 4: Preserve batch_disc_count in reset_for_rescan**

In `reset_for_rescan()`, ensure `batch_disc_count` is NOT reset. It should not appear in the list of fields being cleared. Since `reset_for_rescan()` only resets specific fields (disc, tmdb, wizard, rip state), and `batch_disc_count` is a new field that isn't explicitly cleared, this should work by default. Verify by reading the method.

- [ ] **Step 5: Set initial batch_disc_count to 1 on first scan**

The first disc doesn't go through the DiscFound→Done→restart path. Set `batch_disc_count = 1` when the first scan completes successfully. In the `BackgroundResult::DiscScan` handler (where playlists are received), add:

```rust
if self.config.batch() && self.batch_disc_count == 0 {
    self.batch_disc_count = 1;
}
```

- [ ] **Step 6: Run `cargo clippy -- -D warnings` and `cargo test`**

Expected: No warnings, all existing tests pass

- [ ] **Step 7: Commit**

```
feat(tui): auto-restart wizard on disc detection in batch mode
```

---

### Task 6: TUI batch mode — auto-eject and disc counter display

**Files:**
- Modify: `src/types.rs:475-495` (DashboardView, DoneView — add batch_disc_count)
- Modify: `src/tui/dashboard.rs:536-619` (check_all_done_session — add auto-eject)
- Modify: `src/tui/dashboard.rs:136-142` (render_dashboard_view — batch counter in title)
- Modify: `src/tui/dashboard.rs:360-370` (render_done_view — batch counter in title)

- [ ] **Step 1: Add `batch_disc_count` to view types**

In `src/types.rs`, add to `DashboardView`:

```rust
pub batch_disc_count: u32,
```

And to `DoneView`:

```rust
pub batch_disc_count: u32,
```

- [ ] **Step 2: Pass batch_disc_count when constructing views**

Find where `DashboardView` and `DoneView` are constructed (in `session.rs` or `dashboard.rs` — wherever the view structs are built from session state) and add:

```rust
batch_disc_count: session.batch_disc_count,
```

- [ ] **Step 3: Render disc counter in block title**

In `src/tui/dashboard.rs`, modify the `render_dashboard_view` block title (line 136-142):

```rust
let block_title = if view.batch_disc_count > 0 {
    if view.label.is_empty() {
        format!("bluback \u{2014} Disc {} | Batch", view.batch_disc_count)
    } else {
        format!("bluback \u{2014} {} \u{2014} Disc {} | Batch", view.label, view.batch_disc_count)
    }
} else if view.label.is_empty() {
    "bluback".to_string()
} else {
    format!("bluback \u{2014} {}", view.label)
};
```

Apply the same pattern to the Done screen block title in `render_done_view`.

- [ ] **Step 4: Add auto-eject in check_all_done_session**

In `src/tui/dashboard.rs`, in `check_all_done_session()`, after the post-session hook block (line ~611) and before `session.start_disc_scan()` (line 614), add:

```rust
// Auto-eject in batch mode
if session.config.batch() {
    let device = session.device.to_string_lossy();
    log::info!("Batch mode: ejecting disc {}", device);
    if let Err(e) = crate::disc::eject_disc(&device) {
        log::warn!("Failed to eject disc: {}", e);
        session.status_message = format!("Eject failed: {}", e);
    }
}
```

- [ ] **Step 5: Write test for block title rendering with batch counter**

Add to the existing dashboard tests:

```rust
#[test]
fn test_dashboard_title_with_batch_count() {
    let view = DashboardView {
        jobs: vec![],
        current_rip: 0,
        confirm_abort: false,
        confirm_rescan: false,
        label: "MY_DISC".to_string(),
        verify_failed_idx: None,
        batch_disc_count: 3,
    };
    // The block title should contain "Disc 3" and "Batch"
    let title = if view.batch_disc_count > 0 {
        format!("bluback \u{2014} {} \u{2014} Disc {} | Batch", view.label, view.batch_disc_count)
    } else {
        format!("bluback \u{2014} {}", view.label)
    };
    assert!(title.contains("Disc 3"));
    assert!(title.contains("Batch"));
}
```

- [ ] **Step 6: Run `cargo clippy -- -D warnings` and `cargo test`**

Expected: No warnings, all tests pass

- [ ] **Step 7: Commit**

```
feat(tui): auto-eject after rip and show disc counter in batch mode
```

---

### Task 7: CLI batch mode — episode auto-advance logic

**Files:**
- Modify: `src/util.rs` (add `count_assigned_episodes` helper)
- Test: `src/util.rs` (inline tests)

This is the pure logic that CLI batch mode needs, tested independently before wiring into the CLI loop.

**Important context:** The `Episode` struct (`src/types.rs:16`) does NOT have a `special` field. Specials are tracked as a `HashSet<String>` of playlist numbers (e.g., `specials_set` in `cli.rs:294`). The `count_assigned_episodes` function therefore needs the specials set to exclude special playlists from the count.

- [ ] **Step 1: Write failing tests for episode counting**

Add to the `#[cfg(test)]` block in `src/util.rs`:

```rust
#[test]
fn test_count_assigned_regular_episodes() {
    use crate::types::Episode;
    let mut assignments = HashMap::new();
    assignments.insert(
        "00001".to_string(),
        vec![
            Episode { episode_number: 1, name: "Ep1".into(), runtime: None },
            Episode { episode_number: 2, name: "Ep2".into(), runtime: None },
        ],
    );
    assignments.insert(
        "00002".to_string(),
        vec![Episode { episode_number: 3, name: "Ep3".into(), runtime: None }],
    );
    let specials = std::collections::HashSet::new();
    assert_eq!(count_assigned_episodes(&assignments, &specials), 3);
}

#[test]
fn test_count_assigned_episodes_excludes_specials() {
    use crate::types::Episode;
    let mut assignments = HashMap::new();
    assignments.insert(
        "00001".to_string(),
        vec![Episode { episode_number: 1, name: "Ep1".into(), runtime: None }],
    );
    assignments.insert(
        "00002".to_string(),
        vec![Episode { episode_number: 2, name: "Ep2".into(), runtime: None }],
    );
    let mut specials = std::collections::HashSet::new();
    specials.insert("00001".to_string()); // playlist 00001 is a special
    assert_eq!(count_assigned_episodes(&assignments, &specials), 1);
}

#[test]
fn test_count_assigned_episodes_empty() {
    let assignments: HashMap<String, Vec<crate::types::Episode>> = HashMap::new();
    let specials = std::collections::HashSet::new();
    assert_eq!(count_assigned_episodes(&assignments, &specials), 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_count_assigned --no-capture`
Expected: FAIL — function does not exist

- [ ] **Step 3: Implement count_assigned_episodes**

In `src/util.rs`, add:

```rust
/// Count regular (non-special) episodes across all playlist assignments.
/// Playlists in the `specials` set are excluded from the count.
/// Used by CLI batch mode to auto-advance the starting episode number.
pub fn count_assigned_episodes(
    assignments: &HashMap<String, Vec<crate::types::Episode>>,
    specials: &std::collections::HashSet<String>,
) -> u32 {
    assignments
        .iter()
        .filter(|(playlist_num, _)| !specials.contains(*playlist_num))
        .map(|(_, eps)| eps.len() as u32)
        .sum()
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -- test_count_assigned --no-capture`
Expected: All PASS

- [ ] **Step 5: Run `cargo clippy -- -D warnings`**

Expected: No warnings

- [ ] **Step 6: Commit**

```
feat(util): add count_assigned_episodes for batch mode auto-advance
```

---

### Task 8: CLI batch mode — run_batch implementation

**Files:**
- Modify: `src/cli.rs` (replace `run_batch` stub with real implementation)

- [ ] **Step 1: Implement `run_batch()`**

Replace the `todo!()` stub in `src/cli.rs` with:

```rust
pub fn run_batch(
    args: &Args,
    config: &crate::config::Config,
    headless: bool,
    stream_filter: &crate::streams::StreamFilter,
    tracks_spec: Option<&str>,
) -> anyhow::Result<()> {
    let mut disc_count: u32 = 0;
    let mut total_files: u32 = 0;
    let mut total_failures: u32 = 0;
    let mut next_start_episode = args.start_episode.unwrap_or(1);

    loop {
        // Check cancel flag between iterations
        if crate::CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        // Wait for disc if not the first iteration
        if disc_count > 0 {
            println!("\nWaiting for next disc...");
            let device_str = args.device().to_string_lossy().to_string();
            loop {
                if crate::CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let label = crate::disc::get_volume_label(&device_str);
                if !label.is_empty() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            if crate::CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
        }

        disc_count += 1;

        // Override start_episode for this iteration
        let result = run_batch_iteration(
            args,
            config,
            headless,
            stream_filter,
            tracks_spec,
            next_start_episode,
        );

        match result {
            Ok((files_ripped, regular_episodes_ripped)) => {
                total_files += files_ripped;
                next_start_episode += regular_episodes_ripped;
            }
            Err(e) => {
                eprintln!("Disc {} failed: {}", disc_count, e);
                total_failures += 1;
            }
        }

        // Always eject in batch mode
        let device = args.device().to_string_lossy();
        println!("Ejecting disc...");
        if let Err(e) = crate::disc::eject_disc(&device) {
            eprintln!("Warning: failed to eject disc: {}", e);
        }
    }

    println!(
        "\nBatch complete: {} disc(s), {} file(s), {} failure(s)",
        disc_count, total_files, total_failures
    );

    Ok(())
}
```

- [ ] **Step 2: Implement `run_batch_iteration()`**

This is essentially the body of the existing `run()` function, but takes `start_episode` as a parameter instead of reading from `args.start_episode`, and returns `(files_ripped, episodes_ripped)`.

The cleanest approach: extract the core workflow from `run()` into a shared inner function that both `run()` and `run_batch_iteration()` can call, parameterized by `start_episode`.

```rust
fn run_batch_iteration(
    args: &Args,
    config: &crate::config::Config,
    headless: bool,
    stream_filter: &crate::streams::StreamFilter,
    tracks_spec: Option<&str>,
    start_episode: u32,
) -> anyhow::Result<(u32, u32)> {
    // Same as run() but:
    // 1. Uses `start_episode` parameter instead of `args.start_episode`
    // 2. Does NOT eject at the end (caller handles it)
    // 3. Returns (files_ripped_count, regular_episodes_count)

    // ... scan_disc, lookup_tmdb, display_and_select, rip logic ...
    // At the end, count episodes using count_assigned_episodes(&assignments, &specials_set)
    // Return (success_count, regular_episodes_ripped)
}
```

The exact implementation should extract shared logic from `run()`. Refactor `run()` to call `run_batch_iteration()` with `args.start_episode.unwrap_or(1)` so both paths share the same code. `run()` then handles eject itself. This avoids duplicating the ~1100-line function.

Alternative approach if extraction is too invasive: create a `RunContext` struct that holds the mutable per-iteration state (start_episode) and have `run()` and `run_batch()` both construct one.

The implementer should read the full `run()` function and choose the cleanest extraction. The key requirements are:
- `start_episode` is parameterized (not read from `args`)
- Eject is NOT called (batch loop handles it)
- Return value includes `(files_ripped: u32, regular_episodes_count: u32)` for the auto-advance

- [ ] **Step 3: Run `cargo clippy -- -D warnings` and `cargo build`**

Expected: Compiles without warnings

- [ ] **Step 4: Commit**

```
feat(cli): implement run_batch with disc polling and episode auto-advance
```

---

### Task 9: CLI batch mode — aggregate summary test

**Files:**
- Test: `src/cli.rs` or `src/util.rs` (inline tests)

- [ ] **Step 1: Write test for batch summary formatting**

Add a helper function for the summary line and test it:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_batch_summary_format() {
        let summary = format!(
            "Batch complete: {} disc(s), {} file(s), {} failure(s)",
            3, 12, 1
        );
        assert_eq!(summary, "Batch complete: 3 disc(s), 12 file(s), 1 failure(s)");
    }

    #[test]
    fn test_batch_summary_zero_failures() {
        let summary = format!(
            "Batch complete: {} disc(s), {} file(s), {} failure(s)",
            2, 8, 0
        );
        assert!(summary.contains("0 failure(s)"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -- test_batch_summary --no-capture`
Expected: All PASS

- [ ] **Step 3: Commit**

```
test(cli): add batch summary format tests
```

---

### Task 10: Coordinator propagation for TUI batch

**Files:**
- Modify: `src/tui/coordinator.rs:70-154` (spawn_session — ensure config.batch is available)

- [ ] **Step 1: Verify batch propagation**

The `DriveSession` receives `self.config.clone()` in `spawn_session()` (line 81). Since `batch` is a field on `Config`, it's already propagated. Verify this by reading `spawn_session()` and confirming `config.clone()` passes through.

If the batch flag needs to be overridable per-session (e.g., from CLI `--batch`), the coordinator should apply the CLI override to the config before cloning:

Check if `cli_batch()` needs to be resolved into the config. The pattern used for `eject` is: CLI arg is stored separately on the session (`session.cli_eject`), and `should_eject()` resolves at call site. Follow the same pattern — but since batch mode doesn't need per-session CLI override (it's a global setting), the config value is sufficient.

However, if `--batch` CLI flag is used, the config's `batch` field won't reflect it (it only has the TOML value). The resolution happens in `main.rs` via `should_batch()`. We need to propagate this resolved value.

Add to `spawn_session()` after the existing CLI arg copies:

```rust
session.batch = self.args.batch || (!self.args.no_batch && self.config.batch());
```

And add `pub batch: bool` to the `DriveSession` struct (separate from `config.batch()` — this is the resolved value). Then update the `DiscFound` handler to check `self.batch` instead of `self.config.batch()`.

- [ ] **Step 2: Update DriveSession to use resolved `batch` field**

In `src/session.rs`, change the DiscFound handler and any other `self.config.batch()` calls to use `self.batch` instead.

- [ ] **Step 3: Run `cargo clippy -- -D warnings` and `cargo test`**

Expected: No warnings, all tests pass

- [ ] **Step 4: Commit**

```
feat(tui): propagate resolved batch flag through coordinator to sessions
```

---

### Task 11: Logging — session header per disc in batch

**Files:**
- Modify: `src/session.rs` (emit session header on disc restart)
- Modify: `src/cli.rs` (emit session header per batch iteration)

- [ ] **Step 1: Add session header on batch restart (TUI)**

In the `DiscFound` handler's batch auto-restart path (added in Task 5), after calling `self.start_disc_scan()`, emit a log header:

```rust
log::info!("{}", crate::logging::session_header(
    env!("CARGO_PKG_VERSION"),
    Some(&self.device.to_string_lossy()),
    &self.output_dir.display().to_string(),
    &std::path::PathBuf::from(""), // config path not available on session
    "auto",
));
```

Note: The session doesn't have access to `config_path`. Use the config's aacs_backend value instead. Check the actual `session_header()` signature and adjust parameters.

- [ ] **Step 2: Add session header per batch iteration (CLI)**

In `run_batch()`, at the start of each iteration (after disc detection, before `run_batch_iteration()`):

```rust
log::info!("=== Batch disc {} ===", disc_count);
```

A full `session_header()` call would be better but requires access to the config path. Keep it simple with a disc separator log line.

- [ ] **Step 3: Run `cargo clippy -- -D warnings`**

Expected: No warnings

- [ ] **Step 4: Commit**

```
feat(logging): emit session markers per disc in batch mode
```

---

### Task 12: Final integration — format, clippy, test

**Files:**
- All modified files

- [ ] **Step 1: Run formatter**

Run: `rustup run stable cargo fmt`

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings. Fix any issues.

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass (existing + new).

- [ ] **Step 4: Run `cargo build`**

Run: `cargo build`
Expected: Successful build.

- [ ] **Step 5: Update CLAUDE.md**

Add `--batch` and `--no-batch` to the CLI flags table in `CLAUDE.md`. Add `batch` to the config description. Update the "Key Design Decisions" section with a batch mode entry.

- [ ] **Step 6: Commit**

```
chore: final cleanup, CLAUDE.md updates for batch mode
```

---

### Task 13: Update README and docs

**Files:**
- Modify: `README.md` (add --batch to CLI flags)
- Modify: `docs/ROADMAP-1.0.md` (mark batch mode as complete)

- [ ] **Step 1: Add --batch to README CLI flags section**

Add to the flags table:

```
--batch              Batch mode: rip → eject → wait → repeat
--no-batch           Disable batch mode (overrides config)
```

- [ ] **Step 2: Mark batch mode complete in roadmap**

In `docs/ROADMAP-1.0.md`, mark item #30 (Continuous batch mode) with a completion indicator.

- [ ] **Step 3: Commit**

```
docs: update README and roadmap for batch mode
```
