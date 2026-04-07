# Medium Bug Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the 4 remaining medium-severity bugs from the codebase review.

**Architecture:** All fixes are isolated to their respective modules with no cross-fix dependencies. Each fix can be implemented and tested independently.

**Tech Stack:** Rust, serde_json, anyhow, libc (FFI)

**Spec:** `docs/superpowers/specs/2026-04-07-medium-bugfixes-design.md`

---

## File Map

| File | Changes |
|------|---------|
| `src/cli.rs` | Fix 1: add `movie_mode` parameter to `rip_selected()` |
| `src/tmdb.rs` | Fix 2: extract `extract_array` helper, replace inline pattern |
| `src/media/probe.rs` | Fix 3: exclude NUL from log callback, harden `parse_child_output` |
| `src/drive_monitor.rs` | Fix 4: emit eject+insert on disc swap |

---

### Task 1: Pass resolved `movie_mode` to `rip_selected`

**Files:**
- Modify: `src/cli.rs:1167-1228` (function signature and body)
- Modify: `src/cli.rs:538-553` (call site in `run()`)

- [ ] **Step 1: Add `movie_mode` parameter to `rip_selected`**

In `src/cli.rs`, change the `rip_selected` function signature (line 1167) to add `movie_mode: bool` after `probe_cache`:

Current:
```rust
#[allow(clippy::too_many_arguments)]
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
    stream_filter: &crate::streams::StreamFilter,
    tracks_spec: Option<&str>,
    skip_eject: bool,
    probe_cache: &crate::types::ProbeCache,
) -> anyhow::Result<(u32, u32)> {
```

New:
```rust
#[allow(clippy::too_many_arguments)]
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
    stream_filter: &crate::streams::StreamFilter,
    tracks_spec: Option<&str>,
    skip_eject: bool,
    probe_cache: &crate::types::ProbeCache,
    movie_mode: bool,
) -> anyhow::Result<(u32, u32)> {
```

- [ ] **Step 2: Remove the re-derivation inside `rip_selected`**

In `src/cli.rs`, line 1227, remove the local re-derivation:

Current:
```rust
    let movie_mode = args.movie;
    let mode_str = if movie_mode { "movie" } else { "tv" };
```

New (just derive `mode_str` from the parameter):
```rust
    let mode_str = if movie_mode { "movie" } else { "tv" };
```

- [ ] **Step 3: Pass resolved `movie_mode` at the call site**

In `src/cli.rs`, find the call to `rip_selected` (around line 538-553). Add `movie_mode` as the last argument:

Current:
```rust
    let (success_count, fail_count) = rip_selected(
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
        stream_filter,
        tracks_spec,
        skip_eject,
        &probe_cache,
    )?;
```

New:
```rust
    let (success_count, fail_count) = rip_selected(
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
        stream_filter,
        tracks_spec,
        skip_eject,
        &probe_cache,
        movie_mode,
    )?;
```

The `movie_mode` variable is already in scope at this point — it's returned from `scan_disc()` on line 316.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 6: Commit**

```
git add src/cli.rs && git commit -m "fix: pass resolved movie_mode to rip_selected for correct hook vars

rip_selected() re-derived movie_mode from args.movie, ignoring auto-detection
logic in scan_disc(). Hook {mode} var was 'tv' when it should have been
'movie' for auto-detected single-playlist discs.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Extract TMDb response key helper

**Files:**
- Modify: `src/tmdb.rs:82-102` (three functions + new helper)

- [ ] **Step 1: Add the `extract_array` helper**

In `src/tmdb.rs`, add this function after `tmdb_get` (after line 80):

```rust
fn extract_array<T: serde::de::DeserializeOwned>(
    data: &serde_json::Value,
    key: &str,
) -> Result<Vec<T>> {
    let val = data
        .get(key)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("TMDb response missing '{}' field", key))?;
    serde_json::from_value(val).with_context(|| format!("failed to parse TMDb '{}' field", key))
}
```

- [ ] **Step 2: Replace inline pattern in all three functions**

Replace `search_show` (lines 82-87):
```rust
pub fn search_show(query: &str, api_key: &str) -> Result<Vec<TmdbShow>> {
    let data = tmdb_get("/search/tv", api_key, &[("query", query)])?;
    extract_array(&data, "results")
}
```

Replace `search_movie` (lines 89-94):
```rust
pub fn search_movie(query: &str, api_key: &str) -> Result<Vec<TmdbMovie>> {
    let data = tmdb_get("/search/movie", api_key, &[("query", query)])?;
    extract_array(&data, "results")
}
```

Replace `get_season` (lines 96-102):
```rust
pub fn get_season(show_id: u64, season: u32, api_key: &str) -> Result<Vec<Episode>> {
    let path = format!("/tv/{}/season/{}", show_id, season);
    let data = tmdb_get(&path, api_key, &[])?;
    extract_array(&data, "episodes")
}
```

- [ ] **Step 3: Add tests for `extract_array`**

There is no existing `#[cfg(test)]` module in `tmdb.rs`. Add one at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Item {
        id: u32,
        name: String,
    }

    #[test]
    fn test_extract_array_valid() {
        let data = json!({"results": [{"id": 1, "name": "Test"}]});
        let items: Vec<Item> = extract_array(&data, "results").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, 1);
    }

    #[test]
    fn test_extract_array_empty() {
        let data = json!({"results": []});
        let items: Vec<Item> = extract_array(&data, "results").unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_extract_array_missing_key() {
        let data = json!({"other": []});
        let result: Result<Vec<Item>> = extract_array(&data, "results");
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("missing 'results' field"), "got: {}", err);
    }

    #[test]
    fn test_extract_array_null_value() {
        let data = json!({"results": null});
        let result: Result<Vec<Item>> = extract_array(&data, "results");
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("parse"), "got: {}", err);
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass, including the 4 new TMDb tests and the existing integration tests in `tests/tmdb_parsing.rs`.

- [ ] **Step 5: Commit**

```
git add src/tmdb.rs && git commit -m "fix: extract TMDb response helper for clear error messages

search_show/search_movie/get_season used unwrap_or_default() which
yielded Value::Null when the expected key was missing, producing opaque
deserialization errors. New extract_array() helper gives clear messages
like 'TMDb response missing results field'.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Fix NUL byte in log capture and harden protocol parser

**Files:**
- Modify: `src/media/probe.rs:288-310` (`parse_child_output`)
- Modify: `src/media/probe.rs:348-349` (log callback NUL exclusion)

- [ ] **Step 1: Add test for `parse_child_output` with embedded NUL**

In `src/media/probe.rs`, find the `#[cfg(test)] mod tests` block (starts around line 847). Add these tests:

```rust
    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_child_output_success() {
        let data = "log line 1\nlog line 2\n\0";
        let (lines, status, error) = parse_child_output(data);
        assert_eq!(status, 0);
        assert!(error.is_none());
        assert_eq!(lines, "log line 1\nlog line 2\n");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_child_output_error() {
        let data = "log line\n\x01AACS failed";
        let (lines, status, error) = parse_child_output(data);
        assert_eq!(status, 1);
        assert_eq!(error, Some("AACS failed".to_string()));
        assert_eq!(lines, "log line\n");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_child_output_embedded_nul_with_error() {
        // Simulates: log line with truncated NUL + error status byte
        let data = "log with \0 embedded\n\x01AACS failed";
        let (lines, status, error) = parse_child_output(data);
        assert_eq!(status, 1, "should detect error status, not embedded NUL");
        assert_eq!(error, Some("AACS failed".to_string()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_child_output_embedded_nul_with_success() {
        // Simulates: log line with truncated NUL + success status byte
        let data = "log with \0 embedded\n\0";
        let (lines, status, error) = parse_child_output(data);
        assert_eq!(status, 0);
        assert!(error.is_none());
    }
```

- [ ] **Step 2: Run the embedded NUL test to verify it fails**

Run: `cargo test --lib media::probe::tests::test_parse_child_output_embedded_nul_with_error -- --exact`
Expected: FAIL — the current `rfind('\0')` finds the embedded NUL and returns status 0 instead of 1.

- [ ] **Step 3: Fix `parse_child_output`**

In `src/media/probe.rs`, replace the `parse_child_output` function (lines 288-310):

```rust
#[cfg(target_os = "linux")]
fn parse_child_output(data: &str) -> (&str, u8, Option<String>) {
    // Format: log lines (newline-separated), then status byte (0=ok, 1=err),
    // then optional error message.
    // Find whichever of \0 or \x01 appears last — the status byte is always
    // appended after all log content.
    let pos_zero = data.rfind('\0');
    let pos_one = data.rfind('\x01');

    let (pos, status) = match (pos_zero, pos_one) {
        (Some(z), Some(o)) => {
            if z > o {
                (z, 0u8)
            } else {
                (o, 1u8)
            }
        }
        (Some(z), None) => (z, 0),
        (None, Some(o)) => (o, 1),
        (None, None) => {
            return (
                data,
                1,
                Some("Child process terminated unexpectedly".into()),
            );
        }
    };

    let error_msg = if status != 0 && pos + 1 < data.len() {
        Some(data[pos + 1..].to_string())
    } else {
        None
    };

    (&data[..pos], status, error_msg)
}
```

- [ ] **Step 4: Fix the log callback NUL exclusion**

In `src/media/probe.rs`, find line 349 inside the `log_capture_body!` macro:

Current:
```rust
            let len = (len as usize).min(buf.len());
```

New:
```rust
            let len = (len as usize).min(buf.len().saturating_sub(1));
```

This excludes the NUL terminator that `av_log_format_line2` writes when it truncates output.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib media::probe::tests`
Expected: All probe tests pass, including the 4 new `parse_child_output` tests.

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```
git add src/media/probe.rs && git commit -m "fix: exclude NUL from log capture, harden child-parent protocol parser

Log callback included NUL terminator from av_log truncation. If a log
line exceeded 1023 chars AND an AACS error occurred, parse_child_output's
rfind('\\0') would find the embedded NUL and misclassify the error as
success. Fixed both: callback excludes NUL, parser finds whichever of
\\0/\\x01 appears last (the actual status byte).

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: DriveMonitor emits eject+insert on disc swap

**Files:**
- Modify: `src/drive_monitor.rs:56-62` (add swap detection branch)

- [ ] **Step 1: Add test for disc swap**

In `src/drive_monitor.rs`, add this test in the `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn test_disc_swap_emits_eject_and_insert() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        // Initial state: drive with disc A
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "DISC_A".into());
        let _ = collect_events(&rx); // drain initial events

        // Swap: disc A -> disc B in one poll interval
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "DISC_B".into());
        let events = collect_events(&rx);
        assert_eq!(
            events,
            vec!["ejected:/dev/sr0", "inserted:/dev/sr0:DISC_B"],
            "disc swap should emit eject then insert"
        );
    }

    #[test]
    fn test_same_disc_no_events() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "SAME_DISC".into());
        let _ = collect_events(&rx);

        // Same label again — no events
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "SAME_DISC".into());
        let events = collect_events(&rx);
        assert!(events.is_empty(), "same disc should produce no events");
    }
```

- [ ] **Step 2: Run the swap test to verify it fails**

Run: `cargo test --lib drive_monitor::tests::test_disc_swap_emits_eject_and_insert -- --exact`
Expected: FAIL — currently no events are emitted for non-empty to different non-empty transitions.

- [ ] **Step 3: Add the swap detection branch**

In `src/drive_monitor.rs`, find lines 56-62. Add an `else if` branch after the eject check:

Current:
```rust
            if old_label.is_empty() && !label.is_empty() {
                let _ = self
                    .tx
                    .send(DriveEvent::DiscInserted(drive.clone(), label.clone()));
            } else if !old_label.is_empty() && label.is_empty() {
                let _ = self.tx.send(DriveEvent::DiscEjected(drive.clone()));
            }
```

New:
```rust
            if old_label.is_empty() && !label.is_empty() {
                let _ = self
                    .tx
                    .send(DriveEvent::DiscInserted(drive.clone(), label.clone()));
            } else if !old_label.is_empty() && label.is_empty() {
                let _ = self.tx.send(DriveEvent::DiscEjected(drive.clone()));
            } else if !old_label.is_empty() && !label.is_empty() && old_label != label {
                let _ = self.tx.send(DriveEvent::DiscEjected(drive.clone()));
                let _ = self
                    .tx
                    .send(DriveEvent::DiscInserted(drive.clone(), label.clone()));
            }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib drive_monitor::tests`
Expected: All drive_monitor tests pass, including the 2 new ones.

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```
git add src/drive_monitor.rs && git commit -m "fix: detect disc swaps in DriveMonitor

diff_and_emit only detected empty<->non-empty label transitions. A disc
swap within one poll interval (label A -> label B, both non-empty) was
silently ignored. Now emits DiscEjected + DiscInserted for label changes.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Run formatter**

Run: `rustup run stable cargo fmt`
Expected: No changes, or apply formatting.

- [ ] **Step 4: Commit formatting if needed**

Only if `cargo fmt` made changes:

```
git add -A && git commit -m "style: apply rustfmt

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```
