# MPLS Refactor: Unified Parsing + On-Disc Clip Sizes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace bitrate-based size estimates with real on-disc clip file sizes by refactoring MPLS parsing into a single pass that extracts both chapters and clip sizes.

**Architecture:** `parse_mpls_info()` becomes the core function — parses one MPLS file, extracts chapters from entry-point marks, and stats referenced `.m2ts` clip files. `collect_mpls_info()` is the bulk wrapper called once per mount window. `extract_chapters()` becomes a thin wrapper for rip-time use. Clip sizes flow into `DiscState.clip_sizes` and are read at `RipJob` construction time.

**Tech Stack:** Rust, `mpls` crate (v0.2.0), `std::fs::metadata` for file sizes

---

### Task 1: Add `MplsInfo` struct and `parse_mpls_info` function

**Files:**
- Modify: `src/chapters.rs` (full file — lines 1-79)

This task refactors the core MPLS parsing. `parse_mpls_info` parses an MPLS file once and extracts both chapter marks and clip file sizes. The existing `extract_chapters` becomes a thin wrapper.

- [ ] **Step 1: Write failing test for `parse_mpls_info` with missing path**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of `src/chapters.rs`:

```rust
    #[test]
    fn test_parse_mpls_info_missing_path() {
        let result = parse_mpls_info(std::path::Path::new("/nonexistent/path"), "00001");
        assert!(result.is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -- test_parse_mpls_info_missing_path`
Expected: FAIL — `parse_mpls_info` does not exist yet.

- [ ] **Step 3: Add `MplsInfo` struct and implement `parse_mpls_info`**

Replace the entire content of `src/chapters.rs` with:

```rust
use crate::types::ChapterMark;
use std::collections::HashMap;
use std::path::Path;

/// Data extracted from a single MPLS playlist file.
pub struct MplsInfo {
    pub chapters: Vec<ChapterMark>,
    /// Sum of on-disc `.m2ts` clip file sizes in bytes.
    /// Zero if the STREAM directory is missing or all clips fail to stat.
    pub clip_size: u64,
}

/// Parse an MPLS playlist file and extract chapter marks and clip file sizes.
///
/// Reads `BDMV/PLAYLIST/{playlist_num}.mpls` from the mounted disc.
/// For each PlayItem, stats `BDMV/STREAM/{clip_name}.m2ts` and sums the sizes.
/// Only the primary clip per PlayItem is used (angle 0), matching bluback's remux behavior.
///
/// Returns `None` if the MPLS file can't be read, can't be parsed, or has no entry-point marks.
pub fn parse_mpls_info(mount_point: &Path, playlist_num: &str) -> Option<MplsInfo> {
    let mpls_path = mount_point
        .join("BDMV")
        .join("PLAYLIST")
        .join(format!("{}.mpls", playlist_num));

    let file = std::fs::File::open(&mpls_path).ok()?;
    let mpls_data = mpls::Mpls::from(file).ok()?;

    // Extract chapter marks from entry-point marks
    let entry_marks: Vec<_> = mpls_data
        .marks
        .iter()
        .filter(|m| matches!(m.mark_type, mpls::types::MarkType::EntryPoint))
        .collect();

    if entry_marks.is_empty() {
        return None;
    }

    let base_secs = entry_marks[0].time_stamp.seconds();
    let chapters: Vec<ChapterMark> = entry_marks
        .iter()
        .enumerate()
        .map(|(i, mark)| ChapterMark {
            index: (i + 1) as u32,
            start_secs: mark.time_stamp.seconds() - base_secs,
        })
        .collect();

    // Sum clip file sizes from play items (primary clip only, not multi-angle)
    let stream_dir = mount_point.join("BDMV").join("STREAM");
    let clip_size: u64 = mpls_data
        .play_list
        .play_items
        .iter()
        .map(|item| {
            let clip_path = stream_dir.join(format!("{}.m2ts", item.clip.file_name));
            match std::fs::metadata(&clip_path) {
                Ok(meta) => meta.len(),
                Err(_) => {
                    log::debug!(
                        "Clip file not found: {} (playlist {})",
                        clip_path.display(),
                        playlist_num
                    );
                    0
                }
            }
        })
        .sum();

    Some(MplsInfo {
        chapters,
        clip_size,
    })
}

/// Collect MPLS info (chapters + clip sizes) for multiple playlists in one pass.
///
/// Returns a map of playlist number → `MplsInfo`.
/// Playlists whose MPLS files can't be read are omitted from the result.
pub fn collect_mpls_info(
    mount_point: &Path,
    playlist_nums: &[&str],
) -> HashMap<String, MplsInfo> {
    let mut info = HashMap::new();
    for &num in playlist_nums {
        if let Some(mpls_info) = parse_mpls_info(mount_point, num) {
            info.insert(num.to_string(), mpls_info);
        }
    }
    info
}

/// Extract chapter marks from an MPLS playlist file on a mounted Blu-ray disc.
///
/// Thin wrapper around `parse_mpls_info` — returns only the chapters.
/// Used by `workflow::prepare_remux_options` during ripping.
pub fn extract_chapters(mount_point: &Path, playlist_num: &str) -> Option<Vec<ChapterMark>> {
    parse_mpls_info(mount_point, playlist_num).map(|info| info.chapters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mpls_info_missing_path() {
        let result = parse_mpls_info(std::path::Path::new("/nonexistent/path"), "00001");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_mpls_info_missing_playlist() {
        let dir = std::env::temp_dir().join("bluback_test_mpls_info");
        let playlist_dir = dir.join("BDMV").join("PLAYLIST");
        std::fs::create_dir_all(&playlist_dir).unwrap();
        let result = parse_mpls_info(&dir, "99999");
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_extract_chapters_missing_path() {
        let result = extract_chapters(std::path::Path::new("/nonexistent/path"), "00001");
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_chapters_missing_playlist() {
        let dir = std::env::temp_dir().join("bluback_test_chapters");
        let playlist_dir = dir.join("BDMV").join("PLAYLIST");
        std::fs::create_dir_all(&playlist_dir).unwrap();
        let result = extract_chapters(&dir, "99999");
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

Key changes from original:
- New `MplsInfo` struct with `chapters` and `clip_size`
- `parse_mpls_info` is the core function — parses MPLS, extracts chapters, stats clip files
- `collect_mpls_info` replaces `count_chapters_for_playlists` — returns `HashMap<String, MplsInfo>`
- `extract_chapters` is now a thin wrapper that delegates and returns only chapters
- `count_chapters_for_playlists` is removed

- [ ] **Step 4: Run all tests in chapters module**

Run: `cargo test -- chapters`
Expected: All 4 tests PASS.

- [ ] **Step 5: Verify the build compiles**

Run: `cargo build 2>&1`
Expected: FAIL — the 3 call sites still reference `count_chapters_for_playlists`. This is expected; Task 2 fixes them.

- [ ] **Step 6: Commit**

```
feat: refactor MPLS parsing to extract chapters + clip sizes in one pass

Introduces MplsInfo struct and parse_mpls_info() as the single-parse core.
collect_mpls_info() replaces count_chapters_for_playlists().
extract_chapters() becomes a thin wrapper for rip-time use.
```

---

### Task 2: Add `clip_sizes` to `DiscState` and update all call sites

**Files:**
- Modify: `src/tui/mod.rs:44-54` (DiscState struct)
- Modify: `src/session.rs:736-759` (TUI call site)
- Modify: `src/cli.rs:108-124` (CLI list_playlists call site)
- Modify: `src/cli.rs:319-335` (CLI run_interactive call site)

- [ ] **Step 1: Add `clip_sizes` field to `DiscState`**

In `src/tui/mod.rs`, add the field after `chapter_counts`:

```rust
#[derive(Default)]
pub struct DiscState {
    pub label: String,
    pub label_info: Option<LabelInfo>,
    pub playlists: Vec<Playlist>,
    pub episodes_pl: Vec<Playlist>,
    pub scan_log: Vec<String>,
    pub mount_point: Option<String>,
    pub did_mount: bool,
    pub chapter_counts: HashMap<String, usize>,
    pub clip_sizes: HashMap<String, u64>,
}
```

- [ ] **Step 2: Update TUI call site in `session.rs`**

In `src/session.rs`, replace lines 747-748:

```rust
                        self.disc.chapter_counts =
                            crate::chapters::count_chapters_for_playlists(mount_path, &nums);
```

With:

```rust
                        let mpls_info =
                            crate::chapters::collect_mpls_info(mount_path, &nums);
                        self.disc.chapter_counts = mpls_info
                            .iter()
                            .map(|(k, v)| (k.clone(), v.chapters.len()))
                            .collect();
                        self.disc.clip_sizes = mpls_info
                            .into_iter()
                            .map(|(k, v)| (k, v.clip_size))
                            .collect();
```

Also update the error branch at line 756. Add `self.disc.clip_sizes.clear();` after `self.disc.chapter_counts.clear();`:

```rust
                    Err(_) => {
                        self.disc.chapter_counts.clear();
                        self.disc.clip_sizes.clear();
                        None
                    }
```

- [ ] **Step 3: Update CLI `list_playlists` call site in `cli.rs`**

In `src/cli.rs`, replace lines 109-124:

```rust
    // Mount disc for chapter counts, clip sizes, and title order
    let (chapter_counts, _clip_sizes, title_order) = {
        let device_str = device.to_string();
        match disc::ensure_mounted(&device_str) {
            Ok((mount, did_mount)) => {
                let mount_path = std::path::Path::new(&mount);
                let nums: Vec<&str> = playlists.iter().map(|pl| pl.num.as_str()).collect();
                let mpls_info = crate::chapters::collect_mpls_info(mount_path, &nums);
                let counts: std::collections::HashMap<String, usize> = mpls_info
                    .iter()
                    .map(|(k, v)| (k.clone(), v.chapters.len()))
                    .collect();
                let sizes: std::collections::HashMap<String, u64> = mpls_info
                    .into_iter()
                    .map(|(k, v)| (k, v.clip_size))
                    .collect();
                let order = crate::index::parse_title_order(mount_path);
                if did_mount {
                    let _ = disc::unmount_disc(&device_str);
                }
                (counts, sizes, order)
            }
            Err(_) => (
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
                None,
            ),
        }
    };
```

Note: `_clip_sizes` is unused in `list_playlists` for now. The underscore prefix suppresses the warning while keeping the data available for a future `--list-playlists` size column.

- [ ] **Step 4: Update CLI `run_interactive` call site in `cli.rs`**

In `src/cli.rs`, replace lines 320-335 with the same pattern:

```rust
    // Mount disc for chapter counts, clip sizes, and title order
    let (chapter_counts, _clip_sizes, title_order) = {
        let device_str = device.to_string();
        match disc::ensure_mounted(&device_str) {
            Ok((mount, did_mount)) => {
                let mount_path = std::path::Path::new(&mount);
                let nums: Vec<&str> = all_playlists.iter().map(|pl| pl.num.as_str()).collect();
                let mpls_info = crate::chapters::collect_mpls_info(mount_path, &nums);
                let counts: std::collections::HashMap<String, usize> = mpls_info
                    .iter()
                    .map(|(k, v)| (k.clone(), v.chapters.len()))
                    .collect();
                let sizes: std::collections::HashMap<String, u64> = mpls_info
                    .into_iter()
                    .map(|(k, v)| (k, v.clip_size))
                    .collect();
                let order = crate::index::parse_title_order(mount_path);
                if did_mount {
                    let _ = disc::unmount_disc(&device_str);
                }
                (counts, sizes, order)
            }
            Err(_) => (
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
                None,
            ),
        }
    };
```

Note: CLI mode does not use `RipJob` or `estimated_size`, so `_clip_sizes` is unused here. The underscore prefix suppresses the warning.

- [ ] **Step 5: Build and run tests**

Run: `cargo build && cargo test`
Expected: Build succeeds (no more references to `count_chapters_for_playlists`). All tests pass.

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: Clean.

- [ ] **Step 7: Commit**

```
feat: add clip_sizes to DiscState, switch call sites to collect_mpls_info
```

---

### Task 3: Use real clip sizes in `RipJob.estimated_size`

**Files:**
- Modify: `src/tui/wizard.rs:1585-1601` (RipJob construction)

- [ ] **Step 1: Update `estimated_size` computation to prefer clip sizes**

In `src/tui/wizard.rs`, replace the current estimated_size computation block (lines 1585-1601):

```rust
                // ~20 Mbps (2.5 MB/s) fallback if no probed bitrate
                const FALLBACK_BYTERATE: u64 = 2_500_000;
                let byterate = session
                    .wizard
                    .media_infos
                    .get(&pl.num)
                    .map(|info| info.bitrate_bps / 8)
                    .filter(|&br| br > 0)
                    .unwrap_or(FALLBACK_BYTERATE);
                let estimated_size = pl.seconds as u64 * byterate;
                session.rip.jobs.push(crate::types::RipJob {
                    playlist: pl,
                    episode,
                    filename: filename.clone(),
                    status: crate::types::PlaylistStatus::Pending,
                    estimated_size,
                });
```

With:

```rust
                // Prefer real on-disc clip size; fall back to bitrate estimate
                let estimated_size = session
                    .disc
                    .clip_sizes
                    .get(&pl.num)
                    .copied()
                    .filter(|&sz| sz > 0)
                    .unwrap_or_else(|| {
                        const FALLBACK_BYTERATE: u64 = 2_500_000;
                        let byterate = session
                            .wizard
                            .media_infos
                            .get(&pl.num)
                            .map(|info| info.bitrate_bps / 8)
                            .filter(|&br| br > 0)
                            .unwrap_or(FALLBACK_BYTERATE);
                        pl.seconds as u64 * byterate
                    });
                session.rip.jobs.push(crate::types::RipJob {
                    playlist: pl,
                    episode,
                    filename: filename.clone(),
                    status: crate::types::PlaylistStatus::Pending,
                    estimated_size,
                });
```

- [ ] **Step 2: Build and run tests**

Run: `cargo build && cargo test`
Expected: Build succeeds. All tests pass.

- [ ] **Step 3: Run clippy and fmt**

Run: `rustup run stable cargo fmt && cargo clippy -- -D warnings`
Expected: Clean.

- [ ] **Step 4: Commit**

```
feat: use real on-disc clip sizes for RipJob estimated_size

Falls back to bitrate × duration estimate when clip sizes are
unavailable (unmounted disc, UHD/SSIF layouts, or zero-size clips).
```

---

### Task 4: Final verification

- [ ] **Step 1: Run the full pre-commit checklist**

Run all three in sequence:

```bash
cargo test
rustup run stable cargo fmt -- --check
cargo clippy -- -D warnings
```

Expected: All pass with no errors or warnings.

- [ ] **Step 2: Verify no references to removed function remain**

Run: `grep -r "count_chapters_for_playlists" src/`
Expected: No matches.

- [ ] **Step 3: Verify `extract_chapters` still works as a wrapper**

Run: `cargo test -- extract_chapters`
Expected: Both `test_extract_chapters_missing_path` and `test_extract_chapters_missing_playlist` pass.
