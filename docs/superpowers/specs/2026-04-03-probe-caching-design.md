# Probe Caching: Reduce Disc Access During Scan

## Problem

During playlist scanning, `scan_playlists_with_progress` opens the Blu-ray device N+1 times: once for playlist enumeration (log capture), then once per playlist via `count_streams()` to get video/audio/subtitle counts. Each `open_bluray()` call triggers libbluray initialization and AACS key exchange, causing the drive to spin up and down repeatedly. On a disc with 30+ playlists this adds 10-30 seconds of unnecessary I/O.

Downstream consumers (filename building, stream filtering, TUI track view, `--list-playlists -v`) then re-open the device again for the same playlists to get richer probe data (MediaInfo, StreamInfo), compounding the issue.

## Approach

Replace the `count_streams()` loop with `probe_playlist()` calls for **filtered playlists only** (those >= `min_duration`). Return the probe results as a cache alongside the playlist list. All downstream consumers use the cache instead of re-opening the device.

Unfiltered playlists (short trailers, menus) get stream counts populated via a low-priority background probe after the wizard is interactive.

### Device opens before vs after

| Phase | Before | After |
|-------|--------|-------|
| Scan enumeration | 1 | 1 |
| Stream counting/probing | N (all playlists) | F (filtered only, F << N) |
| `--list-playlists -v` | N | N-F (only unprobed playlists) |
| Filename building | 1 per selected | 0 (cache hit) |
| Stream filter probing | 1 per selected | 0 (cache hit) |
| TUI upfront probe | F (background thread) | 0 (populated from cache) |
| TUI `t` key expand | 0-1 (lazy) | 0 for filtered (cache), 0-1 for unfiltered (lazy) |
| Remux | 1 per selected | 1 per selected (unavoidable) |

Typical disc with 30 playlists, 6 filtered, 4 selected: **~36 opens drops to ~11**.

## Changes

### 1. `scan_playlists_with_progress` (media/probe.rs)

**Signature change:**
```rust
// Before
pub fn scan_playlists_with_progress(
    device: &str,
    on_progress: Option<&dyn Fn(u64, u64)>,
) -> Result<Vec<Playlist>, MediaError>

// After
pub fn scan_playlists_with_progress(
    device: &str,
    min_duration: u32,
    on_progress: Option<&dyn Fn(u64, u64)>,
) -> Result<(Vec<Playlist>, HashMap<String, (MediaInfo, StreamInfo)>), MediaError>
```

**Body change:**
- Replace the `count_streams()` loop with:
  ```
  for each playlist where pl.seconds >= min_duration:
      probe_playlist(device, &pl.num) → cache + populate pl.video/audio/subtitle_streams
  for each playlist where pl.seconds < min_duration:
      leave stream counts at 0 (populated later by background probe)
  ```
- `count_streams()` becomes dead code and is removed.
- `kill_makemkvcon_children()` call stays, runs after the probe loop.

### 2. CLI call sites (cli.rs)

**Scan call site** (cli.rs `run()`):
- Destructure return: `let (playlists, probe_cache) = scan_playlists_with_progress(..., min_duration, ...)?;`
- Pass `min_duration` to scan.

**`--list-playlists -v`** (line ~140):
- For filtered playlists: look up from `probe_cache`.
- For unfiltered playlists: probe on the spot (explicit verbose request warrants it).

**Filename building** (`build_filenames`, line ~874):
- Replace `disc::probe_media_info(device, &pl.num)` with `probe_cache.get(&pl.num).map(|(m, _)| m.clone())`.
- Selected playlists are always filtered, so cache always has them.

**Stream filter probe cache** (line ~1013):
- Replace the `probe_playlist()` loop with direct cache lookups.
- The `probe_cache` HashMap replaces the locally-built `cache` HashMap.

### 3. TUI call sites

**`start_upfront_probe`** (session.rs:530):
- Instead of spawning a background thread to probe episode playlists, populate `wizard.media_infos` and `wizard.stream_infos` directly from the scan cache after scan completes.
- Optionally spawn a background probe for **unfiltered** playlists only (to populate their stream counts for the `f` toggle view).

**`t` key expand** (wizard.rs:1224):
- Check `wizard.stream_infos` first (populated from cache for filtered playlists).
- For unfiltered playlists not yet in cache, the existing lazy probe fires as today.

### 4. `disc::probe_media_info` (disc.rs:319)

This convenience wrapper calls `probe_playlist()` directly. After the change, CLI callers use the cache instead. The function can remain for any future direct callers but is no longer called in the main workflow.

### 5. Background probe for unfiltered playlists (TUI only)

After scan completes and the wizard renders, optionally spawn a background thread that probes unfiltered playlists for their stream counts. Uses the existing `probe_rx` channel pattern. Results merge into `wizard.stream_infos` and update `Playlist.video/audio/subtitle_streams`.

If the user presses `f` before the background probe finishes, unfiltered playlists show `0/0/0` for stream counts (same as the current error fallback). Once probe results arrive, counts update on the next render frame.

## Edge Cases

- **All playlists below min_duration**: Cache is empty. Playlists show `0/0/0` counts. User can still select them; lazy probe fires on `t` key or during rip setup.
- **`--list-playlists` without `-v`**: Doesn't display stream counts (only #, Playlist, Duration, Ch, Sel columns). No regression.
- **Batch mode**: Cache is fresh per scan. On disc swap, scan runs again with a new cache.
- **`--check`**: Dispatches before scan. No change needed.
- **Scan error with partial playlists**: Current behavior returns early with playlists but no stream counts. Same behavior — cache is empty, early return path unchanged.
- **libmmbd backend**: Fewer `open_bluray` calls means fewer makemkvcon spawns, which reduces the need for `kill_makemkvcon_children()` cleanup. The call stays as a safety net.

## Testing

- Existing `count_streams`-related tests are removed (function deleted).
- `scan_playlists_with_progress` tests (if any) updated for new signature and return type.
- Unit tests for cache population logic (filtered vs unfiltered partitioning).
- Existing probe and remux tests unaffected (they test the probe/remux functions directly, not the scan orchestration).

## Not In Scope

- Caching across batch disc swaps (each scan is independent).
- Persisting probe data to disk.
- Changing the remux path (must open device to read stream data).
