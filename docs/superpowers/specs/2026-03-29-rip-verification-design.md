# Rip Verification Design

## Context

bluback v0.10 — item #19 from the roadmap. Post-remux verification to confirm output MKV files are structurally sound and match source expectations. Motivated by "peace of mind" — a quick confirmation that the rip is good so you can move on without manually spot-checking.

## Design Decisions

- **Two verification levels:** `quick` (header probe, milliseconds) and `full` (header probe + sample frame decode, a few seconds)
- **Off by default**, opt-in via `--verify` / config toggle
- **Percentage-based duration tolerance:** 2% (configurable isn't needed — 2% is generous enough)
- **On failure in TUI:** prompt to delete & retry, keep, or skip
- **On failure in CLI/headless:** log warning, keep file
- **Stream counts added to `Playlist`** during scan phase for comparison

## Verification Levels

### Quick (default when enabled)

1. File exists and size > 0
2. FFmpeg can open the output MKV (`avformat_open_input` + `avformat_find_stream_info`)
3. Duration within 2% of source playlist duration
4. Video stream count matches source
5. Audio stream count matches expected (source count adjusted for `stream_selection` mode)
6. Subtitle stream count matches source
7. Chapter count matches what was injected during remux

### Full (adds to quick)

8. Decode one video frame at each of ~5 evenly-spaced seek points (0%, 25%, 50%, 75%, 90%)
9. Each seek + decode must succeed without error

## Types

### New types in `src/verify.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerifyLevel {
    Quick,
    Full,
}

pub struct VerifyExpected {
    pub duration_secs: u32,
    pub video_streams: u32,
    pub audio_streams: u32,
    pub subtitle_streams: u32,
    pub chapters: usize,
    pub tolerance_pct: f64,    // default 2.0
}

pub struct VerifyResult {
    pub passed: bool,
    pub level: VerifyLevel,
    pub checks: Vec<VerifyCheck>,
}

pub struct VerifyCheck {
    pub name: &'static str,       // e.g. "duration", "audio_streams"
    pub passed: bool,
    pub detail: String,           // e.g. "expected 7201s +/-2%, got 7199s"
}
```

### Extended `Playlist` (in `src/types.rs`)

```rust
pub struct Playlist {
    pub num: String,
    pub duration: String,
    pub seconds: u32,
    pub video_streams: u32,
    pub audio_streams: u32,
    pub subtitle_streams: u32,
}
```

Stream counts populated by `scan_playlists_with_progress` in `probe.rs`, which already opens each playlist's input context.

### Extended `PlaylistStatus` (in `src/types.rs`)

```rust
pub enum PlaylistStatus {
    Pending,
    Ripping(RipProgress),
    Verifying,                              // brief transitional state
    Done(u64),                              // no verification ran
    Verified(u64, VerifyResult),            // verification passed
    VerifyFailed(u64, VerifyResult),        // verification failed
    Skipped(u64),
    Failed(String),
}
```

## New Module: `src/verify.rs`

All verification logic lives here:

- `verify_output(path: &Path, expected: &VerifyExpected, level: VerifyLevel) -> VerifyResult` — top-level entry point
- `probe_output_file(path: &Path) -> Result<OutputInfo, MediaError>` — opens MKV, reads duration/stream counts/chapter count
- `decode_sample_frames(path: &Path, points: &[f64]) -> Vec<VerifyCheck>` — seeks and decodes at each point (full mode only)

`VerifyExpected` is built from `Playlist` fields (stream counts, duration) plus the chapter count returned by `remux()`.

## Configuration

### Config file (`config.toml`)

```toml
verify = false           # off by default
verify_level = "quick"   # "quick" or "full"
```

### CLI flags

- `--verify` — enable verification (uses config level, defaults to `quick`)
- `--verify-level <quick|full>` — set level (implies `--verify`)
- `--no-verify` — disable (overrides config)

### Settings panel

Two new items:
- `Verify rips` — toggle (bool)
- `Verify level` — choice (quick / full)

## Integration Flow

### Current flow (no verification)

```
remux thread finishes -> stat file size -> PlaylistStatus::Done -> run post_rip hook
```

### New flow (verification enabled)

```
remux thread finishes -> stat file size -> PlaylistStatus::Verifying ->
  verify_output() -> PlaylistStatus::Verified or VerifyFailed ->
  run post_rip hook
```

Verification runs synchronously in the remux worker thread after remux completes. This keeps it off the main/UI thread.

### TUI behavior on `VerifyFailed`

Dashboard shows a warning indicator on the row. When all jobs finish (or immediately if it's the last/only one), a prompt appears:

```
S01E03.mkv failed verification: duration 12% short (expected ~2400s, got 2100s)
[D]elete and retry / [K]eep / [S]kip
```

- **Delete and retry** — deletes the file, re-queues the job
- **Keep** — accept as-is, treat as Done
- **Skip** — delete the file, mark as Skipped

### CLI/headless behavior

Log a warning with the check details. No prompt, no auto-delete. File is kept.

### Post-rip hook variables

- `{verify}` — `"passed"`, `"failed"`, or `"skipped"` (when verification is off)
- `{verify_detail}` — comma-separated list of failed check names, empty if passed

## Changes to Existing Files

| File | Change |
|------|--------|
| `src/types.rs` | Add stream counts to `Playlist`, add `Verifying`/`Verified`/`VerifyFailed` to `PlaylistStatus` |
| `src/media/probe.rs` | Count streams by type during `scan_playlists_with_progress` |
| `src/config.rs` | `verify` (bool), `verify_level` (quick/full) fields, CLI flags, KNOWN_KEYS, save/load |
| `src/tui/dashboard.rs` | Call verify after remux, handle `VerifyFailed` prompt, render `Verifying`/`Verified`/`VerifyFailed` states |
| `src/tui/settings.rs` | Two new setting items (toggle + choice) |
| `src/cli.rs` | Call verify after remux, log warnings on failure |
| `src/hooks.rs` | Add `{verify}`, `{verify_detail}` template variables |
| `src/main.rs` | Wire `--verify`, `--verify-level`, `--no-verify` flags |
| `src/session.rs` | Thread verify config into session |

## Testing

Unit tests in `src/verify.rs`:
- `VerifyExpected` construction from playlist + chapter count
- Duration tolerance math (pass/fail boundary cases)
- Stream count comparison logic
- `VerifyResult` aggregation (all pass = passed, any fail = failed)

Integration test with fixture MKVs (from `tests/fixtures/media/`):
- Probe a known-good fixture, verify all checks pass
- Verify duration mismatch detection with wrong expected value
- Verify stream count mismatch detection

No tests require hardware or network access.
