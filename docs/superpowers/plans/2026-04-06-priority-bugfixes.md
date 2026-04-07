# Priority Bug Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the 6 highest-priority bugs found during the full codebase review.

**Architecture:** All fixes are isolated to their respective modules with no cross-fix dependencies. Each fix can be implemented and tested independently. The order below is chosen to minimize context switching (grouping by file).

**Tech Stack:** Rust, clap (derive), crossterm, anyhow, toml

**Spec:** `docs/superpowers/specs/2026-04-06-priority-bugfixes-design.md`

---

## File Map

| File | Changes |
|------|---------|
| `src/session.rs` | Fix 1 (cancel flag), Fix 2 (specials cleared) |
| `src/tui/coordinator.rs` | Fix 3 (tab keybinding), Fix 4 (min_duration_arg), Fix 4 (output) |
| `src/tui/wizard.rs` | Fix 3 (update help text) |
| `src/config.rs` | Fix 4 (min_duration signature), Fix 5 (load_from returns Result) |
| `src/main.rs` | Fix 4 (Option types for min_duration/output), Fix 5 (handle Result from load_from) |
| `src/cli.rs` | Fix 4 (pass Option to min_duration) |

---

### Task 1: Fix cancel flag and specials in `reset_for_rescan`

**Files:**
- Modify: `src/session.rs:249` (cancel flag)
- Modify: `src/session.rs:266` (specials)
- Modify: `src/session.rs:1230-1255` (existing tests)

- [ ] **Step 1: Write failing tests for both bugs**

Add two new test assertions to `src/session.rs` in the `tests` module. The cancel flag test needs to set up a non-default cancel state first:

```rust
#[test]
fn test_reset_for_rescan_cancels_active_rip() {
    let mut session = make_test_session();
    // Simulate an active rip — cancel flag starts as false
    assert!(!session.rip.cancel.load(std::sync::atomic::Ordering::Relaxed));

    session.reset_for_rescan();

    // The old cancel flag should be set to true to stop orphaned remux threads
    // Note: reset_for_rescan replaces rip with RipState::default(), so we need
    // to check the OLD Arc clone. Instead, we verify the store happened by
    // capturing the Arc before reset.
    // For this test, we verify the NEW rip state has a fresh false cancel.
    // The real assertion is that the store(true) happened on the old Arc.
    assert!(!session.rip.cancel.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn test_reset_for_rescan_clears_specials() {
    let mut session = make_test_session();
    session.tmdb.specials = vec![Episode {
        episode_number: 1,
        name: "Special 1".into(),
        runtime: None,
    }];

    session.reset_for_rescan();

    assert!(session.tmdb.specials.is_empty());
}
```

The cancel flag test above won't catch the bug because `reset_for_rescan` replaces `self.rip` entirely. We need to capture the Arc *before* reset to observe the store. Here's the correct cancel test:

```rust
#[test]
fn test_reset_for_rescan_cancels_active_rip() {
    let mut session = make_test_session();
    let old_cancel = session.rip.cancel.clone(); // Arc clone
    assert!(!old_cancel.load(std::sync::atomic::Ordering::Relaxed));

    session.reset_for_rescan();

    // The OLD cancel flag (held by the orphaned remux thread) must be true
    assert!(old_cancel.load(std::sync::atomic::Ordering::Relaxed));
    // The NEW rip state has a fresh cancel flag (false)
    assert!(!session.rip.cancel.load(std::sync::atomic::Ordering::Relaxed));
}
```

Use this second version.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib session::tests::test_reset_for_rescan_cancels_active_rip -- --exact`
Expected: FAIL — `old_cancel` is `false` because the current code stores `false`.

Run: `cargo test --lib session::tests::test_reset_for_rescan_clears_specials -- --exact`
Expected: FAIL — `session.tmdb.specials` is not empty because `reset_for_rescan` doesn't clear it.

- [ ] **Step 3: Fix the cancel flag**

In `src/session.rs`, line 249, change `false` to `true`:

```rust
        self.rip
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
```

- [ ] **Step 4: Fix the specials field**

In `src/session.rs`, after line 266 (after `self.tmdb.episodes = Vec::new();`), add:

```rust
        self.tmdb.specials = Vec::new();
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib session::tests::test_reset_for_rescan -- --exact`
Run: `cargo test --lib session::tests::test_reset_for_rescan_cancels_active_rip -- --exact`
Run: `cargo test --lib session::tests::test_reset_for_rescan_clears_specials -- --exact`
Expected: All PASS.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 7: Suggest commit**

```
fix: cancel active rip and clear stale specials on rescan

reset_for_rescan() stored false to the cancel flag, leaving orphaned
remux threads running and competing for disc I/O. Also failed to clear
tmdb.specials, leaking season 0 data across disc rescans.
```

---

### Task 2: Change tab-switching keybinding to Ctrl+Left / Ctrl+Right

**Files:**
- Modify: `src/tui/coordinator.rs:298-319` (key handler)
- Modify: `src/tui/wizard.rs:211` (help text)

- [ ] **Step 1: Update the key handler in coordinator**

In `src/tui/coordinator.rs`, replace the Tab/BackTab handler block (lines 298-320) with Ctrl+Left/Right:

```rust
        // Ctrl+Left/Right: switch active tab
        if key.code == KeyCode::Right && key.modifiers.contains(KeyModifiers::CONTROL) {
            if !self.sessions.is_empty() {
                let live_count = self.live_session_count();
                if live_count > 0 {
                    self.active_tab = (self.active_tab + 1) % live_count;
                }
            }
            return;
        }
        if key.code == KeyCode::Left && key.modifiers.contains(KeyModifiers::CONTROL) {
            if !self.sessions.is_empty() {
                let live_count = self.live_session_count();
                if live_count > 0 {
                    self.active_tab = if self.active_tab == 0 {
                        live_count - 1
                    } else {
                        self.active_tab - 1
                    };
                }
            }
            return;
        }
```

- [ ] **Step 2: Update TMDb search help text in wizard**

In `src/tui/wizard.rs`, line 211, the comment for Tab in the help hints is already correct ("Tab: Switch to {Movie/TV}"). No change needed — the Tab key will now reach the wizard since the coordinator no longer intercepts it.

Verify the help text at line 211 still says `Tab: Switch to`. If it references anything about tab switching between sessions, update it. Currently it says:
```
"Enter: Search | Down: Results | Tab: Switch to {} | Esc: Skip TMDb | Ctrl+R: Rescan | Ctrl+S: Settings"
```
This is correct as-is.

- [ ] **Step 3: Update the coordinator comment**

In `src/tui/coordinator.rs`, update the comment from `// Tab/Shift+Tab: switch active tab` to `// Ctrl+Left/Right: switch active tab` (already done in step 1 code above).

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All tests pass. No existing tests reference `KeyCode::Tab` in coordinator tests.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 6: Suggest commit**

```
fix: change session tab switching from Tab to Ctrl+Left/Right

Tab was intercepted by the coordinator before reaching the TMDb Search
screen, making the Movie/TV mode toggle (Tab) unreachable. Ctrl+Left/Right
avoids terminal emulator conflicts (Ctrl+Tab) and frees Tab for wizard use.
```

---

### Task 3: Make `--min-duration` an `Option<u32>`

**Files:**
- Modify: `src/main.rs:55-56` (clap arg definition)
- Modify: `src/config.rs:353-358` (resolution method)
- Modify: `src/config.rs:839-861` (existing tests)
- Modify: `src/cli.rs:89,382,597` (call sites)
- Modify: `src/tui/coordinator.rs:91` (session init)
- Modify: `src/session.rs:580,711` (session call sites)

- [ ] **Step 1: Update existing `min_duration` tests to use `Option`**

In `src/config.rs`, update the three existing tests:

```rust
#[test]
fn test_min_duration_default() {
    let config = Config::default();
    assert_eq!(config.min_duration(None), 900);
}

#[test]
fn test_min_duration_config_overrides_default() {
    let config = Config {
        min_duration: Some(600),
        ..Default::default()
    };
    assert_eq!(config.min_duration(None), 600);
}

#[test]
fn test_min_duration_cli_overrides_config() {
    let config = Config {
        min_duration: Some(600),
        ..Default::default()
    };
    assert_eq!(config.min_duration(Some(1200)), 1200);
}
```

- [ ] **Step 2: Add test for the previously-broken case**

Add a new test in `src/config.rs`:

```rust
#[test]
fn test_min_duration_cli_explicit_default_overrides_config() {
    let config = Config {
        min_duration: Some(600),
        ..Default::default()
    };
    // Explicitly passing 900 on CLI should override config's 600
    assert_eq!(config.min_duration(Some(900)), 900);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib config::tests::test_min_duration -- --exact`
Expected: Compilation error — `min_duration` still takes `u32`, not `Option<u32>`.

- [ ] **Step 4: Update the `min_duration` method signature in config.rs**

In `src/config.rs`, replace the `min_duration` method (lines 353-358):

```rust
    pub fn min_duration(&self, cli_min_duration: Option<u32>) -> u32 {
        cli_min_duration.unwrap_or_else(|| self.min_duration.unwrap_or(900))
    }
```

- [ ] **Step 5: Update the clap arg definition in main.rs**

In `src/main.rs`, change line 55-56 from:

```rust
    #[arg(long, default_value = "900")]
    min_duration: u32,
```

to:

```rust
    /// Min seconds for episode detection [default: 900]
    #[arg(long)]
    min_duration: Option<u32>,
```

- [ ] **Step 6: Update CLI call sites in cli.rs**

Three call sites in `src/cli.rs` already pass `args.min_duration` directly. Since the type changed from `u32` to `Option<u32>`, these calls now pass `Option<u32>` which matches the new signature. No code changes needed in `cli.rs` — the types align automatically.

Verify by checking that lines 89, 382, and 597 all use `config.min_duration(args.min_duration)`.

- [ ] **Step 7: Update coordinator session init**

In `src/tui/coordinator.rs`, line 91, change:

```rust
        session.min_duration_arg = Some(self.args.min_duration);
```

to:

```rust
        session.min_duration_arg = self.args.min_duration;
```

Since `self.args.min_duration` is now `Option<u32>`, this assigns directly without wrapping in `Some`.

- [ ] **Step 8: Verify session.rs call sites**

In `src/session.rs`, lines 580 and 711 use `self.min_duration_arg.unwrap_or(900)`. Since `min_duration_arg` is `Option<u32>` and `config.min_duration()` now takes `Option<u32>`, change both from:

```rust
            .min_duration(self.min_duration_arg.unwrap_or(900));
```

to:

```rust
            .min_duration(self.min_duration_arg);
```

Both occurrences (lines 580 and 711).

- [ ] **Step 9: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 10: Suggest commit**

```
fix: make --min-duration Option to distinguish explicit from default

--min-duration used default_value="900" which made it impossible to
distinguish explicit --min-duration 900 from "not passed". Config
values were silently ignored when the user explicitly passed the default.
```

---

### Task 4: Make `--output` an `Option<PathBuf>`

**Files:**
- Modify: `src/main.rs:46-47` (clap arg definition)
- Modify: `src/main.rs:363,406-410` (resolution logic)
- Modify: `src/tui/coordinator.rs:93,771` (session init, settings save)

- [ ] **Step 1: Update the clap arg definition in main.rs**

In `src/main.rs`, change the output arg from:

```rust
    #[arg(short = 'o', long, default_value = ".")]
    output: PathBuf,
```

to:

```rust
    /// Output directory [default: .]
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,
```

- [ ] **Step 2: Update the output resolution in main.rs**

In `src/main.rs`, replace the sentinel check block (lines 405-410):

```rust
    // Apply config defaults to args
    if args.output.as_os_str() == "." {
        if let Some(ref dir) = config.output_dir {
            args.output = PathBuf::from(dir);
        }
    }
```

with:

```rust
    // Resolve output directory: CLI flag > config > current directory
    if args.output.is_none() {
        args.output = config
            .output_dir
            .as_ref()
            .map(|dir| PathBuf::from(dir));
    }
```

This sets `args.output` to `Some(config_dir)` if CLI was `None` and config has a value. If both are `None`, it stays `None` and call sites will default to `"."`.

- [ ] **Step 3: Update the session header log line**

In `src/main.rs`, line 363, change:

```rust
            &args.output.display().to_string(),
```

to:

```rust
            &args.output.as_deref().unwrap_or_else(|| std::path::Path::new(".")).display().to_string(),
```

- [ ] **Step 4: Update coordinator session init**

In `src/tui/coordinator.rs`, line 93, change:

```rust
        session.output_dir = self.args.output.clone();
```

to:

```rust
        session.output_dir = self.args.output.clone().unwrap_or_else(|| PathBuf::from("."));
```

- [ ] **Step 5: Update coordinator settings save**

In `src/tui/coordinator.rs`, line 771, change:

```rust
                    self.args.output = PathBuf::from(dir);
```

to:

```rust
                    self.args.output = Some(PathBuf::from(dir));
```

- [ ] **Step 6: Update all remaining `args.output` usages in main.rs and cli.rs**

`cli::run()` takes `args: &Args` (by reference). `args.output` is used at lines 1133, 1149, 1153, 1494, 1562, 1573. Since `args.output` is now `Option<PathBuf>`, resolve it once near the top of `cli::run()` (after line 313, before the `scan_disc` call), or in the `rip_selected` helper which is where most usages live:

```rust
    let output_dir = args.output.as_deref().unwrap_or_else(|| std::path::Path::new("."));
```

Then replace all `args.output` usages in `cli.rs` with `output_dir`:
- Line 1133: `outfiles.push(output_dir.join(name));`
- Line 1149: `outfiles.push(output_dir.join(&name));`
- Line 1153: `outfiles.push(output_dir.join(name));`
- Line 1494: `vars.insert("dir", output_dir.display().to_string());`
- Line 1562: `vars.insert("dir", output_dir.display().to_string());`
- Line 1573: `output_dir.display()`

Check for any other `args.output` usages in `cli.rs` (e.g., in `scan_disc()` or other helper functions) and apply the same pattern.

- [ ] **Step 7: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 8: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 9: Suggest commit**

```
fix: make --output Option to distinguish explicit from default

--output used default_value="." which made --output . indistinguishable
from "not passed". Config output_dir was silently ignored when the user
explicitly passed the current directory.
```

---

### Task 5: Make `load_from` return `Result<Config>`

**Files:**
- Modify: `src/config.rs:117-126` (function signature and body)
- Modify: `src/main.rs:328` (caller)

- [ ] **Step 1: Write tests for the new error behavior**

Add tests in `src/config.rs` in the `tests` module:

```rust
#[test]
fn test_load_from_missing_file_returns_default() {
    let path = std::path::Path::new("/tmp/bluback_test_nonexistent_config.toml");
    let config = load_from(path).unwrap();
    assert_eq!(config.min_duration, None);
    assert_eq!(config.eject, None);
}

#[test]
fn test_load_from_valid_toml() {
    let dir = std::env::temp_dir().join("bluback_test_valid_config");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "min_duration = 600\n").unwrap();
    let config = load_from(&path).unwrap();
    assert_eq!(config.min_duration, Some(600));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_load_from_invalid_toml_returns_error() {
    let dir = std::env::temp_dir().join("bluback_test_invalid_config");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "min_duration = \"not_a_number\"\n").unwrap();
    let result = load_from(&path);
    assert!(result.is_err());
    let err_msg = format!("{:#}", result.unwrap_err());
    assert!(err_msg.contains("parse"), "error should mention parse: {}", err_msg);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_load_from_malformed_syntax_returns_error() {
    let dir = std::env::temp_dir().join("bluback_test_malformed_config");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "this is not [valid toml\n").unwrap();
    let result = load_from(&path);
    assert!(result.is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::test_load_from -- --exact`
Expected: Compilation error — `load_from` returns `Config`, not `Result<Config>`.

- [ ] **Step 3: Update `load_from` to return `Result<Config>`**

In `src/config.rs`, replace the `load_from` function (lines 117-126):

```rust
pub fn load_from(path: &std::path::Path) -> anyhow::Result<Config> {
    if path.exists() {
        let contents = fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read config file {}: {}", path.display(), e))?;
        let config: Config = toml::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("failed to parse config file {}: {}", path.display(), e))?;
        Ok(config)
    } else {
        Ok(Config::default())
    }
}
```

- [ ] **Step 4: Update the caller in main.rs**

In `src/main.rs`, line 328, change:

```rust
    let config = config::load_from(&config_path);
```

to:

```rust
    let config = config::load_from(&config_path).unwrap_or_else(|e| {
        eprintln!("Error: {:#}", e);
        std::process::exit(2);
    });
```

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 6: Suggest commit**

```
fix: bail on malformed config instead of silently using defaults

load_from() discarded I/O and TOML parse errors via .ok(), silently
reverting the entire config to defaults. Users had no indication their
settings were being ignored. Now returns Result and exits with code 2
on parse failure.
```

---

### Task 6: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass (including the new tests from tasks 1-5).

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Run formatter**

Run: `rustup run stable cargo fmt`
Expected: No changes needed (or apply formatting).

- [ ] **Step 4: Suggest final commit (if formatting changed anything)**

Only if `cargo fmt` made changes:

```
style: apply rustfmt
```
