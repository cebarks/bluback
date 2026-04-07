# Priority Bug Fixes from Code Review

**Date:** 2026-04-06
**PR scope:** 6 fixes across 5 modules
**Files touched:** `session.rs`, `coordinator.rs`, `config.rs`, `main.rs`, `cli.rs`

## Context

A full codebase bug review identified 30 issues across the codebase. This spec covers the 6 highest-priority fixes, grouped into a single PR because they are all straightforward correctness bugs with low risk of unintended side effects. Shell injection in hooks (bug #1 from the review) is excluded — it changes the hook execution model and warrants its own PR.

## Fix 1: `reset_for_rescan` stores wrong cancel value

**Bug:** `session.rs:249` — `self.rip.cancel.store(false, Ordering::Relaxed)` should store `true`.

**Impact:** When Ctrl+R triggers a rescan during an active rip, the orphaned remux thread continues reading from the disc because it never sees the cancel flag. This causes I/O contention with the new scan and leaves partial output files uncleaned.

**Fix:** Change `false` to `true`. The remux thread's cancel check (`cancel.load(Relaxed)`) will see the flag and exit its read loop, cleaning up the partial file via its existing cancel path.

**Risk:** None. The `RipState` is immediately replaced with `RipState::default()` on line 269, which creates a fresh `AtomicBool(false)` for the next rip. The `true` only affects the old `Arc<AtomicBool>` clone held by the orphaned thread.

## Fix 2: `tmdb.specials` not cleared on rescan

**Bug:** `session.rs:258-266` — `reset_for_rescan()` manually resets each TmdbState field but omits `self.tmdb.specials`.

**Impact:** Season 0 episodes fetched from TMDb for disc A persist when disc B is scanned. Auto-detection heuristics in `detection.rs` (`apply_tmdb_layer`) use these stale specials to classify playlists, potentially marking regular episodes as specials on the new disc.

**Fix:** Add `self.tmdb.specials = Vec::new();` after the existing field resets (after line 266).

**Risk:** None. The specials are re-fetched from TMDb during the new disc's workflow if auto-detection is enabled.

## Fix 3: Tab-switching keybinding changed to Ctrl+Left / Ctrl+Right

**Bug:** `coordinator.rs:298-319` — The coordinator intercepts bare `Tab` / `BackTab` for session tab switching before forwarding keys to the active session. The TMDb Search screen uses `Tab` to toggle Movie/TV mode (`wizard.rs:1034`), making this toggle unreachable. Even with a single session, the coordinator consumes the key (as a no-op `(0+1)%1 = 0`) and returns early.

**Impact:** Users cannot toggle between Movie and TV search modes in the TMDb Search screen.

**Fix:** Replace the `KeyCode::Tab` / `KeyCode::BackTab` handlers with `KeyCode::Left` + `KeyModifiers::CONTROL` and `KeyCode::Right` + `KeyModifiers::CONTROL`. Bare `Tab` falls through to the active session's key handler, restoring the Movie/TV toggle.

**Keybinding rationale:** `Ctrl+Left` / `Ctrl+Right` was chosen over:
- `Ctrl+Tab` / `Ctrl+Shift+Tab` — many terminal emulators (Konsole, GNOME Terminal, iTerm2) intercept these for their own tab switching and never pass them to the application
- `Alt+Left` / `Alt+Right` — less discoverable
- `Ctrl+N` / `Ctrl+P` — could be confused with "new"

Directional arrows are intuitive for "switch between tabs," have reliable terminal support across crossterm-supported terminals, and bluback has no word-navigation in text inputs that would conflict.

**Risk:** Low. Multi-drive/multi-session is a new feature with few users. The keybinding change only affects users with 2+ drives who relied on bare Tab for session switching. Tab bar rendering (`tab_bar.rs`) and help text need updating to reflect the new keybinding.

## Fix 4: `--min-duration` and `--output` default-value ambiguity

**Bug:** Both flags use clap's `default_value`, making it impossible to distinguish "user explicitly passed the default" from "user didn't pass the flag." The config value is silently ignored when the user explicitly passes the default.

### `--min-duration`

**Current (broken):**
```rust
// main.rs
#[arg(long, default_value = "900")]
min_duration: u32,

// config.rs
pub fn min_duration(&self, cli_min_duration: u32) -> u32 {
    if cli_min_duration != 900 {
        return cli_min_duration;
    }
    self.min_duration.unwrap_or(900)
}
```

If config has `min_duration = 600` and user passes `--min-duration 900`, the sentinel check (`!= 900`) fails, and the config value 600 is used instead of the explicit CLI value 900.

**Fixed:**
```rust
// main.rs
#[arg(long)]
min_duration: Option<u32>,

// config.rs
pub fn min_duration(&self, cli_min_duration: Option<u32>) -> u32 {
    cli_min_duration.unwrap_or_else(|| self.min_duration.unwrap_or(900))
}
```

CLI `Some(900)` now correctly overrides config. CLI `None` falls back to config, then to 900.

### `--output`

**Current (broken):**
```rust
// main.rs
#[arg(short = 'o', long, default_value = ".")]
output: PathBuf,

// main.rs resolution
if args.output.as_os_str() == "." {
    if let Some(ref dir) = config.output_dir {
        args.output = PathBuf::from(dir);
    }
}
```

If user passes `--output .` explicitly to mean "current directory, not what config says," the sentinel check (`== "."`) matches and config overrides it.

**Fixed:**
```rust
// main.rs
#[arg(short = 'o', long)]
output: Option<PathBuf>,

// resolution
let output = args.output.unwrap_or_else(|| {
    config.output_dir.as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
});
```

CLI `Some(".")` is used directly. CLI `None` falls back to config, then to `"."`.

### Call site updates

All consumers of `args.min_duration` and `args.output` must be updated to handle the `Option` type. Key call sites:
- `cli.rs`: `config.min_duration(args.min_duration)` (multiple occurrences)
- `coordinator.rs`: session initialization passes `min_duration_arg`
- `main.rs`: output directory resolution, passed to CLI and TUI entry points

**Risk:** Low. The semantic behavior is identical for the common case (flag not passed). Only the edge case of explicitly passing the default value changes behavior, and that change is the intended fix.

## Fix 5: Config parse errors bail instead of silently defaulting

**Bug:** `config.rs:117-126` — `load_from` discards both I/O read errors and TOML parse errors via `.ok()`, silently reverting the entire config to defaults with no user feedback.

**Impact:** A config with a type error (e.g., `min_duration = "abc"`, `eject = 3`, or a syntax error) silently reverts all settings — including `output_dir`, `aacs_backend`, `device` — to defaults. The user has no indication their config was ignored.

**Current:**
```rust
pub fn load_from(path: &std::path::Path) -> Config {
    if path.exists() {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        Config::default()
    }
}
```

**Fixed:**
```rust
pub fn load_from(path: &std::path::Path) -> anyhow::Result<Config> {
    if path.exists() {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file: {}", path.display()))?;
        let config: Config = toml::from_str(&contents)
            .with_context(|| format!("failed to parse config file: {}", path.display()))?;
        Ok(config)
    } else {
        Ok(Config::default())
    }
}
```

**Caller update (`main.rs`):**
```rust
let config = config::load_from(&config_path).unwrap_or_else(|e| {
    eprintln!("Error: {:#}", e);
    std::process::exit(2);
});
```

Exit code 2 matches the existing convention for usage/config errors.

**Behavior change:**
- File doesn't exist: `Ok(Config::default())` (unchanged)
- File exists, valid TOML: `Ok(parsed_config)` (unchanged)
- File exists, unreadable: `Err` with I/O error message (was: silent default)
- File exists, invalid TOML: `Err` with parse error message including line/column (was: silent default)

**Risk:** This is an intentional behavioral change. A config that previously "worked" (by silently falling back to defaults) will now cause a startup error. This is the correct behavior — running with silently wrong settings is worse than refusing to start and telling the user why.

## Testing Strategy

| Fix | Test |
|-----|------|
| 1. Cancel flag | Assert `cancel.load()` is `true` after `reset_for_rescan()` |
| 2. Specials cleared | Assert `tmdb.specials.is_empty()` after `reset_for_rescan()` |
| 3. Keybinding | Update coordinator key-handling tests to use `Ctrl+Left`/`Ctrl+Right`; verify bare `Tab` reaches wizard in single-session mode |
| 4. min_duration | Test `config.min_duration(Some(900))` vs `config.min_duration(None)` with config value set; similar for output resolution |
| 5. Config errors | Test `load_from` returns `Err` on malformed TOML; returns `Ok(default)` on missing file; returns `Ok(parsed)` on valid file |

## Out of Scope

- Shell injection in hooks (`hooks.rs`) — separate PR, changes hook execution model
- Remaining 24 lower-severity bugs from the review — future PRs
- `--verify` / `--no-verify` missing `conflicts_with` — trivial, can be picked up in a future cleanup
