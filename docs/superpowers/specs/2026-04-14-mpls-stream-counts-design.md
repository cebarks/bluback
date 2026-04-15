# MPLS-Based Stream Counts During Scan

**Date:** 2026-04-14
**Status:** Approved
**Branch:** `feature/detection-driven-classification`

## Problem

Since v0.10.0 (commit `5965be5`), `scan_playlists_with_progress()` opens `bluray:{device}` per-playlist during scan to get stream counts. With the libmmbd AACS backend, each open spawns a makemkvcon process, performs AACS authentication via SCSI REPORT KEY/SEND KEY through the USB bridge, reads stream metadata, then the `MakemkvconGuard` SIGKILLs makemkvcon on drop. For a disc with 7+ playlists, this produces 7+ rapid-fire cycles of spawn → SCSI auth → kill with no cooldown.

The ASMedia USB-SATA bridge (used in the ASUS BW-16D1X-U) cannot handle this burst. The SCSI state machine enters a fault state, causing all subsequent AACS negotiations to time out. The failure is intermittent — it depends on bridge firmware timing and thermal state.

In v0.9.2, scan opened the device exactly **once** (in the fork child). Per-playlist probing was lazy — it happened on-demand during user interaction (TUI) or at rip time (CLI), naturally spaced by seconds or minutes.

## Root Cause

The `count_streams()` loop (v0.10.0) and its successor `probe_playlist()` loop (v0.10.3) front-load all per-playlist device opens into a tight loop during scan. Each open triggers a full AACS authentication cycle through the USB bridge. With libmmbd, this also involves makemkvcon process spawn/kill overhead.

## Key Concept: "Has Stream Counts" vs "Has Full Probe"

Currently these are the same thing — both come from `probe_playlist()`. After this change they split:

- **Has stream counts** — populated from MPLS during scan. Available immediately. Used for detection heuristics, TUI/CLI display, and filtering.
- **Has full stream info** — populated from FFmpeg `probe_playlist()` on demand. Used for track picker, stream selection, and format template placeholders (codec, resolution, etc.).

All code that checks `probe_cache.contains_key()` or `stream_infos.contains_key()` as a proxy for "is this an episode-length playlist" must change to use the duration threshold directly, since MPLS reading populates stream counts but NOT the probe cache or stream_infos maps.

## Solution

Replace per-playlist device probing during scan with MPLS file reads. Blu-ray MPLS playlist files contain a `StreamNumberTable` with video, audio, and subtitle stream counts. The `mpls` crate (already a dependency) exposes this as `PlayItem.stream_number_table.{primary_video_streams, primary_audio_streams, primary_pgs_streams}`. Reading MPLS files is plain filesystem I/O — no AACS, no SCSI, no makemkvcon.

This restores v0.9.2's single-device-open scan pattern while keeping stream count data for auto-detection heuristics and TUI/CLI display.

## Changes

### `src/chapters.rs` — New `mpls_stream_counts()` function

Add a function that parses an MPLS file and returns `(video, audio, subtitle)` stream counts from the first PlayItem's `StreamNumberTable`:

```rust
pub fn mpls_stream_counts(mount_point: &Path, playlist_num: &str) -> Option<(u32, u32, u32)>
```

Uses `primary_video_streams.len()`, `primary_audio_streams.len()`, `primary_pgs_streams.len()`. Returns `None` if the MPLS file can't be read or parsed. Uses the first PlayItem (primary angle) — Blu-ray playlists typically have consistent stream tables across PlayItems since they're segments of the same content. This matches bluback's existing remux behavior of using the primary angle.

### `src/media/probe.rs` — `scan_playlists_with_progress()`

Replace the per-playlist `probe_playlist()` loop with MPLS file reads:

1. After the fork-child scan returns playlists with durations (parent process on Linux, same thread on macOS), call `disc::ensure_mounted(device)` to get a mount point.
2. Iterate all playlists, calling `chapters::mpls_stream_counts()` for each to populate `video_streams`, `audio_streams`, `subtitle_streams` on `Playlist` structs.
3. Fire the existing `on_probe_progress` callback per-playlist during MPLS reads.
4. Run `pre_classify_playlists()` if `auto_detect` is true (uses durations + stream counts — same as before).
5. Unmount the disc if we mounted it (track via the `did_we_mount` bool from `ensure_mounted`).
6. Return an **empty** `ProbeCache`. Full `MediaInfo`/`StreamInfo` is populated lazily downstream.

The function signature stays the same:
```rust
pub fn scan_playlists_with_progress(
    device: &str,
    min_probe_duration: u32,
    auto_detect: bool,
    on_progress: Option<&dyn Fn(u64, u64)>,
    on_probe_progress: Option<&dyn Fn(usize, usize, &str)>,
) -> Result<(Vec<Playlist>, ProbeCache, HashSet<String>), MediaError>
```

Parameters `min_probe_duration` and `auto_detect` are still needed for `pre_classify_playlists()`.

**Note on mount lifecycle:** The session.rs caller mounts again after scan (line 823) for chapter counts and title order. This produces a mount → unmount → mount → unmount sequence (two `udisksctl` round-trips). This is acceptable — `udisksctl` calls are instant, and keeping mount ownership clean between scan and session is worth the trivial overhead.

### `src/cli.rs` — Replace `probe_cache` filtering and add on-demand probing

**Filtering changes** — all `probe_cache.contains_key()` usage changes to duration threshold:

- **`scan_disc()` (line 931-944):** `probe_cache.len()` → `playlists.iter().filter(|pl| pl.seconds >= min_probe_duration).count()`. `probe_cache.is_empty()` → same count == 0. `probed_count` for movie mode detection uses the new count.
- **`run()` (line 397):** `episodes_pl` filtering uses `pl.seconds >= min_probe_duration` instead of `probe_cache.contains_key()`.
- **`run()` (line 481):** Detection-aware movie mode override — same change.
- **`run()` (line 580):** Specials auto-detection — same change.
- **`list_playlists()` (line 168):** Detection input filtering — same change.

**On-demand probing at rip time** — `rip_selected()` (lines 1607, 1624, 1642) currently uses `probe_cache.get(&pl.num).unwrap_or_default()` for stream selection. With an empty cache, this falls through to empty `StreamInfo`, causing stream filters/track specs to produce empty indices → `StreamSelection::All` (silently ignoring the user's stream selection config).

Fix: add on-demand `probe_playlist()` fallback in `rip_selected()`, matching the pattern already used in `list_playlists()` (line 219):
```rust
let stream_info = probe_cache
    .get(&pl.num)
    .map(|(_, si)| si.clone())
    .or_else(|| crate::media::probe::probe_playlist(&device, &pl.num).ok().map(|(_, si)| si))
    .unwrap_or_default();
```
This probe happens once per playlist just before remux — naturally spaced by minutes of remux time.

**`build_filenames()` (line 1388):** MediaInfo for format template placeholders (codec, resolution) comes from `probe_cache.get()`. With an empty cache, custom format templates would show un-filled placeholders at confirmation time but work correctly at rip time. This is acceptable — the filename preview is a best-effort display, and format templates that use codec/resolution placeholders are rare. No change needed here.

**`list_playlists --verbose` (line 213-219):** Already has on-demand `probe_playlist()` fallback. With an empty cache, every verbose playlist would be probed on the spot. This is acceptable — `--list-playlists -v` is a deliberate user action, not automatic scan. The user explicitly asks for detailed info.

### `src/session.rs` — Replace `stream_infos` filtering

**`probed_playlists()` (line 340-346):** Currently filters by `stream_infos.contains_key()`. With an empty probe cache, `stream_infos` starts empty → `probed_playlists()` returns nothing → detection runs on empty set, episode count = 0, movie mode detection breaks.

Fix: change `probed_playlists()` to filter by duration threshold instead of `stream_infos` membership. This requires storing `min_probe_duration` on the session (or passing it through). The method returns "episode-length playlists" — those above the threshold and not in the skip set.

**Pre-classified specials logic (lines 881-903):** Currently uses `!self.wizard.stream_infos.contains_key(&pl.num)` to detect playlists that were pre-classified (skipped during probe). With an empty probe cache, this would match ALL playlists above `min_probe_duration`, not just pre-classified ones.

Fix: use the `skip_set` returned from scan explicitly. Store the skip set on the session and check `skip_set.contains(&pl.num)` instead of inferring from `stream_infos` absence. The `_skip_set` currently discarded at line 692 should be preserved.

**Probe cache transfer (lines 859-864):** `for (num, (media, streams)) in &probe_cache` — this loop becomes a no-op with an empty cache. No change needed, it's correct.

**Detection (lines 868-877):** `probed_playlists()` is called to get detection input. With the fix to `probed_playlists()` above, this works correctly — detection receives playlists with MPLS-derived stream counts.

### Changes summary for `BackgroundResult::DiscScan`

The type at `types.rs:253` stays the same: `DiscScan(anyhow::Result<(String, String, Vec<Playlist>, ProbeCache)>)`. The `ProbeCache` is returned empty but the type is still correct. Adding the skip set to the result tuple is needed:

```rust
DiscScan(anyhow::Result<(String, String, Vec<Playlist>, ProbeCache, HashSet<String>)>)
```

The skip set is needed by the pre-classified specials logic in `session.rs`.

### No changes needed

- **`src/detection.rs`** — receives `&[Playlist]` with stream counts already populated. Source of stream counts (MPLS vs device probe) is transparent.
- **`src/types.rs`** — `Playlist` struct, `ProbeCache` type alias unchanged.
- **`start_on_demand_probe()` in `session.rs`** — already handles lazy full probing for TUI track picker.

## Disc Access Pattern (Before → After)

| Phase | v0.10+ (current) | After fix |
|-------|------------------|-----------|
| Scan | 1 device open (fork child) | 1 device open (fork child) |
| Stream counts | N device opens (tight loop) | 0 device opens (MPLS file reads) |
| Pre-classification | Pure math | Pure math |
| User interaction | 0-N on-demand probes (TUI) | 0-N on-demand probes (TUI) |
| Rip | 1 device open per playlist | 1-2 device opens per playlist (probe + remux) |
| **Total scan-phase opens** | **N+1** | **1** |

Note: rip-phase opens increase from 1 to 1-2 per playlist (on-demand probe + remux). But these are spaced by minutes of remux time, well within the USB bridge's tolerance.

## Edge Cases

- **Mount fails during scan**: Log warning, leave stream counts at 0 for all playlists (same as current probe failure behavior). Detection heuristics and display degrade gracefully with zero counts.
- **MPLS parse fails for a playlist**: Log warning, leave stream counts at 0 for that playlist. Other playlists unaffected.
- **Directory device paths** (test fixtures): `ensure_mounted()` already handles these — returns the directory as mount point if `BDMV/` subdirectory exists.
- **macOS**: Same approach works. No fork on macOS — the mount and MPLS reads happen in the same thread. MPLS files are on the mounted disc filesystem regardless of platform.
- **Multi-PlayItem playlists**: Uses the first PlayItem's `StreamNumberTable`. Blu-ray playlists have consistent stream tables across PlayItems (they're segments of the same content). A comment in the implementation should note this assumption.

## Testing

- Add unit test for `mpls_stream_counts()` — test with missing path, missing playlist, verify it returns `None` gracefully (matching existing `parse_mpls_info` test patterns).
- Existing detection tests use `Playlist` structs with hardcoded stream counts — unaffected by the change in data source.
- Update any tests that assert on `ProbeCache` contents from scan — the cache will now be empty after scan.
- Existing integration tests (`tests/cli_batch_conflicts.rs`, `tests/cli_flag_conflicts.rs`) don't exercise disc scanning — unaffected.
- Test that `probed_playlists()` returns correct results with the duration-threshold approach.
