# Post-Rip Hooks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add user-configurable shell commands that execute after individual playlist rips and after session completion.

**Architecture:** New `src/hooks.rs` module owns template expansion and shell execution. Config adds `[post_rip]` and `[post_session]` TOML tables via a shared `HookConfig` struct. CLI and TUI call into `hooks.rs` at their existing completion points. Settings panel gets a new "Hooks" section.

**Tech Stack:** Rust std (`std::process::Command`), existing `log` crate for output capture, existing `toml`/`serde` for config parsing.

**Spec:** `docs/superpowers/specs/2026-03-28-post-rip-hooks-design.md`

---

### Task 1: Add HookConfig struct and Config fields

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write tests for HookConfig TOML parsing**

Add to the `#[cfg(test)] mod tests` block in `src/config.rs`:

```rust
#[test]
fn test_parse_post_rip_config() {
    let toml_str = r#"
        [post_rip]
        command = "notify-send '{filename}'"
        on_failure = true
        blocking = false
        log_output = false
    "#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let hook = config.post_rip.unwrap();
    assert_eq!(hook.command.as_deref(), Some("notify-send '{filename}'"));
    assert_eq!(hook.on_failure, Some(true));
    assert_eq!(hook.blocking, Some(false));
    assert_eq!(hook.log_output, Some(false));
}

#[test]
fn test_parse_post_session_config() {
    let toml_str = r#"
        [post_session]
        command = "echo done"
    "#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let hook = config.post_session.unwrap();
    assert_eq!(hook.command.as_deref(), Some("echo done"));
    assert!(hook.on_failure.is_none());
    assert!(hook.blocking.is_none());
}

#[test]
fn test_parse_missing_hooks_defaults() {
    let config: Config = toml::from_str("").unwrap();
    assert!(config.post_rip.is_none());
    assert!(config.post_session.is_none());
}

#[test]
fn test_hook_config_accessors() {
    let hook = HookConfig {
        command: Some("echo hi".into()),
        on_failure: None,
        blocking: None,
        log_output: None,
    };
    assert!(!hook.on_failure());
    assert!(hook.blocking());
    assert!(hook.log_output());

    let hook2 = HookConfig {
        command: Some("echo hi".into()),
        on_failure: Some(true),
        blocking: Some(false),
        log_output: Some(false),
    };
    assert!(hook2.on_failure());
    assert!(!hook2.blocking());
    assert!(!hook2.log_output());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::test_parse_post_rip -- --no-capture 2>&1 | head -20`
Expected: FAIL — `HookConfig` type doesn't exist yet.

- [ ] **Step 3: Add HookConfig struct and Config fields**

Add the struct after `MetadataConfig` (around line 33 in `src/config.rs`):

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HookConfig {
    pub command: Option<String>,
    pub on_failure: Option<bool>,
    pub blocking: Option<bool>,
    pub log_output: Option<bool>,
}

impl HookConfig {
    pub fn on_failure(&self) -> bool {
        self.on_failure.unwrap_or(false)
    }

    pub fn blocking(&self) -> bool {
        self.blocking.unwrap_or(true)
    }

    pub fn log_output(&self) -> bool {
        self.log_output.unwrap_or(true)
    }
}
```

Add two fields to the `Config` struct (after `metadata`):

```rust
    pub post_rip: Option<HookConfig>,
    pub post_session: Option<HookConfig>,
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config::tests::test_parse_post_rip config::tests::test_parse_post_session config::tests::test_parse_missing_hooks config::tests::test_hook_config_accessors`
Expected: All 4 PASS.

- [ ] **Step 5: Commit**

```
feat: add HookConfig struct and post_rip/post_session config fields
```

---

### Task 2: Add KNOWN_KEYS, serialization, and validation

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write tests for serialization and validation**

Add to tests in `src/config.rs`:

```rust
#[test]
fn test_hook_config_serialization_roundtrip() {
    let config = Config {
        post_rip: Some(HookConfig {
            command: Some("echo '{filename}'".into()),
            on_failure: Some(true),
            blocking: Some(false),
            log_output: Some(false),
        }),
        ..Default::default()
    };
    let toml_str = config.to_toml_string();
    assert!(toml_str.contains("[post_rip]"));
    assert!(toml_str.contains(r#"command = "echo '{filename}'"#));
    assert!(toml_str.contains("on_failure = true"));
    assert!(toml_str.contains("blocking = false"));
    assert!(toml_str.contains("log_output = false"));
    let reparsed: Config = toml::from_str(&toml_str).unwrap();
    let hook = reparsed.post_rip.unwrap();
    assert_eq!(hook.command.as_deref(), Some("echo '{filename}'"));
    assert_eq!(hook.on_failure, Some(true));
}

#[test]
fn test_hook_config_default_serialization_commented() {
    let config = Config::default();
    let toml_str = config.to_toml_string();
    assert!(toml_str.contains("[post_rip]"));
    assert!(toml_str.contains("# command = \"\""));
    assert!(toml_str.contains("[post_session]"));
}

#[test]
fn test_validate_hook_known_keys() {
    let raw = r#"
        [post_rip]
        command = "echo hi"
        on_failure = true
        blocking = false
        log_output = true
    "#;
    let warnings = validate_raw_toml(raw);
    assert!(warnings.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::test_hook_config_serialization config::tests::test_hook_config_default_serialization config::tests::test_validate_hook_known`
Expected: FAIL — `to_toml_string()` doesn't emit hook sections yet.

- [ ] **Step 3: Add KNOWN_KEYS entries**

Add to the `KNOWN_KEYS` array (after `"metadata.tags"`):

```rust
    "post_rip",
    "post_rip.command",
    "post_rip.on_failure",
    "post_rip.blocking",
    "post_rip.log_output",
    "post_session",
    "post_session.command",
    "post_session.on_failure",
    "post_session.blocking",
    "post_session.log_output",
```

- [ ] **Step 4: Add hook serialization to `to_toml_string()`**

Add after the `tmdb_api_key` emit block (end of `to_toml_string()`, before the final `out`):

```rust
        fn emit_hook_section(out: &mut String, section: &str, hook: Option<&HookConfig>) {
            out.push('\n');
            out.push_str(&format!("[{}]\n", section));
            if let Some(h) = hook {
                emit_str(out, "command", &h.command, "");
                emit_bool(out, "on_failure", h.on_failure, false);
                emit_bool(out, "blocking", h.blocking, true);
                emit_bool(out, "log_output", h.log_output, true);
            } else {
                emit_str(out, "command", &None, "");
                emit_bool(out, "on_failure", None, false);
                emit_bool(out, "blocking", None, true);
                emit_bool(out, "log_output", None, true);
            }
        }

        emit_hook_section(&mut out, "post_rip", self.post_rip.as_ref());
        emit_hook_section(&mut out, "post_session", self.post_session.as_ref());
```

Note: `emit_str`, `emit_bool` are inner functions of `to_toml_string()`. The `emit_hook_section` needs to be defined inside `to_toml_string()` too, or the emit calls need to be inlined. Since the existing `emit_*` helpers are inner `fn`s, the simplest approach is to inline the emit calls:

```rust
        out.push('\n');
        out.push_str("[post_rip]\n");
        let pr = self.post_rip.as_ref();
        emit_str(&mut out, "command", &pr.and_then(|h| h.command.clone()), "");
        emit_bool(&mut out, "on_failure", pr.and_then(|h| h.on_failure), false);
        emit_bool(&mut out, "blocking", pr.and_then(|h| h.blocking), true);
        emit_bool(&mut out, "log_output", pr.and_then(|h| h.log_output), true);

        out.push('\n');
        out.push_str("[post_session]\n");
        let ps = self.post_session.as_ref();
        emit_str(&mut out, "command", &ps.and_then(|h| h.command.clone()), "");
        emit_bool(&mut out, "on_failure", ps.and_then(|h| h.on_failure), false);
        emit_bool(&mut out, "blocking", ps.and_then(|h| h.blocking), true);
        emit_bool(&mut out, "log_output", ps.and_then(|h| h.log_output), true);
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib config::tests`
Expected: All config tests PASS (including new ones and existing roundtrip tests).

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass. The new `post_rip`/`post_session` fields default to `None` so existing config parsing is unaffected. Check that `test_save_default_config_all_commented` still passes — new lines with `#` prefix and `[section]` headers are acceptable.

- [ ] **Step 7: Commit**

```
feat: add hook config serialization, KNOWN_KEYS, and validation
```

---

### Task 3: Add `--no-hooks` CLI flag

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add the flag to Args**

Add after the `log_file` field (around line 147 in `src/main.rs`):

```rust
    /// Disable post-rip and post-session hooks for this run
    #[arg(long)]
    no_hooks: bool,
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles. The field exists but isn't read yet.

- [ ] **Step 3: Commit**

```
feat: add --no-hooks CLI flag
```

---

### Task 4: Create hooks module with template expansion

**Files:**
- Create: `src/hooks.rs`
- Modify: `src/main.rs` (add `mod hooks`)

- [ ] **Step 1: Write template expansion tests**

Create `src/hooks.rs` with tests first:

```rust
use std::collections::HashMap;

/// Expand `{var}` placeholders in a command string.
/// Known variables are replaced with their values. Empty values become "".
/// Unknown placeholders are left as-is for forward compatibility.
///
/// TODO(debt): Research shell injection risk from template substitution.
/// Filenames with shell metacharacters ($, `, ", ;, etc.) in template
/// variables could be exploited when expanded into an sh -c command string.
/// Future fix: switch to environment variables or add shell escaping.
pub fn expand_template(command: &str, vars: &HashMap<&str, String>) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_known_vars() {
        let mut vars = HashMap::new();
        vars.insert("filename", "S01E03_Title.mkv".into());
        vars.insert("title", "Breaking Bad".into());
        let result = expand_template("echo '{filename}' '{title}'", &vars);
        assert_eq!(result, "echo 'S01E03_Title.mkv' 'Breaking Bad'");
    }

    #[test]
    fn test_expand_unknown_vars_left_as_is() {
        let vars = HashMap::new();
        let result = expand_template("echo {unknown}", &vars);
        assert_eq!(result, "echo {unknown}");
    }

    #[test]
    fn test_expand_empty_var_becomes_empty_string() {
        let mut vars = HashMap::new();
        vars.insert("season", String::new());
        let result = expand_template("s{season}e{episode}", &vars);
        assert_eq!(result, "se{episode}");
    }

    #[test]
    fn test_expand_no_placeholders() {
        let vars = HashMap::new();
        let result = expand_template("echo hello", &vars);
        assert_eq!(result, "echo hello");
    }

    #[test]
    fn test_expand_mixed_known_and_unknown() {
        let mut vars = HashMap::new();
        vars.insert("file", "/output/test.mkv".into());
        let result = expand_template("{file} {future_var}", &vars);
        assert_eq!(result, "/output/test.mkv {future_var}");
    }

    #[test]
    fn test_expand_repeated_var() {
        let mut vars = HashMap::new();
        vars.insert("title", "Test".into());
        let result = expand_template("{title} - {title}", &vars);
        assert_eq!(result, "Test - Test");
    }

    #[test]
    fn test_expand_adjacent_braces() {
        let vars = HashMap::new();
        let result = expand_template("{{not_a_var}}", &vars);
        // Not a valid placeholder pattern — left as-is
        assert_eq!(result, "{{not_a_var}}");
    }
}
```

- [ ] **Step 2: Add `mod hooks` to main.rs**

Add after `mod logging;` in `src/main.rs`:

```rust
mod hooks;
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib hooks::tests`
Expected: FAIL — `todo!()` panics.

- [ ] **Step 4: Implement expand_template**

Replace the `todo!()` body:

```rust
pub fn expand_template(command: &str, vars: &HashMap<&str, String>) -> String {
    let mut result = String::with_capacity(command.len());
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            // Try to parse a placeholder name (lowercase ascii + underscore)
            let mut name = String::new();
            let mut found_close = false;
            for next in chars.by_ref() {
                if next == '}' {
                    found_close = true;
                    break;
                }
                if next.is_ascii_lowercase() || next == '_' {
                    name.push(next);
                } else {
                    // Not a valid placeholder — emit what we consumed
                    result.push('{');
                    result.push_str(&name);
                    result.push(next);
                    name.clear();
                    break;
                }
            }
            if found_close && !name.is_empty() {
                if let Some(val) = vars.get(name.as_str()) {
                    result.push_str(val);
                } else {
                    // Unknown var — leave as-is
                    result.push('{');
                    result.push_str(&name);
                    result.push('}');
                }
            } else if !name.is_empty() {
                // We broke out of the loop without finding '}' — already emitted
            } else if found_close {
                // Empty braces: {}
                result.push('{');
                result.push('}');
            } else {
                result.push('{');
            }
        } else {
            result.push(ch);
        }
    }

    result
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib hooks::tests`
Expected: All 7 PASS.

- [ ] **Step 6: Commit**

```
feat: add hooks module with template expansion
```

---

### Task 5: Add hook execution logic

**Files:**
- Modify: `src/hooks.rs`

- [ ] **Step 1: Add the execution and public API functions**

Add above the `#[cfg(test)]` block in `src/hooks.rs`:

```rust
use crate::config::Config;

/// Run a post-rip hook if configured and appropriate.
pub fn run_post_rip(config: &Config, vars: &HashMap<&str, String>, no_hooks: bool) {
    if no_hooks {
        log::debug!("Post-rip hook skipped (--no-hooks)");
        return;
    }
    let hook = match config.post_rip.as_ref() {
        Some(h) => h,
        None => return,
    };
    let command = match hook.command.as_deref() {
        Some(cmd) if !cmd.is_empty() => cmd,
        _ => return,
    };

    let is_success = vars.get("status").map(|s| s.as_str()) == Some("success");
    if !is_success && !hook.on_failure() {
        log::debug!("Post-rip hook skipped (status=failed, on_failure=false)");
        return;
    }

    let expanded = expand_template(command, vars);
    execute_hook(&expanded, hook.blocking(), hook.log_output(), "post_rip");
}

/// Run a post-session hook if configured and appropriate.
pub fn run_post_session(config: &Config, vars: &HashMap<&str, String>, no_hooks: bool) {
    if no_hooks {
        log::debug!("Post-session hook skipped (--no-hooks)");
        return;
    }
    let hook = match config.post_session.as_ref() {
        Some(h) => h,
        None => return,
    };
    let command = match hook.command.as_deref() {
        Some(cmd) if !cmd.is_empty() => cmd,
        _ => return,
    };

    let failed_count: u32 = vars
        .get("failed")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let has_failures = failed_count > 0;
    if has_failures && !hook.on_failure() {
        log::debug!("Post-session hook skipped (failures detected, on_failure=false)");
        return;
    }

    let expanded = expand_template(command, vars);
    execute_hook(&expanded, hook.blocking(), hook.log_output(), "post_session");
}

fn execute_hook(command: &str, blocking: bool, log_output: bool, label: &str) {
    log::info!("[{}] Running: {}", label, command);

    if blocking {
        execute_blocking(command, log_output, label);
    } else {
        let cmd = command.to_string();
        let lbl = label.to_string();
        std::thread::spawn(move || {
            execute_blocking(&cmd, log_output, &lbl);
        });
    }
}

fn execute_blocking(command: &str, log_output: bool, label: &str) {
    let result = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .output();

    match result {
        Ok(output) => {
            if log_output {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stdout.trim().is_empty() {
                    log::info!("[{}] stdout: {}", label, stdout.trim());
                }
                if !stderr.trim().is_empty() {
                    log::warn!("[{}] stderr: {}", label, stderr.trim());
                }
            }
            if !output.status.success() {
                let code = output
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".into());
                log::warn!("[{}] Hook exited with code {}", label, code);
            }
        }
        Err(e) => {
            log::warn!("[{}] Failed to execute hook: {}", label, e);
        }
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles.

- [ ] **Step 3: Commit**

```
feat: add hook execution with blocking/non-blocking support
```

---

### Task 6: Integrate hooks into CLI rip path

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Pass `no_hooks` through to `rip_selected`**

The `rip_selected` function signature at line 921 needs `no_hooks: bool` added. Update the signature:

```rust
fn rip_selected(
    args: &Args,
    config: &crate::config::Config,
    device: &str,
    episodes_pl: &[Playlist],
    selected: &[usize],
    outfiles: &[PathBuf],
    metadata_per_playlist: &[Option<crate::types::MkvMetadata>],
    no_hooks: bool,
) -> anyhow::Result<()> {
```

Update the call site in `run()` (around line 346) to pass `args.no_hooks`:

```rust
    rip_selected(
        args,
        config,
        &device,
        &episodes_pl,
        &selected,
        &outfiles,
        &metadata_per_playlist,
        args.no_hooks,
    )
```

- [ ] **Step 2: Add post-rip hook call after each playlist completes**

In `rip_selected`, we need access to context for template variables. Add these local variables before the rip loop (before the `for (i, &idx)` loop around line 969):

```rust
    let movie_mode = args.movie;
    let mode_str = if movie_mode { "movie" } else { "tv" };
```

Then after the `match result` block (after the `Ok(chapters_added)` success arm around line 1067, and after the `Err(e)` failure arm around line 1082), add a hook call. The cleanest way: capture status/error after the match, then call the hook. Replace the `match result { ... }` block (lines 1058-1084) with:

```rust
        let (hook_status, hook_error, hook_size, hook_chapters) = match result {
            Ok(chapters_added) => {
                let final_size = std::fs::metadata(outfile)?.len();
                if !is_tty {
                    println!("  [{}] 100% {} — done", pl.num, format_size(final_size));
                }
                println!("Done: {} ({})", filename, format_size(final_size));
                if chapters_added > 0 {
                    println!("  Added {} chapter markers", chapters_added);
                }
                ("success", String::new(), final_size, chapters_added)
            }
            Err(crate::media::MediaError::Cancelled) => {
                if outfile.exists() {
                    let _ = std::fs::remove_file(outfile);
                }
                println!("Cancelled — removed partial file {}", filename);
                // Post-rip hook before break
                let mut vars = std::collections::HashMap::new();
                vars.insert("status", "failed".to_string());
                vars.insert("error", "Cancelled".to_string());
                vars.insert("file", outfile.display().to_string());
                vars.insert("filename", filename.to_string());
                vars.insert("dir", args.output.display().to_string());
                vars.insert("size", "0".to_string());
                vars.insert("chapters", "0".to_string());
                vars.insert("playlist", pl.num.clone());
                vars.insert("device", device.to_string());
                vars.insert("mode", mode_str.to_string());
                vars.insert("label", label.clone());
                vars.insert("title", tmdb_ctx.show_name.as_deref().unwrap_or("").to_string());
                vars.insert("season", tmdb_ctx.season_num.map(|n| n.to_string()).unwrap_or_default());
                let episodes = tmdb_ctx.episode_assignments.get(&pl.num);
                vars.insert("episode", episodes.and_then(|e| e.first()).map(|e| e.episode_number.to_string()).unwrap_or_default());
                vars.insert("episode_name", episodes.and_then(|e| e.first()).map(|e| e.name.clone()).unwrap_or_default());
                crate::hooks::run_post_rip(config, &vars, no_hooks);
                break;
            }
            Err(e) => {
                let err_msg = e.to_string();
                if outfile.exists() {
                    let _ = std::fs::remove_file(outfile);
                }
                println!("Error: {} — removed partial file {}", err_msg, filename);
                had_failure = true;
                ("failed", err_msg, 0u64, 0usize)
            }
        };

        // Post-rip hook (success and non-cancel failure paths)
        if hook_status != "unused" {
            let episodes = tmdb_ctx.episode_assignments.get(&pl.num);
            let mut vars = std::collections::HashMap::new();
            vars.insert("file", outfile.display().to_string());
            vars.insert("filename", filename.to_string());
            vars.insert("dir", args.output.display().to_string());
            vars.insert("size", hook_size.to_string());
            vars.insert("chapters", hook_chapters.to_string());
            vars.insert("title", tmdb_ctx.show_name.as_deref().unwrap_or("").to_string());
            vars.insert("season", tmdb_ctx.season_num.map(|n| n.to_string()).unwrap_or_default());
            vars.insert("episode", episodes.and_then(|e| e.first()).map(|e| e.episode_number.to_string()).unwrap_or_default());
            vars.insert("episode_name", episodes.and_then(|e| e.first()).map(|e| e.name.clone()).unwrap_or_default());
            vars.insert("playlist", pl.num.clone());
            vars.insert("label", label.clone());
            vars.insert("mode", mode_str.to_string());
            vars.insert("device", device.to_string());
            vars.insert("status", hook_status.to_string());
            vars.insert("error", hook_error);
            crate::hooks::run_post_rip(config, &vars, no_hooks);
        }
```

Wait — this duplicates the vars setup for the cancel path. A cleaner approach: always build vars after the match (the cancel case breaks, so we handle it inline before the break). Let me restructure:

Replace the entire `match result { ... }` block (lines 1058-1084) with:

```rust
        let (hook_status, hook_error, hook_size, hook_chapters) = match result {
            Ok(chapters_added) => {
                let final_size = std::fs::metadata(outfile)?.len();
                if !is_tty {
                    println!("  [{}] 100% {} — done", pl.num, format_size(final_size));
                }
                println!("Done: {} ({})", filename, format_size(final_size));
                if chapters_added > 0 {
                    println!("  Added {} chapter markers", chapters_added);
                }
                ("success", String::new(), final_size, chapters_added)
            }
            Err(crate::media::MediaError::Cancelled) => {
                if outfile.exists() {
                    let _ = std::fs::remove_file(outfile);
                }
                println!("Cancelled — removed partial file {}", filename);
                ("failed", "Cancelled".to_string(), 0, 0)
            }
            Err(e) => {
                let err_msg = e.to_string();
                if outfile.exists() {
                    let _ = std::fs::remove_file(outfile);
                }
                println!("Error: {} — removed partial file {}", err_msg, filename);
                had_failure = true;
                ("failed", err_msg, 0, 0)
            }
        };

        // Post-rip hook
        {
            let episodes = tmdb_ctx.episode_assignments.get(&pl.num);
            let mut vars = std::collections::HashMap::new();
            vars.insert("file", outfile.display().to_string());
            vars.insert("filename", filename.to_string());
            vars.insert("dir", args.output.display().to_string());
            vars.insert("size", hook_size.to_string());
            vars.insert("chapters", hook_chapters.to_string());
            vars.insert("title", tmdb_ctx.show_name.as_deref().unwrap_or("").to_string());
            vars.insert("season", tmdb_ctx.season_num.map(|n| n.to_string()).unwrap_or_default());
            vars.insert("episode", episodes.and_then(|e| e.first()).map(|e| e.episode_number.to_string()).unwrap_or_default());
            vars.insert("episode_name", episodes.and_then(|e| e.first()).map(|e| e.name.clone()).unwrap_or_default());
            vars.insert("playlist", pl.num.clone());
            vars.insert("label", label.clone());
            vars.insert("mode", mode_str.to_string());
            vars.insert("device", device.to_string());
            vars.insert("status", hook_status.to_string());
            vars.insert("error", hook_error);
            crate::hooks::run_post_rip(config, &vars, no_hooks);
        }

        if hook_status == "failed" && matches!(result, Err(crate::media::MediaError::Cancelled)) {
            break;
        }
```

Actually, `result` has been moved into the match. We need a different approach for the cancel-break. Use a flag:

```rust
        let was_cancelled = matches!(&result, Err(crate::media::MediaError::Cancelled));

        let (hook_status, hook_error, hook_size, hook_chapters) = match result {
            // ... same arms as above but without the `break` in Cancelled
        };

        // Post-rip hook
        {
            // ... vars setup as above ...
            crate::hooks::run_post_rip(config, &vars, no_hooks);
        }

        if was_cancelled {
            break;
        }
```

This is the cleanest structure. The `was_cancelled` check must come before the match borrows `result`. Since `matches!` only borrows:

```rust
        if is_tty {
            println!(); // newline after \r progress
        }

        let was_cancelled = matches!(&result, Err(crate::media::MediaError::Cancelled));

        let (hook_status, hook_error, hook_size, hook_chapters) = match result {
            Ok(chapters_added) => {
                let final_size = std::fs::metadata(outfile)?.len();
                if !is_tty {
                    println!("  [{}] 100% {} — done", pl.num, format_size(final_size));
                }
                println!("Done: {} ({})", filename, format_size(final_size));
                if chapters_added > 0 {
                    println!("  Added {} chapter markers", chapters_added);
                }
                ("success", String::new(), final_size, chapters_added)
            }
            Err(crate::media::MediaError::Cancelled) => {
                if outfile.exists() {
                    let _ = std::fs::remove_file(outfile);
                }
                println!("Cancelled — removed partial file {}", filename);
                ("failed", "Cancelled".to_string(), 0, 0)
            }
            Err(e) => {
                let err_msg = e.to_string();
                if outfile.exists() {
                    let _ = std::fs::remove_file(outfile);
                }
                println!("Error: {} — removed partial file {}", err_msg, filename);
                had_failure = true;
                ("failed", err_msg, 0, 0)
            }
        };

        {
            let episodes = tmdb_ctx.episode_assignments.get(&pl.num);
            let mut vars = std::collections::HashMap::new();
            vars.insert("file", outfile.display().to_string());
            vars.insert("filename", filename.to_string());
            vars.insert("dir", args.output.display().to_string());
            vars.insert("size", hook_size.to_string());
            vars.insert("chapters", hook_chapters.to_string());
            vars.insert("title", tmdb_ctx.show_name.as_deref().unwrap_or("").to_string());
            vars.insert("season", tmdb_ctx.season_num.map(|n| n.to_string()).unwrap_or_default());
            vars.insert("episode", episodes.and_then(|e| e.first()).map(|e| e.episode_number.to_string()).unwrap_or_default());
            vars.insert("episode_name", episodes.and_then(|e| e.first()).map(|e| e.name.clone()).unwrap_or_default());
            vars.insert("playlist", pl.num.clone());
            vars.insert("label", label.clone());
            vars.insert("mode", mode_str.to_string());
            vars.insert("device", device.to_string());
            vars.insert("status", hook_status.to_string());
            vars.insert("error", hook_error);
            crate::hooks::run_post_rip(config, &vars, no_hooks);
        }

        if was_cancelled {
            break;
        }
```

- [ ] **Step 3: Add `label` and `tmdb_ctx` to `rip_selected` parameters**

The `rip_selected` function needs access to `label` and `tmdb_ctx` for the hook vars. Update the signature to add them:

```rust
fn rip_selected(
    args: &Args,
    config: &crate::config::Config,
    device: &str,
    episodes_pl: &[Playlist],
    selected: &[usize],
    outfiles: &[PathBuf],
    metadata_per_playlist: &[Option<crate::types::MkvMetadata>],
    no_hooks: bool,
    label: &str,
    tmdb_ctx: &TmdbContext,
) -> anyhow::Result<()> {
```

Update the call site in `run()`:

```rust
    rip_selected(
        args,
        config,
        &device,
        &episodes_pl,
        &selected,
        &outfiles,
        &metadata_per_playlist,
        args.no_hooks,
        &label,
        &tmdb_ctx,
    )
```

- [ ] **Step 4: Add post-session hook call after the rip loop**

After the rip loop and mount guard cleanup, before the "All done!" println (around line 1087-1095):

```rust
    // Post-session hook
    {
        let succeeded = selected.len() - if had_failure { 1 } else { 0 }; // approximate
        let mut vars = std::collections::HashMap::new();
        vars.insert("title", tmdb_ctx.show_name.as_deref().unwrap_or("").to_string());
        vars.insert("season", tmdb_ctx.season_num.map(|n| n.to_string()).unwrap_or_default());
        vars.insert("label", label.to_string());
        vars.insert("device", device.to_string());
        vars.insert("mode", mode_str.to_string());
        vars.insert("dir", args.output.display().to_string());
        vars.insert("total", selected.len().to_string());
        vars.insert("succeeded", succeeded.to_string());
        vars.insert("failed", if had_failure { "1" } else { "0" }.to_string());
        vars.insert("skipped", "0".to_string()); // CLI doesn't track skipped count separately
        crate::hooks::run_post_session(config, &vars, no_hooks);
    }
```

Actually, we should count success/failure/skipped properly. Track counts in the rip loop. Add before the loop:

```rust
    let mut success_count = 0u32;
    let mut fail_count = 0u32;
    let mut skip_count = 0u32;
```

In the success arm: `success_count += 1;`
In the skip path (the `OverwriteAction::Skip` continue): `skip_count += 1;`
In the error arm: `fail_count += 1;` (replace `had_failure = true`)
In the cancel arm: `fail_count += 1;`

Then use these in the post-session hook vars and replace `had_failure` with `fail_count > 0` for the eject check.

- [ ] **Step 5: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 7: Commit**

```
feat: integrate post-rip and post-session hooks into CLI path
```

---

### Task 7: Integrate hooks into TUI rip path

**Files:**
- Modify: `src/tui/dashboard.rs`
- Modify: `src/session.rs`

- [ ] **Step 1: Add `no_hooks` to DriveSession**

In `src/session.rs`, add to the `DriveSession` struct (after `no_metadata`):

```rust
    pub no_hooks: bool,
```

In the `DriveSession::new()` constructor, initialize it:

```rust
            no_hooks: false,
```

- [ ] **Step 2: Set `no_hooks` from Args in the coordinator**

Find where `DriveSession` fields are set from `Args` in the coordinator or wherever sessions are spawned. Search for `no_metadata` assignments to find the pattern, then add `no_hooks` alongside it.

- [ ] **Step 3: Add post-rip hook in `poll_active_job_session`**

In `src/tui/dashboard.rs`, in the `poll_active_job_session` function, add hook calls when a job completes. After the `Disconnected` arm sets `PlaylistStatus::Done(file_size)` (around line 532), and after the error arms set `PlaylistStatus::Failed`, add a helper call.

Add a helper function to `dashboard.rs` to build hook vars from session state:

```rust
fn build_post_rip_vars(session: &crate::session::DriveSession, job_idx: usize, status: &str, error: &str) -> std::collections::HashMap<&'static str, String> {
    let job = &session.rip.jobs[job_idx];
    let outfile = session.output_dir.join(&job.filename);
    let file_size = match &job.status {
        PlaylistStatus::Done(size) => *size,
        _ => 0,
    };

    let mut vars = std::collections::HashMap::new();
    vars.insert("file", outfile.display().to_string());
    vars.insert("filename", job.filename.clone());
    vars.insert("dir", session.output_dir.display().to_string());
    vars.insert("size", file_size.to_string());
    vars.insert("chapters", "0".to_string()); // chapter count not tracked in TUI RipJob
    vars.insert("title", if session.tmdb.movie_mode {
        session.tmdb.movie_results.get(session.tmdb.selected_movie.unwrap_or(0))
            .map(|m| m.title.clone()).unwrap_or_default()
    } else {
        session.tmdb.show_name.clone()
    });
    vars.insert("season", session.wizard.season_num.map(|n| n.to_string()).unwrap_or_default());
    vars.insert("episode", job.episode.first().map(|e| e.episode_number.to_string()).unwrap_or_default());
    vars.insert("episode_name", job.episode.first().map(|e| e.name.clone()).unwrap_or_default());
    vars.insert("playlist", job.playlist.num.clone());
    vars.insert("label", session.disc.label.clone());
    vars.insert("mode", if session.tmdb.movie_mode { "movie" } else { "tv" }.to_string());
    vars.insert("device", session.device.display().to_string());
    vars.insert("status", status.to_string());
    vars.insert("error", error.to_string());
    vars
}
```

Then add calls in `poll_active_job_session`:

After the `Disconnected` arm (success, around line 532-534):
```rust
                    let vars = build_post_rip_vars(session, idx, "success", "");
                    crate::hooks::run_post_rip(&session.config, &vars, session.no_hooks);
```

After the `Ok(Err(MediaError::Cancelled))` arm (around line 510-512):
```rust
                    let vars = build_post_rip_vars(session, idx, "failed", "Cancelled");
                    crate::hooks::run_post_rip(&session.config, &vars, session.no_hooks);
```

After the `Ok(Err(e))` arm (around line 520):
```rust
                    let err_msg = e.to_string();
                    // ... existing code ...
                    let vars = build_post_rip_vars(session, idx, "failed", &err_msg);
                    crate::hooks::run_post_rip(&session.config, &vars, session.no_hooks);
```

Note: Capture `e.to_string()` before `e` is consumed by `PlaylistStatus::Failed(e.to_string())`.

- [ ] **Step 4: Add post-session hook in `check_all_done_session`**

In `check_all_done_session`, after the disc unmount and before `session.start_disc_scan()`:

```rust
        // Post-session hook
        {
            let (succeeded, failed, skipped) = session.rip.jobs.iter().fold((0u32, 0u32, 0u32), |(s, f, sk), j| {
                match j.status {
                    PlaylistStatus::Done(_) => (s + 1, f, sk),
                    PlaylistStatus::Failed(_) => (s, f + 1, sk),
                    PlaylistStatus::Skipped(_) => (s, f, sk + 1),
                    _ => (s, f, sk),
                }
            });
            let total = succeeded + failed + skipped;
            let mut vars = std::collections::HashMap::new();
            vars.insert("title", if session.tmdb.movie_mode {
                session.tmdb.movie_results.get(session.tmdb.selected_movie.unwrap_or(0))
                    .map(|m| m.title.clone()).unwrap_or_default()
            } else {
                session.tmdb.show_name.clone()
            });
            vars.insert("season", session.wizard.season_num.map(|n| n.to_string()).unwrap_or_default());
            vars.insert("label", session.disc.label.clone());
            vars.insert("device", session.device.display().to_string());
            vars.insert("mode", if session.tmdb.movie_mode { "movie" } else { "tv" }.to_string());
            vars.insert("dir", session.output_dir.display().to_string());
            vars.insert("total", total.to_string());
            vars.insert("succeeded", succeeded.to_string());
            vars.insert("failed", failed.to_string());
            vars.insert("skipped", skipped.to_string());
            crate::hooks::run_post_session(&session.config, &vars, session.no_hooks);
        }
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 7: Commit**

```
feat: integrate post-rip and post-session hooks into TUI path
```

---

### Task 8: Add hooks to settings panel

**Files:**
- Modify: `src/types.rs`

- [ ] **Step 1: Write test for settings panel hook items**

Add to `types::tests`:

```rust
#[test]
fn test_settings_has_hooks_section() {
    let config = crate::config::Config::default();
    let state = SettingsState::from_config(&config);
    let has_hooks_separator = state.items.iter().any(|i| {
        matches!(i, SettingItem::Separator { label: Some(l) } if l == "Hooks")
    });
    assert!(has_hooks_separator);
}

#[test]
fn test_settings_hook_items_from_config() {
    let config = crate::config::Config {
        post_rip: Some(crate::config::HookConfig {
            command: Some("echo test".into()),
            on_failure: Some(true),
            blocking: Some(false),
            log_output: Some(false),
        }),
        ..Default::default()
    };
    let state = SettingsState::from_config(&config);
    let cmd = state.items.iter().find(|i| {
        matches!(i, SettingItem::Text { key, .. } if key == "post_rip.command")
    });
    assert!(matches!(cmd, Some(SettingItem::Text { value, .. }) if value == "echo test"));
    let on_fail = state.items.iter().find(|i| {
        matches!(i, SettingItem::Toggle { key, .. } if key == "post_rip.on_failure")
    });
    assert!(matches!(on_fail, Some(SettingItem::Toggle { value: true, .. })));
}

#[test]
fn test_settings_hook_to_config_roundtrip() {
    let config = crate::config::Config {
        post_rip: Some(crate::config::HookConfig {
            command: Some("echo test".into()),
            on_failure: Some(true),
            blocking: Some(false),
            log_output: None,
        }),
        ..Default::default()
    };
    let state = SettingsState::from_config(&config);
    let restored = state.to_config();
    let hook = restored.post_rip.unwrap();
    assert_eq!(hook.command.as_deref(), Some("echo test"));
    assert_eq!(hook.on_failure, Some(true));
    assert_eq!(hook.blocking, Some(false));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib types::tests::test_settings_has_hooks`
Expected: FAIL — no Hooks separator exists yet.

- [ ] **Step 3: Add hook items to settings panel**

In `SettingsState::from_config_with_drives()`, add after the Metadata toggle item (before the final `Separator { label: None }` and the `Action` item):

```rust
            SettingItem::Separator {
                label: Some("Hooks".into()),
            },
            SettingItem::Text {
                label: "Post-Rip Command".into(),
                key: "post_rip.command".into(),
                value: config.post_rip.as_ref().and_then(|h| h.command.clone()).unwrap_or_default(),
            },
            SettingItem::Toggle {
                label: "  Run on Failure".into(),
                key: "post_rip.on_failure".into(),
                value: config.post_rip.as_ref().map(|h| h.on_failure()).unwrap_or(false),
            },
            SettingItem::Toggle {
                label: "  Blocking".into(),
                key: "post_rip.blocking".into(),
                value: config.post_rip.as_ref().map(|h| h.blocking()).unwrap_or(true),
            },
            SettingItem::Toggle {
                label: "  Log Output".into(),
                key: "post_rip.log_output".into(),
                value: config.post_rip.as_ref().map(|h| h.log_output()).unwrap_or(true),
            },
            SettingItem::Text {
                label: "Post-Session Command".into(),
                key: "post_session.command".into(),
                value: config.post_session.as_ref().and_then(|h| h.command.clone()).unwrap_or_default(),
            },
            SettingItem::Toggle {
                label: "  Run on Failure".into(),
                key: "post_session.on_failure".into(),
                value: config.post_session.as_ref().map(|h| h.on_failure()).unwrap_or(false),
            },
            SettingItem::Toggle {
                label: "  Blocking".into(),
                key: "post_session.blocking".into(),
                value: config.post_session.as_ref().map(|h| h.blocking()).unwrap_or(true),
            },
            SettingItem::Toggle {
                label: "  Log Output".into(),
                key: "post_session.log_output".into(),
                value: config.post_session.as_ref().map(|h| h.log_output()).unwrap_or(true),
            },
```

- [ ] **Step 4: Add hook fields to `to_config()`**

In the `to_config()` method's `SettingItem::Text` match arm, add cases for hook commands:

```rust
                    "post_rip.command" if !value.is_empty() => {
                        let hook = config.post_rip.get_or_insert_with(Default::default);
                        hook.command = Some(value.clone());
                    }
                    "post_session.command" if !value.is_empty() => {
                        let hook = config.post_session.get_or_insert_with(Default::default);
                        hook.command = Some(value.clone());
                    }
```

In the `SettingItem::Toggle` match arm, add cases for hook toggles:

```rust
                    "post_rip.on_failure" if *value => {
                        let hook = config.post_rip.get_or_insert_with(Default::default);
                        hook.on_failure = Some(true);
                    }
                    "post_rip.blocking" if !*value => {
                        let hook = config.post_rip.get_or_insert_with(Default::default);
                        hook.blocking = Some(false);
                    }
                    "post_rip.log_output" if !*value => {
                        let hook = config.post_rip.get_or_insert_with(Default::default);
                        hook.log_output = Some(false);
                    }
                    "post_session.on_failure" if *value => {
                        let hook = config.post_session.get_or_insert_with(Default::default);
                        hook.on_failure = Some(true);
                    }
                    "post_session.blocking" if !*value => {
                        let hook = config.post_session.get_or_insert_with(Default::default);
                        hook.blocking = Some(false);
                    }
                    "post_session.log_output" if !*value => {
                        let hook = config.post_session.get_or_insert_with(Default::default);
                        hook.log_output = Some(false);
                    }
```

- [ ] **Step 5: Update the item count test**

The `test_settings_state_from_config_item_count` test checks for 19 non-separator items. We're adding 8 new settings items (2 Text + 6 Toggle) and 1 new Separator. Update the assertion:

```rust
        assert_eq!(non_separator_count, 27); // 26 settings + 1 action
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --lib types::tests`
Expected: All pass including new hook tests.

- [ ] **Step 7: Run full test suite**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 8: Commit**

```
feat: add hooks section to settings panel with to_config roundtrip
```

---

### Task 9: Update CLAUDE.md documentation

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update relevant sections**

In the "Key Design Decisions" section, add a bullet:

```
- **Post-rip hooks** — `[post_rip]` and `[post_session]` config tables with `command`, `on_failure`, `blocking`, `log_output` fields. Commands run via `sh -c` with `{var}` template expansion (TODO(debt): shell injection risk from unescaped values). Per-file hook fires after each playlist remux; per-session hook fires after all jobs complete. Both called from CLI (`cli.rs`) and TUI (`tui/dashboard.rs`). `--no-hooks` disables for the run. Hook failures are logged but never fail the rip.
```

In the "Architecture" section, add `hooks.rs` to the numbered list:

```
12. `hooks.rs` — post-rip/post-session hook execution: template expansion, `sh -c` execution, blocking/non-blocking modes, output logging
```

In the "CLI Flags" section, add:

```
      --no-hooks               Disable post-rip/post-session hooks for this run
```

In the "Environment variable overrides" bullet, note that hook config has no env var overrides.

- [ ] **Step 2: Commit**

```
docs: update CLAUDE.md for post-rip hooks
```

---

### Task 10: Final verification

- [ ] **Step 1: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 2: Run fmt**

Run: `rustup run stable cargo fmt`

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 4: Verify config roundtrip manually**

Run: `cargo run -- --settings` — open settings panel, scroll to Hooks section, set a post-rip command, save with Ctrl+S, close. Verify the config file has the `[post_rip]` section.

- [ ] **Step 5: Commit if any fmt/clippy fixes were needed**

```
style: fmt and clippy fixes for post-rip hooks
```
