# Pure Rust FFmpeg Integration Design

**Date:** 2026-03-22
**Status:** Draft
**Branch:** TBD (separate branch + PR)

## Goal

Replace all external CLI tool dependencies (ffprobe, ffmpeg, mkvpropedit) with FFmpeg library bindings via `ffmpeg-the-third`, eliminating fragile process spawning and text parsing in favor of direct API access with typed errors.

### Motivation

- **Reduce external binary dependencies** — eliminate ffmpeg, ffprobe, mkvpropedit on PATH
- **Improve reliability** — typed errors replace stderr/JSON/regex parsing
- **Enable new capabilities** — single-pass chapter injection, direct progress callbacks, better stream metadata access
- **Pure Rust where feasible** — FFmpeg system libraries still required (libbluray/libaacs chain makes pure Rust impossible for disc reading), but no CLI tools on PATH

### Non-Goals

- Pure Rust / zero C dependencies (not feasible due to libbluray + libaacs)
- Static linking of FFmpeg (over-engineering for the target audience)
- Replacing platform tools (lsblk, udisksctl, eject, findmnt)
- Replacing the `mpls` crate for MPLS chapter extraction

## Approach

**Full FFmpeg bindings via `ffmpeg-the-third`** (dynamic linking). Chosen over:

- **Hybrid (bindings for probe, CLI for remux)** — half-measures, still shells out for the main operation
- **Pure Rust media layer** — AACS decryption has no pure Rust path; over-engineering

Fallback: if `bluray:` protocol doesn't work through Rust bindings, degrade to hybrid approach (probe via bindings, remux via CLI).

## Module Structure

New `src/media/` module with a clean, bluback-agnostic API designed for eventual extraction as a shared crate (for `~/code/media-tools` reuse).

```
src/media/
  mod.rs          — public API re-exports
  probe.rs        — playlist scanning, stream probing, media info extraction
  remux.rs        — lossless remux with chapter injection and progress callbacks
  error.rs        — media-specific error types
```

### Public API

```rust
// probe.rs
pub fn scan_playlists(device: &str) -> Result<Vec<Playlist>, MediaError>;
pub fn probe_streams(device: &str, playlist: &str) -> Result<StreamInfo, MediaError>;
pub fn probe_media_info(device: &str, playlist: &str) -> Result<MediaInfo, MediaError>;

// remux.rs
pub enum StreamSelection {
    All,              // map all video, audio, subtitle streams (default)
    PreferSurround,   // surround first, stereo secondary, all video + subs
    Manual(Vec<usize>), // explicit stream indices (future extensibility)
}

pub struct RemuxOptions {
    pub device: String,
    pub playlist: String,
    pub output: PathBuf,
    pub chapters: Vec<ChapterMark>,
    pub stream_selection: StreamSelection,
}

pub fn remux<F>(options: RemuxOptions, on_progress: F) -> Result<(), MediaError>
where
    F: FnMut(RipProgress) + Send;

// error.rs
pub enum MediaError {
    AacsRevoked,
    AacsAuthFailed(String),
    AacsTimeout,
    DeviceNotFound(String),
    NoDisc,
    PlaylistNotFound(String),
    NoStreams,
    RemuxFailed(String),
    OutputExists(PathBuf),
    Cancelled,
    Ffmpeg(ffmpeg::Error),
    Io(std::io::Error),
}
```

### Design Constraints

- Shared types (`Playlist`, `MediaInfo`, `ChapterMark`, `RipProgress`) stay in `src/types.rs` — used across codebase, not FFmpeg-specific
- Media-specific types (`StreamSelection`, `RemuxOptions`) live in `src/media/` — they're part of the media API boundary
- No `async` — stays blocking, consistent with rest of codebase
- No bluback-specific types (`App`, TUI state) in the media module — clean boundary for future extraction
- `AtomicBool`-based cancellation instead of process killing

## Probe Implementation

All three probe functions use `ffmpeg-the-third`'s `format::input()` to open `bluray:{device}` and read metadata directly from FFmpeg structs.

### `scan_playlists()`

**Replaces:** regex parsing of ffprobe text stderr (`playlist (\d+)\.mpls \((\d+:\d+:\d+)\)`)

Open `bluray:{device}` with libavformat, iterate programs/titles. FFmpeg's libbluray integration exposes playlists as programs. Duration read from `AVStream.duration` or `AVFormatContext.duration`.

60-second timeout retained via `std::thread::spawn` + `recv_timeout` — AACS hangs happen in libbluray before FFmpeg returns.

### `probe_streams()`

**Replaces:** text-based stream line scanning with regex

Open with `-playlist` option, iterate `streams()`. Check `parameters().medium()` for Audio/Subtitle. Direct access to channel layout, codec ID, language tag — no text parsing.

Returns a new `StreamInfo` with typed metadata instead of raw text lines:

```rust
pub struct StreamInfo {
    pub audio_streams: Vec<AudioStream>,
    pub subtitle_count: u32,
}

pub struct AudioStream {
    pub index: usize,
    pub codec: String,        // e.g., "truehd", "dts"
    pub channels: u16,        // e.g., 6 for 5.1
    pub channel_layout: String, // e.g., "5.1(side)"
    pub language: Option<String>,
    pub profile: Option<String>, // e.g., "DTS-HD MA"
}
```

This replaces the current `StreamInfo { audio_streams: Vec<String>, sub_count: u32 }` which stores raw ffprobe text lines.

### `probe_media_info()`

**Replaces:** `probe_media_info()` and its ~190-line `parse_media_info_json()` helper

Direct struct access:
- Video: `codec().name()`, `width()`, `height()`, `aspect_ratio()`, `frame_rate()`
- HDR: `color_transfer_characteristic()` enum (SMPTE2084, ARIB_STD_B67, etc.) + side_data for DV/HDR10+
- Audio: `codec().name()`, `channel_layout()`, `channels()`, language metadata
- Bitrate: `AVFormatContext.bit_rate` — direct integer

**Risk:** `AVStream.side_data` for HDR sub-type detection (HDR10 vs DV vs HDR10+) may not be exposed in the safe API. Scoped `unsafe` block if needed.

## Remux Implementation

Single function replaces both ffmpeg CLI spawning and mkvpropedit chapter workflow.

### Core Loop

1. Open input context (`bluray:{device}` with `-playlist` option)
2. Create output context (MKV file)
3. Copy stream codec parameters — default maps all streams (`StreamSelection::All`)
4. Inject `AVChapter` entries from `Vec<ChapterMark>` (MPLS data)
5. Write header
6. Read packets, rescale timestamps, write packets (lossless copy)
7. Report progress via `on_progress` callback
8. Write trailer

### Stream Selection

Default: `StreamSelection::All` — equivalent to `ffmpeg -map 0 -c copy`. All video, audio, and subtitle streams preserved.

**Behavioral change from current bluback** which defaults to surround-preferred audio selection. `PreferSurround` stays available as opt-in via config key `stream_selection` (values: `"all"`, `"prefer_surround"`). Existing config files without this key get the new `All` default. Users who want the old behavior add `stream_selection = "prefer_surround"` to their config. Output files will be larger by default (all audio tracks preserved). This should be noted in the PR description.

### Chapter Injection

Before writing output header (step 5), convert `Vec<ChapterMark>` to `AVChapter` entries:
- `time_base`: `{1, 1000}` (millisecond precision)
- `start`: `start_secs * 1000`
- `end`: next chapter start (or stream duration for last)
- `metadata`: "Chapter N" title

**Eliminates:** `apply_chapters()`, `chapters_to_ogm()`, `format_chapter_time()`, temporary `.chapters.txt` files, mkvpropedit dependency.

### Progress

Direct access to packet data replaces reader thread + text parsing:
- `packet.pts()` / `packet.dts()` → `out_time_secs`
- Accumulated `packet.size()` → `total_size`
- Wall clock → `speed`, `fps`

Callback invoked periodically (every ~100ms or N packets). `RipProgress` struct unchanged — TUI/CLI consumers don't change.

### Cancellation

`AtomicBool` checked each loop iteration. Clean shutdown: write trailer if possible, return `Err(MediaError::Cancelled)`. No orphaned processes.

## Error Handling

`MediaError` enum replaces stderr string parsing with typed, matchable errors.

### AACS Detection

FFmpeg library returns error codes from libbluray/libaacs during `format::input()`. Error string inspection distinguishes:
- Revoked host certificate → `AacsRevoked`
- General auth failure → `AacsAuthFailed`
- Timeout (60s) → `AacsTimeout`

**Known limitation:** Library errors may be less granular than CLI diagnostic messages initially. May need error string matching as a bridge, with iteration to improve.

### Improvements Over Current Approach

- AACS errors caught at open time, not discovered mid-stream
- Cancellation produces valid partial MKV (trailer written) vs potentially corrupt file from `child.kill()`
- Error types are matchable — TUI can show specific guidance per variant
- No "ffprobe exited 0 but printed error to stderr" ambiguity

## Codebase Integration

### Files Modified

| File | Changes |
|---|---|
| `src/main.rs` | Remove `check_dependencies()` call. FFmpeg library availability verified implicitly at first use (library link failure is a build error, not a runtime check). |
| `src/disc.rs` | Remove `scan_playlists`, `probe_streams`, `probe_media_info`, `check_dependencies`, `has_mkvpropedit`, `check_aacs_error`. Replace with `media::` calls. Keep device/mount/eject functions. |
| `src/rip.rs` | Remove `start_rip`, `build_map_args`, `parse_progress_line`. Keep `estimate_final_size`, `estimate_eta`, `format_eta` (these are pure math on `RipProgress` and work identically with the new callback-based progress). |
| `src/chapters.rs` | Remove `apply_chapters`, `chapters_to_ogm`, `format_chapter_time`. Keep `extract_chapters`, `count_chapters_for_playlists`. |
| `src/tui/mod.rs` | Remove `has_mkvpropedit` check, dependency checks. Update `RipState` progress channel setup. |
| `src/tui/dashboard.rs` | Replace reader thread + mpsc + `start_rip` + `build_map_args` + `probe_streams` + `parse_progress_line` + `apply_chapters` calls with `media::remux()` callback that sends `RipProgress` over the existing channel. This is where the bulk of rip-related changes happen. |
| `src/cli.rs` | Replace synchronous line-by-line progress loop (`start_rip` + `parse_progress_line` + `build_map_args` + `probe_streams` + `apply_chapters`) with `media::remux()` call and progress callback that prints to stdout. |
| `src/types.rs` | `StreamInfo` restructured with typed `AudioStream` fields (see Probe section). Rest unchanged. |
| `Cargo.toml` | Add `ffmpeg-the-third`. Remove `which`. |

### Files Created

| File | Purpose |
|---|---|
| `src/media/mod.rs` | Public API re-exports |
| `src/media/probe.rs` | Probe implementations |
| `src/media/remux.rs` | Remux with chapters + progress |
| `src/media/error.rs` | `MediaError` enum + Display/Error impls |

### CI Changes

GitHub Actions workflows (`ci.yml`, `release.yml`) need `ffmpeg-devel` (or equivalent) package installed for build + test. Both workflows build Rust code and will fail to link without FFmpeg development headers.

### Test Changes

**Removed:** `parse_media_info_json` tests (6 tests), `check_aacs_error` tests (4 tests), `build_map_args` tests (3 tests), `parse_progress_line` tests (2 tests), OGM formatting tests (`format_chapter_time`, `chapters_to_ogm`) — all test parsing/classification logic that no longer exists.

**Added:** Probe result mapping tests, stream selection logic tests, chapter-to-AVChapter conversion tests, error type mapping tests.

**Unchanged:** Duration parsing, filename generation, estimation math, volume label parsing, MPLS extraction — all pure functions unrelated to the CLI→library migration.

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| `bluray:` protocol doesn't work through Rust bindings | Medium | First task on branch is a spike to verify. Fallback to hybrid approach (probe via bindings, remux via CLI). |
| `AVStream.side_data` not exposed in safe API (HDR detection) | Medium | Scoped `unsafe` block to access raw pointer. Well-documented, minimal surface. |
| `AVChapter` writing not exposed in safe API | Low-Medium | Drop to `ffmpeg-sys-the-third` for chapter setup. Isolated unsafe block. |
| AACS error classification less precise than stderr parsing | Medium | Error string inspection as bridge. Iterate to improve over time. |
| `ffmpeg-devel` package differences across distros | Low | Document required packages in README. CI validates on Ubuntu (Actions default). |

## Dependencies

| Crate | Version | Purpose | Change |
|---|---|---|---|
| `ffmpeg-the-third` | latest | FFmpeg library bindings | **Added** |
| `which` | — | Binary-on-PATH detection | **Removed** |
| `mpls` | 0.2.0 | MPLS chapter parsing | Unchanged |
| `regex` | — | Volume label parsing | Unchanged (scope reduced) |
| All others | — | — | Unchanged |
