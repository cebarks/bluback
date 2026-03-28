# Log File Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add structured file logging using the `log` crate + `fern` backend, replacing scattered `eprintln!` calls with unified log macros and adding workflow milestone logging.

**Architecture:** `log` facade for all log macros, `fern` dispatches to two targets — always-on file logging (`debug+`) and configurable stderr output in CLI mode (`warn+` by default). New `src/logging.rs` module handles initialization, rotation, and session headers. Config gains 4 new fields. Existing `eprintln!` calls migrated to `log::warn!/error!`.

**Tech Stack:** `log`, `fern`, `chrono` (for timestamp formatting in log lines)

**Spec:** `docs/superpowers/specs/2026-03-28-v0.10-log-file-support-design.md`

---

### Task 1: Add Dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add log, fern, and chrono to Cargo.toml**

```toml
log = "0.4"
fern = "0.7"
chrono = { version = "0.4", default-features = false, features = ["clock"] }
```

Add these three lines to the `[dependencies]` section, after `ctrlc = "3"`.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add log, fern, chrono for structured logging"
```

---

### Task 2: Add Config Fields for Logging

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add 4 new fields to the Config struct**

In `src/config.rs`, add these fields to the `Config` struct (after the `multi_drive` field):

```rust
    pub log_file: Option<bool>,
    pub log_level: Option<String>,
    pub log_dir: Option<String>,
    pub max_log_files: Option<u32>,
```

- [ ] **Step 2: Add accessor methods to the Config impl block**

Add these methods to `impl Config`, after the `multi_drive_mode()` method:

```rust
    pub fn log_file_enabled(&self) -> bool {
        self.log_file.unwrap_or(true)
    }

    pub fn log_level(&self) -> &str {
        self.log_level.as_deref().unwrap_or("warn")
    }

    pub fn log_dir(&self) -> PathBuf {
        if let Some(ref dir) = self.log_dir {
            return PathBuf::from(dir);
        }
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"));
        home.join(".local").join("share").join("bluback").join("logs")
    }

    pub fn max_log_files(&self) -> u32 {
        self.max_log_files.unwrap_or(10)
    }
```

- [ ] **Step 3: Add new keys to KNOWN_KEYS**

Add `"log_file"`, `"log_level"`, `"log_dir"`, `"max_log_files"` to the `KNOWN_KEYS` array:

```rust
const KNOWN_KEYS: &[&str] = &[
    "tmdb_api_key",
    "preset",
    "tv_format",
    "movie_format",
    "special_format",
    "eject",
    "max_speed",
    "min_duration",
    "show_filtered",
    "output_dir",
    "device",
    "stream_selection",
    "verbose_libbluray",
    "reserve_index_space",
    "overwrite",
    "aacs_backend",
    "multi_drive",
    "log_file",
    "log_level",
    "log_dir",
    "max_log_files",
];
```

- [ ] **Step 4: Add serialization to `to_toml_string()`**

In the `to_toml_string()` method, add these lines before the final `emit_str(&mut out, "tmdb_api_key", ...)` block:

```rust
        out.push('\n');
        emit_bool(&mut out, "log_file", self.log_file, true);
        emit_str(&mut out, "log_level", &self.log_level, "warn");
        emit_str(&mut out, "log_dir", &self.log_dir, "");
        emit_u32(&mut out, "max_log_files", self.max_log_files, 10);
```

- [ ] **Step 5: Add validation for max_log_files**

In `validate_config()`, add after the `reserve_index_space` validation block:

```rust
    if let Some(m) = config.max_log_files {
        if m == 0 {
            warnings.push("max_log_files must be > 0".into());
        }
    }
    if let Some(ref level) = config.log_level {
        if !["error", "warn", "info", "debug", "trace"].contains(&level.as_str()) {
            warnings.push(format!(
                "log_level must be error, warn, info, debug, or trace — got \"{}\"",
                level
            ));
        }
    }
```

- [ ] **Step 6: Write tests for new config fields**

Add these tests to the `#[cfg(test)] mod tests` block in `config.rs`:

```rust
    #[test]
    fn test_parse_log_config() {
        let toml_str = r#"
            log_file = false
            log_level = "debug"
            log_dir = "/tmp/bluback-logs"
            max_log_files = 5
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.log_file, Some(false));
        assert_eq!(config.log_level.as_deref(), Some("debug"));
        assert_eq!(config.log_dir.as_deref(), Some("/tmp/bluback-logs"));
        assert_eq!(config.max_log_files, Some(5));
    }

    #[test]
    fn test_log_config_defaults() {
        let config = Config::default();
        assert!(config.log_file_enabled());
        assert_eq!(config.log_level(), "warn");
        assert_eq!(config.max_log_files(), 10);
        assert!(config.log_dir().to_string_lossy().ends_with("bluback/logs"));
    }

    #[test]
    fn test_validate_max_log_files_zero_warns() {
        let config = Config {
            max_log_files: Some(0),
            ..Default::default()
        };
        let warnings = validate_config(&config);
        assert!(warnings.iter().any(|w| w.contains("max_log_files")));
    }

    #[test]
    fn test_validate_invalid_log_level_warns() {
        let config = Config {
            log_level: Some("verbose".into()),
            ..Default::default()
        };
        let warnings = validate_config(&config);
        assert!(warnings.iter().any(|w| w.contains("log_level")));
    }

    #[test]
    fn test_log_config_serialization_roundtrip() {
        let config = Config {
            log_file: Some(false),
            log_level: Some("debug".into()),
            max_log_files: Some(5),
            ..Default::default()
        };
        let toml_str = config.to_toml_string();
        assert!(toml_str.contains("log_file = false"));
        assert!(toml_str.contains(r#"log_level = "debug""#));
        assert!(toml_str.contains("max_log_files = 5"));
        let reparsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(reparsed.log_file, Some(false));
        assert_eq!(reparsed.log_level.as_deref(), Some("debug"));
        assert_eq!(reparsed.max_log_files, Some(5));
    }
```

- [ ] **Step 7: Run tests to verify**

Run: `cargo test --lib config::tests`
Expected: All tests pass including new ones.

- [ ] **Step 8: Commit**

```bash
git add src/config.rs
git commit -m "feat: add log_file, log_level, log_dir, max_log_files config fields"
```

---

### Task 3: Add CLI Flags for Logging

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add 3 new CLI args to the Args struct**

In `src/main.rs`, add these fields to the `Args` struct, after the `check` field:

```rust
    /// Stderr log verbosity: error, warn, info, debug, trace [default: warn]
    #[arg(long, value_parser = ["error", "warn", "info", "debug", "trace"])]
    log_level: Option<String>,

    /// Disable log file output
    #[arg(long)]
    no_log: bool,

    /// Custom log file path (overrides default location)
    #[arg(long, conflicts_with = "no_log")]
    log_file: Option<PathBuf>,
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: add --log-level, --no-log, --log-file CLI flags"
```

---

### Task 4: Implement `src/logging.rs`

**Files:**
- Create: `src/logging.rs`
- Modify: `src/main.rs` (add `mod logging;`)

- [ ] **Step 1: Create `src/logging.rs` with rotation and init functions**

```rust
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use log::LevelFilter;

use crate::config::Config;

/// Parse a log level string into a LevelFilter.
pub fn parse_level(s: &str) -> LevelFilter {
    match s {
        "error" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "info" => LevelFilter::Info,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Warn,
    }
}

/// Delete oldest log files beyond the max limit.
pub fn rotate_logs(log_dir: &Path, max_files: usize) -> anyhow::Result<()> {
    if !log_dir.exists() {
        return Ok(());
    }

    let mut logs: Vec<_> = fs::read_dir(log_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "log")
        })
        .map(|e| e.path())
        .collect();

    if logs.len() < max_files {
        return Ok(());
    }

    // Sort by filename (chronological since timestamp is prefix)
    logs.sort();

    let to_remove = logs.len() - max_files + 1; // +1 to make room for the new log
    for path in &logs[..to_remove] {
        let _ = fs::remove_file(path);
    }

    Ok(())
}

/// Build the session header written at the top of each log file.
pub fn session_header(
    version: &str,
    device: &str,
    output_dir: &str,
    config_path: &str,
    aacs_backend: &str,
) -> String {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let platform = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    format!(
        "=== bluback {} ===\n\
         Started: {}\n\
         Platform: {}\n\
         Device: {}\n\
         Output: {}\n\
         Config: {}\n\
         AACS backend: {}\n\
         ===\n",
        version, now, platform, device, output_dir, config_path, aacs_backend
    )
}

/// Initialize logging. Creates log directory, rotates old logs, configures fern.
///
/// Returns the log file path if file logging is active, or None if disabled.
pub fn init(
    config: &Config,
    stderr_level: LevelFilter,
    log_file_path: Option<PathBuf>,
    no_log: bool,
    is_tui: bool,
) -> anyhow::Result<Option<PathBuf>> {
    let file_logging = !no_log && config.log_file_enabled();

    let log_path = if file_logging {
        let log_dir = match log_file_path {
            Some(ref p) => p.parent().unwrap_or(Path::new(".")).to_path_buf(),
            None => config.log_dir(),
        };
        fs::create_dir_all(&log_dir)?;
        rotate_logs(&log_dir, config.max_log_files() as usize)?;

        let path = match log_file_path {
            Some(p) => p,
            None => {
                let now = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S");
                log_dir.join(format!("bluback-{}.log", now))
            }
        };
        Some(path)
    } else {
        None
    };

    let mut dispatch = fern::Dispatch::new();

    // File output: debug+ (captures everything useful)
    if let Some(ref path) = log_path {
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;

        let file_dispatch = fern::Dispatch::new()
            .level(LevelFilter::Debug)
            .format(|out, message, record| {
                let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                let target = record.target();
                if target == "bluback" || target.starts_with("bluback::") {
                    out.finish(format_args!(
                        "{} [{}] {}",
                        now,
                        record.level(),
                        message
                    ))
                } else {
                    out.finish(format_args!(
                        "{} [{}] [{}] {}",
                        now,
                        record.level(),
                        target,
                        message
                    ))
                }
            })
            .chain(file);

        dispatch = dispatch.chain(file_dispatch);
    }

    // Stderr output: configurable level, CLI mode only (TUI owns the terminal)
    if !is_tui {
        let stderr_dispatch = fern::Dispatch::new()
            .level(stderr_level)
            .format(|out, message, record| {
                match record.level() {
                    log::Level::Error => out.finish(format_args!("Error: {}", message)),
                    log::Level::Warn => out.finish(format_args!("Warning: {}", message)),
                    _ => out.finish(format_args!("{}", message)),
                }
            })
            .chain(std::io::stderr());

        dispatch = dispatch.chain(stderr_dispatch);
    }

    // If neither file nor stderr, still install a no-op logger so log macros don't panic
    dispatch.apply()
        .map_err(|e| anyhow::anyhow!("Failed to initialize logging: {}", e))?;

    Ok(log_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_level() {
        assert_eq!(parse_level("error"), LevelFilter::Error);
        assert_eq!(parse_level("warn"), LevelFilter::Warn);
        assert_eq!(parse_level("info"), LevelFilter::Info);
        assert_eq!(parse_level("debug"), LevelFilter::Debug);
        assert_eq!(parse_level("trace"), LevelFilter::Trace);
        assert_eq!(parse_level("garbage"), LevelFilter::Warn);
    }

    #[test]
    fn test_rotate_logs_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        rotate_logs(dir.path(), 10).unwrap();
        // No files to rotate — should be a no-op
    }

    #[test]
    fn test_rotate_logs_under_limit() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..3 {
            fs::write(dir.path().join(format!("bluback-2026-03-{:02}T00-00-00.log", i)), "test").unwrap();
        }
        rotate_logs(dir.path(), 10).unwrap();
        let count = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_rotate_logs_at_limit_deletes_oldest() {
        let dir = tempfile::tempdir().unwrap();
        for i in 1..=10 {
            fs::write(
                dir.path().join(format!("bluback-2026-03-{:02}T00-00-00.log", i)),
                "test",
            ).unwrap();
        }
        rotate_logs(dir.path(), 10).unwrap();
        // Should delete oldest to make room for the new one
        let remaining: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(remaining.len(), 9);
        assert!(!remaining.contains(&"bluback-2026-03-01T00-00-00.log".to_string()));
    }

    #[test]
    fn test_rotate_logs_nonexistent_dir() {
        let path = Path::new("/tmp/definitely_does_not_exist_bluback_test");
        rotate_logs(path, 10).unwrap();
        // Should be a no-op, not an error
    }

    #[test]
    fn test_rotate_logs_ignores_non_log_files() {
        let dir = tempfile::tempdir().unwrap();
        for i in 1..=10 {
            fs::write(
                dir.path().join(format!("bluback-2026-03-{:02}T00-00-00.log", i)),
                "test",
            ).unwrap();
        }
        fs::write(dir.path().join("notes.txt"), "keep me").unwrap();
        rotate_logs(dir.path(), 10).unwrap();
        // notes.txt should still exist
        assert!(dir.path().join("notes.txt").exists());
    }

    #[test]
    fn test_session_header_format() {
        let header = session_header("0.10.0", "/dev/sr0", "./output", "~/.config/bluback/config.toml", "libaacs");
        assert!(header.contains("bluback 0.10.0"));
        assert!(header.contains("/dev/sr0"));
        assert!(header.contains("./output"));
        assert!(header.contains("libaacs"));
        assert!(header.starts_with("=== bluback"));
        assert!(header.trim_end().ends_with("==="));
    }
}
```

- [ ] **Step 2: Register the module in `main.rs`**

Add `mod logging;` to the module declarations at the top of `src/main.rs`, after `mod drive_monitor;`:

```rust
mod logging;
```

- [ ] **Step 3: Run tests to verify**

Run: `cargo test --lib logging::tests`
Expected: All 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/logging.rs src/main.rs
git commit -m "feat: add logging module with fern init, rotation, and session header"
```

---

### Task 5: Wire Logging Into main.rs Startup

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Initialize logging in `run_inner()` after config loading**

In `run_inner()`, after the config validation block (the `if config_path.exists()` block, around line 224) and before the `if args.check` block, add:

```rust
    // Initialize logging
    let use_tui = !args.no_tui && atty_stdout();
    let stderr_level = logging::parse_level(
        args.log_level.as_deref()
            .unwrap_or_else(|| config.log_level())
    );
    let log_path = logging::init(
        &config,
        stderr_level,
        args.log_file.clone(),
        args.no_log,
        use_tui,
    )?;

    if let Some(ref path) = log_path {
        let header = logging::session_header(
            env!("CARGO_PKG_VERSION"),
            args.device.as_deref()
                .map(|d| d.to_string_lossy().to_string())
                .as_deref()
                .unwrap_or("auto-detect"),
            &args.output.display().to_string(),
            &config_path.display().to_string(),
            args.aacs_backend.as_deref().unwrap_or("auto"),
        );
        // Write header directly to log file (before log macros, which add timestamps)
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(path) {
            let _ = f.write_all(header.as_bytes());
        }
        log::info!("Log file: {}", path.display());
    }
```

Add `use std::io::Write;` to the existing imports at the top of `main.rs` if not already present.

- [ ] **Step 2: Remove the duplicate `use_tui` binding later in `run_inner()`**

There's an existing `let use_tui = !args.no_tui && atty_stdout();` line later in the function (around line 264). Remove it since we now define `use_tui` earlier.

- [ ] **Step 3: Migrate the top-level error handler in `run()` from `eprintln!` to `log::error!`**

In the `run()` function, change:

```rust
            eprintln!("Error: {:#}", e);
```

to:

```rust
            log::error!("{:#}", e);
```

Note: `log::error!` is dispatched to stderr via fern (which formats it as `Error: ...`), so user-visible behavior is preserved. However, logging may not be initialized if the error occurs before `logging::init()`. In that case, the log macro is a no-op and the error is silently lost. To handle this, also keep a fallback:

```rust
        Err(e) => {
            // log::error may be a no-op if logging hasn't initialized yet
            log::error!("{:#}", e);
            // Fallback for pre-logging errors (config load failures, etc.)
            if log::log_enabled!(log::Level::Error) {
                // Logger is active, error was dispatched
            } else {
                eprintln!("Error: {:#}", e);
            }
            classify_exit_code(&e)
        }
```

Actually, simpler approach — `fern` will have been initialized before most errors can occur. The only pre-logging errors are arg parsing (handled by clap before `run_inner`) and ctrlc handler setup (which uses `.expect()`). So just replace the `eprintln!` with `log::error!` and keep an `eprintln!` fallback for safety:

```rust
        Err(e) => {
            if log::max_level() == LevelFilter::Off {
                eprintln!("Error: {:#}", e);
            }
            log::error!("{:#}", e);
            classify_exit_code(&e)
        }
```

Add `use log::LevelFilter;` to the imports.

- [ ] **Step 4: Migrate config validation warnings**

In `run_inner()`, change the config validation block from:

```rust
            for w in config::validate_raw_toml(&raw) {
                eprintln!("Warning: {} in {}", w, config_path.display());
            }
            for w in config::validate_config(&config) {
                eprintln!("Warning: {}", w);
            }
```

to:

```rust
            for w in config::validate_raw_toml(&raw) {
                log::warn!("{} in {}", w, config_path.display());
            }
            for w in config::validate_config(&config) {
                log::warn!("{}", w);
            }
```

Note: These warnings fire before `logging::init()`, so they'll only reach stderr via the fallback (not the log file). This is acceptable — config validation warnings are rare and shown at startup. If you want them in the log file too, move the logging init before config validation. But config loading must happen first (for log config values), creating a chicken-and-egg situation. The current ordering is fine.

- [ ] **Step 5: Verify it compiles and runs**

Run: `cargo check`
Expected: Compiles without errors.

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire logging init into startup, migrate top-level error/warning output"
```

---

### Task 6: Migrate `eprintln!` Calls in `aacs.rs`

**Files:**
- Modify: `src/aacs.rs`

- [ ] **Step 1: Replace all 3 `eprintln!` calls with log macros**

In `src/aacs.rs`, make these replacements:

Line 125-128 (Libaacs backend, libmmbd symlink detected):
```rust
                    // Before:
                    eprintln!(
                        "Warning: system libaacs.so is a symlink to libmmbd. \
                         Searching for real libaacs..."
                    );
                    // After:
                    log::warn!(
                        "system libaacs.so is a symlink to libmmbd — searching for real libaacs"
                    );
```

Line 139-142 (KEYDB.cfg not found):
```rust
                    // Before:
                    eprintln!(
                        "Warning: KEYDB.cfg not found at {} — AACS decryption may fail.",
                        keydb.display()
                    );
                    // After:
                    log::warn!(
                        "KEYDB.cfg not found at {} — AACS decryption may fail",
                        keydb.display()
                    );
```

Line 150-153 (libmmbd masquerading without makemkvcon):
```rust
                    // Before:
                    eprintln!(
                        "Warning: libmmbd.so is installed as libaacs but makemkvcon was not found. \
                         AACS initialization may hang. Consider setting aacs_backend = \"libaacs\" in config."
                    );
                    // After:
                    log::warn!(
                        "libmmbd.so is installed as libaacs but makemkvcon was not found — \
                         AACS initialization may hang. Consider setting aacs_backend = \"libaacs\" in config"
                    );
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/aacs.rs
git commit -m "refactor: migrate aacs.rs warnings from eprintln to log::warn"
```

---

### Task 7: Migrate `eprintln!` Calls in `tui/coordinator.rs`

**Files:**
- Modify: `src/tui/coordinator.rs`

- [ ] **Step 1: Replace the 2 `eprintln!` calls with log macros**

Line 106 (session thread panic):
```rust
                    // Before:
                    eprintln!("Session thread panicked: {}", msg);
                    // After:
                    log::error!("Session thread panicked: {}", msg);
```

Line 604 (episode overlap):
```rust
                        // Before:
                        eprintln!(
                            "Warning: Episode {} of {} S{:02} is assigned in multiple sessions",
                            ep, show_name, season
                        );
                        // After:
                        log::warn!(
                            "Episode {} of {} S{:02} is assigned in multiple sessions",
                            ep, show_name, season
                        );
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/tui/coordinator.rs
git commit -m "refactor: migrate coordinator.rs eprintln to log::error/warn"
```

---

### Task 8: Migrate `eprintln!` Calls in `media/remux.rs` and `cli.rs`

**Files:**
- Modify: `src/media/remux.rs`
- Modify: `src/cli.rs`

- [ ] **Step 1: Migrate `media/remux.rs` chapter warning**

Find the `eprintln!` around line 155 in `src/media/remux.rs` (chapter injection warning). Replace with `log::warn!`. Read the file first to find the exact text.

- [ ] **Step 2: Migrate `cli.rs` eprintln calls**

In `src/cli.rs`, there are several `eprintln!` calls. Categorize and migrate:

- Line 83: `eprintln!();` — this is a blank line for formatting. Replace with `println!();` (it's user-facing output formatting, not logging).
- Line 126: `eprintln!("Probing streams...");` — status message. Replace with `log::info!("Probing streams...");` and keep a `println!("Probing streams...");` for user output (or leave as-is if the user expects to see it).
- Line 286: Read the context — likely a warning about overwrite/skip. Replace with appropriate `log::warn!`.
- Line 354: `eprintln!();` — formatting blank line. Replace with `println!();`.
- Line 1325: TMDb auto-selection. Replace with `log::info!("TMDb: auto-selected \"{}\" ({})", movie.title, year);`. Keep user-facing output as `println!` too.
- Line 1348: TMDb auto-selection. Replace with `log::info!("TMDb: auto-selected \"{}\"", show.name);`. Keep user-facing output as `println!` too.

Note: Some of these `eprintln!` calls serve double duty (user feedback AND diagnostic). For those, add a `log::info!` and change the `eprintln!` to `println!` (since fern handles stderr now, avoid double-printing to stderr).

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add src/media/remux.rs src/cli.rs
git commit -m "refactor: migrate remaining eprintln calls to log macros"
```

---

### Task 9: Add Workflow Milestone Logging

**Files:**
- Modify: `src/disc.rs`
- Modify: `src/media/probe.rs`
- Modify: `src/workflow.rs`
- Modify: `src/tmdb.rs`
- Modify: `src/media/remux.rs`

This task adds new `log::info!` and `log::debug!` calls at key workflow points. These are *additions*, not migrations of existing output.

- [ ] **Step 1: Add logging to disc detection**

In `src/disc.rs`, add `log::info!` calls at key points:
- After a disc is successfully detected: `log::info!("Disc detected: volume_label={}", label);`
- After mount: `log::info!("Disc mounted at {}", mount_path.display());`
- After unmount: `log::debug!("Disc unmounted");`

Read `disc.rs` to find the exact functions and locations. The detection happens in `detect_optical_drives()` and `get_volume_label()`. Mount/unmount is in `mount_disc()` / `unmount_disc()` or `MountGuard`.

- [ ] **Step 2: Add logging to playlist scanning**

In `src/media/probe.rs`, in the `log_capture_body!` macro, after the thread-local buffer push (around line 311-317), add:

```rust
                // Also route through the log crate for file logging
                log::debug!(target: "ffmpeg", "{}", s.trim());
```

This goes inside the `if let Ok(s) = ...` block, after the `THREAD_LOG_BUFFER.with` call. Note: the macro runs in an `unsafe extern "C"` context, but `log::debug!` is safe to call.

Also add after `scan_playlists_with_progress` returns successfully:
```rust
log::info!("Scan complete: found {} playlists", playlists.len());
```

- [ ] **Step 3: Add logging to TMDb lookups**

In `src/tmdb.rs`, in the `tmdb_get` function, add logging *without* the API key:

```rust
    log::debug!("TMDb request: {}{}", path, if extra_params.is_empty() { String::new() } else {
        format!("?{}", extra_params.iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<_>>().join("&"))
    });
```

This logs the path and query parameters (show name, season number) but NOT the `api_key` parameter.

- [ ] **Step 4: Add logging to remux start/completion**

In `src/media/remux.rs`, at the start of the `remux()` function:
```rust
log::info!("Remux started: playlist={}, output={}", opts.playlist, opts.output.display());
```

At the end, after successful completion:
```rust
log::info!("Remux completed: {}", opts.output.display());
```

On error/cancel:
```rust
log::warn!("Remux cancelled or failed: {}", opts.output.display());
```

- [ ] **Step 5: Add logging to workflow**

In `src/workflow.rs`, in `build_output_filename`:
```rust
log::debug!("Output filename: {}", filename);
```

In `check_overwrite`:
```rust
log::debug!("Overwrite check: {} — {}", path.display(), if exists { "exists" } else { "new" });
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 7: Commit**

```bash
git add src/disc.rs src/media/probe.rs src/workflow.rs src/tmdb.rs src/media/remux.rs
git commit -m "feat: add workflow milestone logging across disc, probe, tmdb, remux, workflow"
```

---

### Task 10: Add Settings Panel Integration

**Files:**
- Modify: `src/types.rs`

- [ ] **Step 1: Add logging settings to `SettingsState::new()`**

In `src/types.rs`, in the `SettingsState::new()` method, add a Logging separator and items. Insert before the TMDb separator (before the `SettingItem::Separator { label: Some("TMDb".into()) }` line):

```rust
            SettingItem::Separator {
                label: Some("Logging".into()),
            },
            SettingItem::Toggle {
                label: "Log to File".into(),
                key: "log_file".into(),
                value: config.log_file.unwrap_or(true),
            },
            SettingItem::Choice {
                label: "Stderr Log Level".into(),
                key: "log_level".into(),
                options: vec![
                    "error".into(),
                    "warn".into(),
                    "info".into(),
                    "debug".into(),
                    "trace".into(),
                ],
                selected: match config.log_level.as_deref() {
                    Some("error") => 0,
                    Some("info") => 2,
                    Some("debug") => 3,
                    Some("trace") => 4,
                    _ => 1, // warn is default
                },
                custom_value: None,
            },
```

- [ ] **Step 2: Add `to_config` mapping for the new settings**

In the `to_config()` method of `SettingsState`, add handling for the new keys. In the `SettingItem::Toggle` match arm, add:

```rust
                    "log_file" if !*value => config.log_file = Some(false),
```

In the `SettingItem::Choice` match arm, add:

```rust
                    "log_level" => {
                        let val = &options[*selected];
                        if val != "warn" {
                            config.log_level = Some(val.clone());
                        }
                    }
```

- [ ] **Step 3: Update the non-separator count in settings tests**

In the test `test_settings_state_construction`, update the non-separator count assertion from 16 to 18 (15 old settings + 2 new + 1 action):

```rust
        assert_eq!(non_separator_count, 18); // 17 settings + 1 action
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib types::tests`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/types.rs
git commit -m "feat: add log_file and log_level to settings panel"
```

---

### Task 11: Add `--check` Logging Validation

**Files:**
- Modify: `src/check.rs`

- [ ] **Step 1: Add log directory check**

In `src/check.rs`, read the file to find where checks are added to the `results` vector. Add a new check for the log directory:

```rust
    // Log directory
    let log_dir = config.log_dir();
    if log_dir.exists() {
        let log_count = std::fs::read_dir(&log_dir)
            .map(|entries| entries.filter_map(|e| e.ok()).filter(|e| {
                e.path().extension().is_some_and(|ext| ext == "log")
            }).count())
            .unwrap_or(0);
        results.push(CheckResult {
            label: "Log directory".into(),
            status: CheckStatus::Pass,
            detail: format!("{} ({} logs)", log_dir.display(), log_count),
        });
    } else {
        results.push(CheckResult {
            label: "Log directory".into(),
            status: CheckStatus::Warn,
            detail: format!("{} (will be created on first run)", log_dir.display()),
        });
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/check.rs
git commit -m "feat: add log directory validation to --check"
```

---

### Task 12: Final Verification and Cleanup

**Files:**
- All modified files

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass (234+ unit tests, 3 integration tests).

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No warnings.

- [ ] **Step 3: Run fmt**

Run: `cargo fmt --check`
Expected: No formatting issues.

- [ ] **Step 4: Verify log file is created on a test run**

Run: `cargo run -- --check`
Expected: Should show the check output. Verify a log file was created at `~/.local/share/bluback/logs/bluback-*.log`.

Run: `ls ~/.local/share/bluback/logs/`
Expected: One log file with today's date.

Run: `cat ~/.local/share/bluback/logs/bluback-*.log`
Expected: Session header followed by log lines.

- [ ] **Step 5: Verify `--no-log` suppresses file creation**

Run: `cargo run -- --check --no-log`
Then check: no new log file was created.

- [ ] **Step 6: Verify `--log-level debug` shows debug output on stderr**

Run: `cargo run -- --check --log-level debug`
Expected: Debug-level messages visible on stderr.

- [ ] **Step 7: Add TODO for future libbluray trace enhancement**

Add this comment at the top of `src/logging.rs`, after the use statements:

```rust
// TODO: Future enhancement — capture raw libbluray BD_DEBUG_MASK output at trace
// level via stderr fd redirection (dup2 to pipe + reader thread). Currently only
// libbluray messages flowing through FFmpeg's av_log callback are captured.
```

- [ ] **Step 8: Commit any remaining changes**

```bash
git add -A
git commit -m "chore: final logging cleanup, add libbluray trace TODO"
```
