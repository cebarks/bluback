# Probe Caching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce disc spin-ups during scan by probing only filtered playlists and caching results for all downstream consumers.

**Architecture:** `scan_playlists_with_progress` gains a `min_duration` parameter and returns a probe cache (`HashMap<String, (MediaInfo, StreamInfo)>`) alongside playlists. Filtered playlists are fully probed during scan; unfiltered playlists get stream counts later via background probe. All CLI/TUI consumers use the cache instead of re-opening the device.

**Tech Stack:** Rust, `ffmpeg-the-third`, `std::collections::HashMap`, `std::sync::mpsc`

**Spec:** `docs/superpowers/specs/2026-04-03-probe-caching-design.md`

---

### Task 1: Change `scan_playlists_with_progress` signature and replace `count_streams`

**Files:**
- Modify: `src/media/probe.rs:50-126`
- Modify: `src/media/mod.rs:7`

- [ ] **Step 1: Update the function signature**

In `src/media/probe.rs`, change the signature to accept `min_duration` and return a probe cache:

```rust
// src/media/probe.rs — replace lines 83-86
pub fn scan_playlists_with_progress(
    device: &str,
    min_duration: u32,
    on_progress: Option<&dyn Fn(u64, u64)>,
) -> Result<(Vec<Playlist>, HashMap<String, (MediaInfo, StreamInfo)>), MediaError> {
```

Add the import at the top of the file (after the existing `use` block around line 10):

```rust
use std::collections::HashMap;
use crate::types::{MediaInfo, StreamInfo};
```

Note: `MediaInfo` and `StreamInfo` are already imported via `use crate::types::{MediaInfo, Playlist, StreamInfo};` — just verify `StreamInfo` is included. If only `MediaInfo` and `Playlist` are imported, add `StreamInfo`.

- [ ] **Step 2: Replace the count_streams loop with probe_playlist for filtered playlists**

Replace lines 102-125 (the scan_error early return through the count_streams loop and log line) with:

```rust
    if let Some(err_msg) = scan_error {
        if !playlists.is_empty() {
            log::info!("Scan complete: found {} playlists", playlists.len());
            return Ok((playlists, HashMap::new()));
        }
        return Err(MediaError::AacsAuthFailed(err_msg));
    }

    // Probe filtered playlists (>= min_duration) for full stream info.
    // Unfiltered playlists are left with 0 stream counts — populated later
    // by background probe or on-demand.
    let mut probe_cache: HashMap<String, (MediaInfo, StreamInfo)> = HashMap::new();
    for pl in &mut playlists {
        if pl.seconds >= min_duration {
            match probe_playlist(device, &pl.num) {
                Ok((media, ref streams)) => {
                    pl.video_streams = streams.video_streams.len() as u32;
                    pl.audio_streams = streams.audio_streams.len() as u32;
                    pl.subtitle_streams = streams.subtitle_streams.len() as u32;
                    probe_cache.insert(pl.num.clone(), (media, streams.clone()));
                }
                Err(e) => {
                    log::warn!("Failed to probe playlist {}: {}", pl.num, e);
                    // Leave stream counts at 0 — same as old count_streams error fallback
                }
            }
        }
    }

    crate::aacs::kill_makemkvcon_children();

    log::info!("Scan complete: found {} playlists", playlists.len());
    Ok((playlists, probe_cache))
```

- [ ] **Step 3: Delete the `count_streams` function**

Remove the `count_streams` function (lines 50-69). It's no longer called anywhere.

- [ ] **Step 4: Update the re-export in `src/media/mod.rs`**

No change needed — line 7 re-exports `scan_playlists_with_progress` by name, and the signature change is compatible.

- [ ] **Step 5: Verify it compiles (expect errors from callers)**

Run: `cargo check 2>&1 | head -30`

Expected: Compile errors in `cli.rs` and `session.rs` due to the changed signature and return type. This confirms the function change propagated. These callers are fixed in Tasks 2-4.

---

### Task 2: Update `list_playlists` in CLI (first scan call site)

**Files:**
- Modify: `src/cli.rs:71-149`

- [ ] **Step 1: Update the scan call and destructure the return**

In `src/cli.rs`, replace lines 88-97:

```rust
    eprint!("Scanning disc at {}...", device);
    let (playlists, probe_cache) = crate::media::scan_playlists_with_progress(
        &device,
        config.min_duration(args.min_duration),
        Some(&|elapsed, timeout| {
            eprint!(
                "\rScanning disc at {} (AACS negotiation {}s/{}s)...",
                device, elapsed, timeout
            );
        }),
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;
```

- [ ] **Step 2: Update verbose probe to use cache for filtered playlists**

Replace lines 138-149 (the verbose_info block):

```rust
    // Verbose mode: use cache for filtered playlists, probe remaining
    let verbose_info: Vec<Option<(crate::types::MediaInfo, crate::types::StreamInfo)>> =
        if args.verbose {
            playlists
                .iter()
                .map(|pl| {
                    if let Some(cached) = probe_cache.get(&pl.num) {
                        Some(cached.clone())
                    } else {
                        // Unfiltered playlist — probe on the spot for verbose display
                        crate::media::probe::probe_playlist(&device, &pl.num).ok()
                    }
                })
                .collect()
        } else {
            vec![None; playlists.len()]
        };
```

Note: The `Probing streams...` print is removed because filtered playlists are already probed. For verbose mode with many unfiltered playlists, this might still take a moment, but the user explicitly requested verbose output.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -30`

Expected: This call site compiles. Errors remain in `scan_disc` and `session.rs`.

---

### Task 3: Update `scan_disc` and `run` in CLI (second scan call site)

**Files:**
- Modify: `src/cli.rs:245-275, 395-445, 860-877, 1012-1024`

- [ ] **Step 1: Update `scan_disc` to pass min_duration and return probe cache**

Change the `scan_disc` function signature (line 395) and body:

```rust
fn scan_disc(
    args: &Args,
    config: &crate::config::Config,
) -> anyhow::Result<(
    String,
    Option<LabelInfo>,
    Vec<Playlist>,
    bool,
    HashMap<String, (crate::types::MediaInfo, crate::types::StreamInfo)>,
)> {
```

Update the scan call inside `scan_disc` (lines 415-425):

```rust
    eprint!("Scanning disc at {}...", device);
    let (playlists, probe_cache) = crate::media::scan_playlists_with_progress(
        &device,
        config.min_duration(args.min_duration),
        Some(&|elapsed, timeout| {
            eprint!(
                "\rScanning disc at {} (AACS negotiation {}s/{}s)...",
                device, elapsed, timeout
            );
        }),
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;
```

Update the return statement at the end of `scan_disc` (around line 445 — after the `episodes_pl.is_empty()` bail) to include the cache. The current return looks like:

```rust
    Ok((label, label_info, episodes_pl, movie_mode))
```

Keep `playlists` alive (currently it's consumed into `episodes_pl`). The function needs to return `episodes_pl` (not all playlists) since that's what callers use. Change to:

```rust
    Ok((label, label_info, episodes_pl, movie_mode, probe_cache))
```

Note: `episodes_pl` is built from `disc::filter_episodes(&playlists, ...)` which filters by `min_duration`. The probe cache already contains exactly these playlists (plus any that matched the duration threshold). So the cache keys are a superset of `episodes_pl` playlist nums.

- [ ] **Step 2: Update `run()` to destructure the new return**

In `run()` (line 256), update:

```rust
    let (label, label_info, episodes_pl, movie_mode, probe_cache) = scan_disc(args, config)?;
```

- [ ] **Step 3: Update filename building to use cache**

Replace lines 872-877 (the `probe_media_info` call in the `default_names` closure):

```rust
            // Use probe cache instead of re-opening the device
            let media_info = if use_custom_format || is_special {
                probe_cache.get(&pl.num).map(|(m, _)| m.clone())
            } else {
                None
            };
```

- [ ] **Step 4: Update stream filter probe cache to use scan cache**

Replace lines 1012-1024 (the probe_cache block that builds a local cache):

```rust
    // Stream info for selected playlists — use scan cache, no additional device opens
    let stream_probe_cache: &HashMap<String, (crate::types::MediaInfo, crate::types::StreamInfo)> =
        &probe_cache;
```

Then update the reference on line ~1050+ where `probe_cache` was used — it's now `stream_probe_cache`. Actually, the simplest approach: just rename the variable. The existing code references `probe_cache` in the stream selection logic below. Since `probe_cache` from scan already contains all filtered playlists, and selected playlists are always filtered, no additional probing is needed. Remove the local `probe_cache` block entirely and use the `probe_cache` from `scan_disc` directly.

Find the usage of the old local `probe_cache` further down in `run()` (around lines 1050-1070 where stream selection is resolved):

```rust
                    if let Some((_, streams)) = probe_cache.get(&pl.num) {
```

This now references the `probe_cache` from `scan_disc` — which has the same type and same keys. No change needed to the lookup code, just the removal of the local shadowing block.

- [ ] **Step 5: Verify it compiles**

Run: `cargo check 2>&1 | head -30`

Expected: CLI compiles. Errors remain in `session.rs`.

---

### Task 4: Update TUI scan thread and `start_upfront_probe`

**Files:**
- Modify: `src/types.rs:241-242`
- Modify: `src/session.rs:530-559, 561-601, 720-767`

- [ ] **Step 1: Update `BackgroundResult::DiscScan` to carry probe cache**

In `src/types.rs`, change line 242:

```rust
    /// Disc scan completed: (device, label, playlists, probe_cache)
    DiscScan(anyhow::Result<(String, String, Vec<Playlist>, std::collections::HashMap<String, (MediaInfo, StreamInfo)>)>),
```

- [ ] **Step 2: Update the scan thread in `start_disc_scan`**

In `src/session.rs`, update the closure in `start_disc_scan` (lines 590-600). The scan thread needs `min_duration`:

First, capture `min_duration` before the thread spawn (around line 564, after `let max_speed = ...`):

```rust
        let min_duration = self.config.min_duration.unwrap_or(900);
```

Then update the closure (lines 590-600):

```rust
                let result = (|| -> anyhow::Result<(String, String, Vec<Playlist>, std::collections::HashMap<String, (MediaInfo, StreamInfo)>)> {
                    let (playlists, probe_cache) = crate::media::scan_playlists_with_progress(
                        &dev_str,
                        min_duration,
                        Some(&move |elapsed, timeout| {
                            let _ =
                                tx_progress.send(BackgroundResult::ScanProgress(elapsed, timeout));
                        }),
                    )
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                    Ok((dev_str, label, playlists, probe_cache))
                })();
```

Add the necessary import at the top of `session.rs` if not present:

```rust
use crate::types::{MediaInfo, StreamInfo};
```

- [ ] **Step 3: Update `poll_background` to handle the new DiscScan variant**

In `session.rs`, find the `BackgroundResult::DiscScan(Ok(...))` match arm (around line 700-767). Update the destructuring:

```rust
            BackgroundResult::DiscScan(Ok((dev_str, label, playlists, probe_cache))) => {
```

After the existing episode filtering and chapter count extraction (around line 755, after the `self.disc.episodes_pl.is_empty()` check), replace the `self.start_upfront_probe()` call with direct cache population:

```rust
                } else {
                    // Populate wizard state from scan probe cache (no additional device opens)
                    for (num, (media, streams)) in &probe_cache {
                        self.wizard.media_infos.insert(num.clone(), media.clone());
                        self.wizard.stream_infos.insert(num.clone(), streams.clone());
                    }

                    // Background-probe unfiltered playlists for their stream counts
                    self.start_unfiltered_probe(&probe_cache);

                    self.screen = Screen::TmdbSearch;
```

- [ ] **Step 4: Replace `start_upfront_probe` with `start_unfiltered_probe`**

Replace the `start_upfront_probe` function (lines 530-559) with:

```rust
    /// Spawn a background thread to probe stream info for unfiltered playlists
    /// (those not already in the scan probe cache). Populates stream counts
    /// so they're available when the user toggles the 'f' filter.
    fn start_unfiltered_probe(
        &mut self,
        probe_cache: &std::collections::HashMap<String, (crate::types::MediaInfo, crate::types::StreamInfo)>,
    ) {
        let device = self.device.to_string_lossy().to_string();
        let unprobed_nums: Vec<String> = self
            .disc
            .playlists
            .iter()
            .filter(|pl| !probe_cache.contains_key(&pl.num))
            .map(|pl| pl.num.clone())
            .collect();
        if unprobed_nums.is_empty() {
            return;
        }

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name(format!("probe-unfiltered-{}", self.device.display()))
            .spawn(move || {
                let mut results = std::collections::HashMap::new();
                for num in &unprobed_nums {
                    if let Ok((media, streams)) = crate::media::probe::probe_playlist(&device, num)
                    {
                        results.insert(num.clone(), (media, streams));
                    }
                }
                let _ = tx.send(crate::types::BackgroundResult::BulkProbe(results));
            })
            .expect("failed to spawn unfiltered probe thread");

        self.probe_rx = Some(rx);
    }
```

- [ ] **Step 5: Update `poll_probe` to also update playlist stream counts**

In the `poll_probe` method (lines 492-528), add stream count updates when bulk probe results arrive. After the existing lines that insert into `media_infos` and `stream_infos` (lines 501-503), add:

```rust
                for (num, (media, streams)) in results {
                    self.wizard.media_infos.insert(num.clone(), media);
                    self.wizard.stream_infos.insert(num.clone(), streams.clone());
                    // Update playlist stream counts for unfiltered playlists
                    if let Some(pl) = self.disc.playlists.iter_mut().find(|pl| pl.num == num) {
                        pl.video_streams = streams.video_streams.len() as u32;
                        pl.audio_streams = streams.audio_streams.len() as u32;
                        pl.subtitle_streams = streams.subtitle_streams.len() as u32;
                    }
                    // If this was a lazy probe for the expanded playlist, enter track edit mode
```

Note: The `.find()` is O(n) per result but playlists is small (<100) and this runs once asynchronously. Not worth optimizing.

- [ ] **Step 6: Verify it compiles**

Run: `cargo check 2>&1`

Expected: Clean compile, no errors.

---

### Task 5: Run tests and fix any breakage

**Files:**
- Possibly modify: `src/media/probe.rs`, `src/session.rs`, `src/cli.rs` (test modules)

- [ ] **Step 1: Run the full test suite**

Run: `cargo test 2>&1 | tail -30`

Expected: All existing tests pass. The changes are to function signatures and call sites — any tests that call `scan_playlists_with_progress` directly or reference `count_streams` will need updating.

- [ ] **Step 2: Fix any broken tests**

If tests reference `count_streams` (now deleted) or the old `scan_playlists_with_progress` signature, update them. These are likely in `src/media/probe.rs`'s test module. Since `count_streams` was a private function, it's unlikely to have direct tests — but check.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`

Expected: Clean (no new warnings). Watch for:
- Unused imports (if `count_streams` was the only user of something)
- Clone warnings on the probe cache

- [ ] **Step 4: Run formatter**

Run: `rustup run stable cargo fmt`

---

### Task 6: Suggest commit

- [ ] **Step 1: Review the diff**

Run: `git diff --stat`

Expected files changed:
- `src/media/probe.rs` — signature change, count_streams removed, probe loop added
- `src/media/mod.rs` — possibly no change (re-export is by name)
- `src/cli.rs` — three call sites updated
- `src/types.rs` — DiscScan variant updated
- `src/session.rs` — scan thread, poll_background, start_unfiltered_probe

- [ ] **Step 2: Suggest commit message**

Suggest to the user:

```
perf: cache probe results during scan to reduce disc access

Replace per-playlist count_streams() calls with probe_playlist() for
filtered playlists only during disc scan. Return probe cache alongside
playlists so all downstream consumers (filename building, stream
filtering, TUI track view, --list-playlists -v) use cached data
instead of re-opening the device.

Unfiltered playlists are background-probed in TUI mode after the
wizard renders, so stream counts populate transparently.

Typical disc: ~36 device opens reduced to ~11.
```
