# MPLS-Based Stream Counts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace per-playlist device probing during disc scan with MPLS file reads to eliminate rapid-fire AACS authentication cycles that crash USB bridges.

**Architecture:** Remove the `probe_playlist()` loop from `scan_playlists_with_progress()` and replace it with `mpls_stream_counts()` — a new function that reads stream counts from MPLS files via plain filesystem I/O. The probe cache is returned empty; full probing is deferred to on-demand (TUI track picker) and rip-time (CLI stream selection). All code that used probe cache membership as a proxy for "episode-length playlist" is changed to use the duration threshold directly.

**Tech Stack:** Rust, `mpls` crate (already a dependency), `ffmpeg-the-third`

**Spec:** `docs/superpowers/specs/2026-04-14-mpls-stream-counts-design.md`

---

### Task 1: Add `mpls_stream_counts()` to chapters module

**Files:**
- Modify: `src/chapters.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module at the bottom of `src/chapters.rs`:

```rust
#[test]
fn test_mpls_stream_counts_missing_path() {
    let result = mpls_stream_counts(std::path::Path::new("/nonexistent/path"), "00001");
    assert!(result.is_none());
}

#[test]
fn test_mpls_stream_counts_missing_playlist() {
    let dir = std::env::temp_dir().join("bluback_test_stream_counts");
    let playlist_dir = dir.join("BDMV").join("PLAYLIST");
    std::fs::create_dir_all(&playlist_dir).unwrap();
    let result = mpls_stream_counts(&dir, "99999");
    assert!(result.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test mpls_stream_counts -- --test-threads=1`
Expected: FAIL — `mpls_stream_counts` not found

- [ ] **Step 3: Write the implementation**

Add above the `#[cfg(test)]` block in `src/chapters.rs`:

```rust
/// Get stream counts from an MPLS playlist file without opening the device.
///
/// Reads the `StreamNumberTable` from the first PlayItem to get video, audio,
/// and subtitle (PGS) stream counts. Returns `None` if the MPLS file can't be
/// read or parsed, or if the playlist has no PlayItems.
///
/// This avoids opening `bluray:{device}` (which triggers AACS authentication)
/// just to count streams. Blu-ray playlists have consistent stream tables across
/// PlayItems since they are segments of the same content.
pub fn mpls_stream_counts(mount_point: &Path, playlist_num: &str) -> Option<(u32, u32, u32)> {
    let mpls_path = mount_point
        .join("BDMV")
        .join("PLAYLIST")
        .join(format!("{}.mpls", playlist_num));

    let file = std::fs::File::open(&mpls_path).ok()?;
    let mpls_data = mpls::Mpls::from(file).ok()?;

    let first_item = mpls_data.play_list.play_items.first()?;
    let stn = &first_item.stream_number_table;

    Some((
        stn.primary_video_streams.len() as u32,
        stn.primary_audio_streams.len() as u32,
        stn.primary_pgs_streams.len() as u32,
    ))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test mpls_stream_counts -- --test-threads=1`
Expected: PASS — both tests return `None` for missing paths

- [ ] **Step 5: Run full test suite and clippy**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: All pass

- [ ] **Step 6: Commit**

```
feat: add mpls_stream_counts() for device-free stream count reads
```

---

### Task 2: Replace probe loop with MPLS reads in `scan_playlists_with_progress()`

**Files:**
- Modify: `src/media/probe.rs`

- [ ] **Step 1: Replace the probe loop**

In `src/media/probe.rs`, replace lines 107-136 (the `probe_indices` / `probe_playlist()` loop) with MPLS-based stream count reads:

```rust
    // Read stream counts from MPLS files instead of opening the device per-playlist.
    // This avoids rapid-fire AACS authentication cycles that can crash USB bridges
    // (especially ASMedia USB-SATA bridges with libmmbd backend).
    let mpls_counts: HashMap<String, (u32, u32, u32)> = match crate::disc::ensure_mounted(device) {
        Ok((mount, did_mount)) => {
            let mount_path = std::path::Path::new(&mount);
            let total = playlists.len();
            let counts: HashMap<String, (u32, u32, u32)> = playlists
                .iter()
                .enumerate()
                .filter_map(|(i, pl)| {
                    if let Some(cb) = &on_probe_progress {
                        cb(i + 1, total, &pl.num);
                    }
                    crate::chapters::mpls_stream_counts(mount_path, &pl.num)
                        .map(|c| (pl.num.clone(), c))
                })
                .collect();
            if did_mount {
                let _ = crate::disc::unmount_disc(device);
            }
            counts
        }
        Err(e) => {
            log::warn!("Could not mount disc for MPLS stream counts: {}", e);
            HashMap::new()
        }
    };

    for pl in &mut playlists {
        if let Some(&(v, a, s)) = mpls_counts.get(&pl.num) {
            pl.video_streams = v;
            pl.audio_streams = a;
            pl.subtitle_streams = s;
        }
    }

    let probe_cache = HashMap::new();
```

Keep the existing `pre_classify_playlists()` call (lines 101-105) and the return statement (lines 138-139) as-is.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: Clean compile — function signature unchanged, return type unchanged

- [ ] **Step 3: Run tests**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: All pass (no existing tests depend on probe cache contents from scan)

- [ ] **Step 4: Commit**

```
fix: replace per-playlist device probing with MPLS file reads during scan

Eliminates rapid-fire AACS authentication cycles that crash ASMedia
USB-SATA bridges. Stream counts now come from MPLS files (plain file I/O)
instead of opening bluray:{device} per-playlist.
```

---

### Task 3: Add skip_set to `BackgroundResult::DiscScan` and propagate in session

**Files:**
- Modify: `src/types.rs`
- Modify: `src/session.rs`

- [ ] **Step 1: Update the `DiscScan` variant in `types.rs`**

In `src/types.rs`, change line 253:

Old:
```rust
    /// Disc scan completed: (device, label, playlists, probe_cache)
    DiscScan(anyhow::Result<(String, String, Vec<Playlist>, ProbeCache)>),
```

New:
```rust
    /// Disc scan completed: (device, label, playlists, probe_cache, skip_set)
    DiscScan(
        anyhow::Result<(
            String,
            String,
            Vec<Playlist>,
            ProbeCache,
            std::collections::HashSet<String>,
        )>,
    ),
```

- [ ] **Step 2: Update `start_disc_scan()` in `session.rs` to include skip_set**

In `src/session.rs`, change the scan thread closure (around lines 691-712):

Old:
```rust
                let result = (|| -> anyhow::Result<(String, String, Vec<Playlist>, ProbeCache)> {
                    let (playlists, probe_cache, _skip_set) =
                        crate::media::scan_playlists_with_progress(
```

New:
```rust
                let result = (|| -> anyhow::Result<(String, String, Vec<Playlist>, ProbeCache, std::collections::HashSet<String>)> {
                    let (playlists, probe_cache, skip_set) =
                        crate::media::scan_playlists_with_progress(
```

And the return (line 710):

Old:
```rust
                    Ok((dev_str, label, playlists, probe_cache))
```

New:
```rust
                    Ok((dev_str, label, playlists, probe_cache, skip_set))
```

- [ ] **Step 3: Update the `DiscScan` handler in `poll_background()`**

In `src/session.rs`, change the match arm (line 815):

Old:
```rust
            BackgroundResult::DiscScan(Ok((device, label, playlists, probe_cache))) => {
```

New:
```rust
            BackgroundResult::DiscScan(Ok((device, label, playlists, probe_cache, skip_set))) => {
```

- [ ] **Step 4: Add `skip_set` field to `DriveSession`**

In `src/session.rs`, add a field to `DriveSession` struct after line 75 (`history_ripped_playlists`):

```rust
    /// Playlist numbers that were pre-classified (skipped during probe).
    pub skip_set: std::collections::HashSet<String>,
```

Initialize it in `DriveSession::new()` (add to the `Self { ... }` block):

```rust
            skip_set: std::collections::HashSet::new(),
```

And in the `reset()` method, clear it:

```rust
        self.skip_set.clear();
```

- [ ] **Step 5: Store skip_set in the `DiscScan` handler**

In `src/session.rs`, after the `BackgroundResult::DiscScan` match arm sets `self.disc.playlists = playlists` (around line 819), add:

```rust
                self.skip_set = skip_set;
```

- [ ] **Step 6: Verify it compiles and tests pass**

Run: `cargo build && cargo test && cargo clippy -- -D warnings`
Expected: All pass

- [ ] **Step 7: Commit**

```
refactor: propagate skip_set through DiscScan result to session
```

---

### Task 4: Replace `probed_playlists()` with duration-threshold filtering

**Files:**
- Modify: `src/session.rs`

- [ ] **Step 1: Update existing tests for `probed_playlists()` if any**

Run: `cargo test probed_playlists`
Check if there are tests that depend on the current behavior. If so, update them to expect duration-based filtering.

- [ ] **Step 2: Change `probed_playlists()` to use duration threshold**

In `src/session.rs`, replace the `probed_playlists()` method (lines 340-346):

Old:
```rust
    pub fn probed_playlists(&self) -> Vec<&Playlist> {
        self.disc
            .playlists
            .iter()
            .filter(|pl| self.wizard.stream_infos.contains_key(&pl.num))
            .collect()
    }
```

New:
```rust
    /// Returns playlists above the probe duration threshold that aren't pre-classified.
    /// These are the "episode-length" playlists used for detection and episode assignment.
    pub fn probed_playlists(&self) -> Vec<&Playlist> {
        let min_dur = self.config.min_probe_duration(self.min_probe_duration_arg);
        self.disc
            .playlists
            .iter()
            .filter(|pl| pl.seconds >= min_dur && !self.skip_set.contains(&pl.num))
            .collect()
    }
```

- [ ] **Step 3: Replace pre-classified specials logic**

In `src/session.rs`, replace lines 881-903 (the pre-classified specials block):

Old:
```rust
                if self.auto_detect {
                    let min_dur = self.config.min_probe_duration(self.min_probe_duration_arg);
                    for pl in &self.disc.playlists {
                        if pl.seconds >= min_dur
                            && !self.wizard.stream_infos.contains_key(&pl.num)
                            && !self
                                .wizard
                                .detection_results
                                .iter()
                                .any(|d| d.playlist_num == pl.num)
                        {
```

New:
```rust
                if self.auto_detect {
                    let min_dur = self.config.min_probe_duration(self.min_probe_duration_arg);
                    for pl in &self.disc.playlists {
                        if pl.seconds >= min_dur
                            && self.skip_set.contains(&pl.num)
                            && !self
                                .wizard
                                .detection_results
                                .iter()
                                .any(|d| d.playlist_num == pl.num)
                        {
```

- [ ] **Step 4: Update `build_tmdb_search_view()` probed_count**

At line 388, `probed_count: self.probed_playlists().len()` — this now uses the updated `probed_playlists()` which filters by duration threshold. No code change needed, just verify the semantics are correct.

- [ ] **Step 5: Verify it compiles and tests pass**

Run: `cargo build && cargo test && cargo clippy -- -D warnings`
Expected: All pass

- [ ] **Step 6: Commit**

```
fix: use duration threshold for episode-length filtering instead of probe cache
```

---

### Task 5: Replace `probe_cache.contains_key()` filtering in CLI

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Update `scan_disc()` counts**

In `src/cli.rs`, replace lines 931-944:

Old:
```rust
    let probed_count = probe_cache.len();
    let short_count = playlists.len() - probed_count;
    println!(
        "Found {} playlists ({} episode-length, {} short/extras).\n",
        playlists.len(),
        probed_count,
        short_count
    );

    if probe_cache.is_empty() {
        anyhow::bail!("No episode-length playlists found. Try lowering --min-probe-duration.");
    }

    let movie_mode = args.movie || (probed_count == 1 && args.season.is_none());
```

New:
```rust
    let probed_count = playlists
        .iter()
        .filter(|pl| pl.seconds >= min_probe_duration)
        .count();
    let short_count = playlists.len() - probed_count;
    println!(
        "Found {} playlists ({} episode-length, {} short/extras).\n",
        playlists.len(),
        probed_count,
        short_count
    );

    if probed_count == 0 {
        anyhow::bail!("No episode-length playlists found. Try lowering --min-probe-duration.");
    }

    let movie_mode = args.movie || (probed_count == 1 && args.season.is_none());
```

- [ ] **Step 2: Add `min_probe_duration` and `skip_set` to `scan_disc` return**

`run()` needs both the duration threshold and the skip set to properly filter episode-length playlists — the current `probe_cache.contains_key()` implicitly excludes pre-classified playlists (they were never probed). We must maintain that behavior.

Change `scan_disc` return type (line 872-882):

Old:
```rust
) -> anyhow::Result<(
    String,
    Option<LabelInfo>,
    Vec<Playlist>,
    bool,
    crate::types::ProbeCache,
)> {
```

New:
```rust
) -> anyhow::Result<(
    String,
    Option<LabelInfo>,
    Vec<Playlist>,
    bool,
    crate::types::ProbeCache,
    u32,
    std::collections::HashSet<String>,
)> {
```

Update `scan_disc` internals: change line 911 from `_skip_set` to `skip_set`:
```rust
    let (playlists, probe_cache, skip_set) = crate::media::scan_playlists_with_progress(
```

Update the return statement (line 949):

Old:
```rust
    Ok((label, label_info, playlists, movie_mode, probe_cache))
```

New:
```rust
    Ok((label, label_info, playlists, movie_mode, probe_cache, min_probe_duration, skip_set))
```

Update the caller at line 391-392:

Old:
```rust
    let (label, label_info, mut all_playlists, mut movie_mode, probe_cache) =
        scan_disc(args, config)?;
```

New:
```rust
    let (label, label_info, mut all_playlists, mut movie_mode, probe_cache, min_probe_duration, skip_set) =
        scan_disc(args, config)?;
```

- [ ] **Step 3: Update `run()` episodes_pl filtering at line 394-399**

Old:
```rust
    // Build episodes_pl from probe_cache membership (probed playlists in disc order)
    let mut episodes_pl: Vec<Playlist> = all_playlists
        .iter()
        .filter(|pl| probe_cache.contains_key(&pl.num))
        .cloned()
        .collect();
```

New:
```rust
    // Build episodes_pl from duration threshold (episode-length playlists in disc order)
    let mut episodes_pl: Vec<Playlist> = all_playlists
        .iter()
        .filter(|pl| pl.seconds >= min_probe_duration && !skip_set.contains(&pl.num))
        .cloned()
        .collect();
```

- [ ] **Step 4: Update detection filtering at lines 479-481**

Old:
```rust
            let probed_playlists: Vec<&Playlist> = all_playlists
                .iter()
                .filter(|pl| probe_cache.contains_key(&pl.num))
                .collect();
```

New:
```rust
            let probed_playlists: Vec<&Playlist> = all_playlists
                .iter()
                .filter(|pl| pl.seconds >= min_probe_duration && !skip_set.contains(&pl.num))
                .collect();
```

- [ ] **Step 5: Update detection filtering at lines 578-580**

Old:
```rust
            let probed_playlists: Vec<Playlist> = all_playlists
                .iter()
                .filter(|pl| probe_cache.contains_key(&pl.num))
                .cloned()
                .collect();
```

New:
```rust
            let probed_playlists: Vec<Playlist> = all_playlists
                .iter()
                .filter(|pl| pl.seconds >= min_probe_duration && !skip_set.contains(&pl.num))
                .cloned()
                .collect();
```

- [ ] **Step 6: Update `list_playlists()` detection filtering at line 166-168**

Old:
```rust
        let probed_playlists: Vec<Playlist> = playlists
            .iter()
            .filter(|pl| probe_cache.contains_key(&pl.num))
            .cloned()
            .collect();
```

New:
```rust
        let probed_playlists: Vec<Playlist> = playlists
            .iter()
            .filter(|pl| pl.seconds >= min_probe_duration && !skip_set.contains(&pl.num))
            .cloned()
            .collect();
```

- [ ] **Step 7: Verify it compiles and tests pass**

Run: `cargo build && cargo test && cargo clippy -- -D warnings`
Expected: All pass

- [ ] **Step 8: Commit**

```
fix: replace probe_cache.contains_key() with duration threshold in CLI
```

---

### Task 6: Add on-demand probing fallback in `rip_selected()`

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Update stream info lookups in `rip_selected()`**

At lines 1607-1610, 1624-1627, and 1642-1646, the code does `probe_cache.get(&pl.num).map(|(_, si)| si.clone()).unwrap_or_default()`. Add on-demand probe fallback.

Replace line 1604-1610:

Old:
```rust
        // Resolve stream selection per-playlist
        let stream_selection = if let Some(tracks) = tracks_spec {
            let stream_info = probe_cache
                .get(&pl.num)
                .map(|(_, si)| si.clone())
                .unwrap_or_default();
```

New:
```rust
        // Resolve stream selection per-playlist.
        // Probe on demand if not cached — this opens the device once per playlist,
        // naturally spaced by minutes of remux time.
        let on_demand = probe_cache
            .get(&pl.num)
            .cloned()
            .or_else(|| crate::media::probe::probe_playlist(&device, &pl.num).ok());

        let stream_selection = if let Some(tracks) = tracks_spec {
            let stream_info = on_demand
                .as_ref()
                .map(|(_, si)| si.clone())
                .unwrap_or_default();
```

Replace line 1623-1627:

Old:
```rust
        } else if !stream_filter.is_empty() {
            let stream_info = probe_cache
                .get(&pl.num)
                .map(|(_, si)| si.clone())
                .unwrap_or_default();
```

New:
```rust
        } else if !stream_filter.is_empty() {
            let stream_info = on_demand
                .as_ref()
                .map(|(_, si)| si.clone())
                .unwrap_or_default();
```

Replace line 1641-1646:

Old:
```rust
            crate::media::StreamSelection::Manual(indices) => {
                let stream_info = probe_cache
                    .get(&pl.num)
                    .map(|(_, si)| si)
                    .cloned()
                    .unwrap_or_default();
```

New:
```rust
            crate::media::StreamSelection::Manual(indices) => {
                let stream_info = on_demand
                    .as_ref()
                    .map(|(_, si)| si.clone())
                    .unwrap_or_default();
```

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo build && cargo test && cargo clippy -- -D warnings`
Expected: All pass

- [ ] **Step 3: Commit**

```
fix: add on-demand probe fallback in rip_selected() for stream selection
```

---

### Task 7: Format and final verification

**Files:** All modified files

- [ ] **Step 1: Format code**

Run: `rustup run stable cargo fmt`

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All 625+ tests pass

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 4: Commit any formatting changes**

```
style: apply cargo fmt
```
