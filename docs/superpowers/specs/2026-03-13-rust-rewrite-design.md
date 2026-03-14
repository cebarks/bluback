# bluback (formerly ripblu) Rust Rewrite — Design Spec

## Summary

Rewrite ripblu as "bluback" in Rust. Rebranded as a Blu-ray backup tool. All existing functionality is preserved. Adds a ratatui TUI as the default interface (wizard for setup, dashboard for ripping) with a plain-text CLI fallback. Single binary, no runtime dependencies beyond ffmpeg/ffprobe and libaacs.

## Why Rust

- Single binary distribution — no Python runtime needed
- Type safety on playlist/episode data (structs vs dicts)
- Better error handling with `Result<T, E>`
- TUI via ratatui (no good Python equivalent without pip dependencies)

## Project Structure

```
Cargo.toml
src/
  main.rs          — entry point, clap parsing, TTY detection, mode dispatch
  disc.rs          — scan_playlists, probe_streams, get_volume_label, parse_volume_label, filter_episodes
  tmdb.rs          — TMDb API client (search, get_season), API key management
  rip.rs           — ffmpeg invocation, progress parsing, format_size, build_map_args
  types.rs         — Playlist, Episode, LabelInfo, StreamInfo, RipProgress structs
  util.rs          — duration_to_seconds, sanitize_filename, parse_selection, guess_start_episode, assign_episodes
  tui/
    mod.rs         — TUI app state, event loop, screen routing
    wizard.rs      — wizard screens (search, show select, season/episode, playlist select, confirm)
    dashboard.rs   — ripping progress dashboard
  cli.rs           — plain-text fallback mode (same behavior as current Python script)
```

## Dependencies

| Crate | Purpose |
|---|---|
| `clap` (derive) | Argument parsing |
| `ratatui` | TUI rendering framework |
| `crossterm` | Terminal backend for ratatui (raw mode, key events, colors) |
| `ureq` | Blocking HTTP client for TMDb API |
| `serde` + `serde_json` | JSON deserialization for TMDb responses |
| `regex` | Volume label parsing, ffprobe output parsing |
| `anyhow` | Application error handling |
| `which` | Check for ffmpeg/ffprobe on PATH |

No async runtime. All I/O is blocking.

## CLI Flags

Clap struct uses `#[command(version)]` to enable `--version` from `Cargo.toml`.

```
ripblu [OPTIONS]

Options:
  -d, --device <PATH>          Blu-ray device path [default: /dev/sr0]
  -o, --output <DIR>           Output directory [default: .]
  -s, --season <NUM>           Season number
  -e, --start-episode <NUM>    Starting episode number
      --min-duration <SECS>    Min seconds for episode detection [default: 900]
      --dry-run                Show what would be ripped
      --no-tui                 Plain text mode (auto if not a TTY)
```

## Mode Selection

1. `--no-tui` explicitly set → plain text mode (`cli.rs`)
2. stdout is not a TTY (piped/redirected) → plain text mode
3. Otherwise → TUI mode (`tui/`)

## Types (`types.rs`)

All types derive `Debug, Clone` at minimum. TMDb response types also derive `Deserialize`.

```rust
struct Playlist {
    num: String,        // e.g., "00003"
    duration: String,   // e.g., "0:43:22"
    seconds: u32,
}

struct Episode {
    episode_number: u32,
    name: String,
    runtime: Option<u32>,  // minutes
}

struct TmdbShow {
    id: u64,
    name: String,
    first_air_date: Option<String>,
}

struct LabelInfo {
    show: String,
    season: u32,
    disc: u32,
}

struct StreamInfo {
    audio_streams: Vec<String>,
    sub_count: u32,
}

struct RipProgress {
    frame: u64,
    fps: f64,
    total_size: u64,      // bytes
    out_time_secs: u32,   // seconds into content
    bitrate: String,
    speed: f64,
}

enum PlaylistStatus {
    Pending,
    Ripping(RipProgress),
    Done(u64),    // final size in bytes
    Failed(String),
}

struct RipJob {
    playlist: Playlist,
    episode: Option<Episode>,
    filename: String,
    status: PlaylistStatus,
}
```

## Core Functions (1:1 with Python)

### `disc.rs`

- `scan_playlists(device: &str) -> Result<Vec<Playlist>>` — run ffprobe, parse playlist lines. Handle `TimeoutExpired` equivalent via `Command::timeout` or spawn + wait_timeout.
- `probe_streams(device: &str, playlist_num: &str) -> Result<Option<StreamInfo>>` — probe a specific playlist for audio/subtitle streams. Returns `None` on failure.
- `get_volume_label(device: &str) -> String` — run `lsblk -no LABEL <device>`. Return empty string on failure.
- `parse_volume_label(label: &str) -> Option<LabelInfo>` — regex parse volume label for show/season/disc.
- `filter_episodes(playlists: &[Playlist], min_duration: u32) -> Vec<&Playlist>` — filter by minimum duration.
- `check_dependencies() -> Result<()>` — verify ffmpeg and ffprobe are on PATH via `which::which`.

### `tmdb.rs`

- `get_api_key() -> Option<String>` — read from `~/.config/ripblu/tmdb_api_key` or `TMDB_API_KEY` env var.
- `save_api_key(key: &str) -> Result<()>` — save to config file with 0o600 permissions.
- `search_show(query: &str, api_key: &str) -> Result<Vec<TmdbShow>>` — search TMDb `/search/tv`.
- `get_season(show_id: u64, season: u32, api_key: &str) -> Result<Vec<Episode>>` — fetch season episodes from TMDb.

Internally uses `ureq::get()` with 10-second timeout. Deserialize with serde into the types above.

### `rip.rs`

- `build_map_args(streams: &StreamInfo) -> Vec<String>` — same audio selection logic: prefer surround, include stereo as secondary, all subtitles.
- `start_rip(device: &str, playlist_num: &str, map_args: &[String], outfile: &Path) -> Result<RipHandle>` — spawn ffmpeg with `-loglevel error -nostats -progress pipe:1`. Returns a handle for reading progress.
- `parse_progress(line: &str, state: &mut HashMap<String, String>) -> Option<RipProgress>` — parse key=value lines from ffmpeg `-progress` output into `RipProgress`. Returns `Some` on `progress=` lines. Note: ffmpeg emits `out_time` as `HH:MM:SS.microseconds` (e.g., `"00:23:45.123456"`), not raw seconds — use `duration_to_seconds` on the truncated value. `total_size` can be negative or non-numeric early in the stream — parse defensively, default to 0.
- `format_size(bytes: u64) -> String` — adaptive B/KiB/MiB/GiB/TiB formatting.

### `util.rs`

- `duration_to_seconds(dur: &str) -> u32`
- `sanitize_filename(name: &str) -> String`
- `parse_selection(text: &str, max_val: usize) -> Option<Vec<usize>>` — returns 0-based indices. Supports single numbers, ranges, open-ended ranges (`3-`), comma-separated, and `"all"`.
- `guess_start_episode(disc_number: Option<u32>, episodes_on_disc: usize) -> u32`
- `assign_episodes(playlists: &[Playlist], episodes: &[Episode], start_episode: u32) -> HashMap<String, Episode>` — sequential assignment by playlist order.

## TUI Mode (`tui/`)

### Architecture

The TUI uses ratatui's immediate-mode rendering pattern:
- `App` struct holds all state (current screen, playlists, episodes, selections, rip jobs)
- Main event loop: read crossterm events → update state → render frame
- Each screen is a function that takes `&App` and renders widgets to a `Frame`

### Wizard Screens (`wizard.rs`)

**Screen 1: Scan & TMDb Search**
- Shows spinner while ffprobe runs (spawned in background thread, main thread renders)
- After scan: displays playlist count and volume label
- Text input widget for TMDb search query (pre-filled from volume label)
- Enter to search, Esc to skip TMDb

**Screen 2: Show Selection**
- Selectable list of TMDb results (up/down arrows, Enter to select)
- Esc to go back to search

**Screen 3: Season & Episode Config**
- Season number text input (pre-filled from label/CLI)
- After entering: fetches season data (spinner), displays episode list
- Starting episode number text input (pre-filled from `guess_start_episode`)
- Enter to confirm, Esc to go back

**Screen 4: Playlist Selection**
- Checklist of episode-length playlists with episode names shown
- All selected by default
- Space to toggle, Enter to confirm
- Default filenames shown — Enter on a row to edit its name
- Esc to go back

**Screen 5: Confirmation**
- Summary table: playlist → filename
- Enter to start ripping, Esc to go back

**Wizard skipping:** CLI flags pre-fill their corresponding inputs. Pre-filled fields are shown as read-only (displayed but not editable). If all inputs for a screen are pre-filled, the screen is skipped entirely. The confirmation screen (Screen 5) always shows.

**Navigation:** Esc goes back one screen. `q` quits the app.

### Ripping Dashboard (`dashboard.rs`)

```
 Ripping: SGU Season 1 (Disc 2)                          3/5 complete

 Playlist   Episode              Status        Size       ETA
 ─────────────────────────────────────────────────────────────────
 00003      S01E06 - Water       ✓ Done        4.2 GiB
 00004      S01E07 - Earth (1)   ✓ Done        4.1 GiB
 00005      S01E08 - Earth (2)   ▓▓▓▓▓▓░░░░    2.1 GiB    3:22
 00006      S01E09 - Life        Pending
 00007      S01E10 - Justice     Pending

 frame=12345  fps=120  bitrate=36.2 Mbps  speed=2.3x

 [q] Abort
```

- Table widget with one row per rip job
- Active rip shows a `Gauge` (progress bar) based on `out_time_secs / total_seconds`
- Bottom status bar shows detailed ffmpeg stats for active rip
- Estimated final size: `current_size / current_time * total_time`
- ETA: `remaining_content_time / speed`
- Completed rows show final size with checkmark
- `q` prompts confirmation before aborting (kills ffmpeg process)
- On all complete: shows summary, waits for any key to exit

**Threading model:** ffmpeg runs in a child process. The main thread runs the ratatui event loop, polling both crossterm events and ffmpeg stdout (via non-blocking reads or a reader thread that sends progress updates through a channel).

## Plain Text Mode (`cli.rs`)

Identical behavior to the current Python script:
- Interactive prompts with validation loops
- Same text output format
- Progress line with `\r` updates (frame, fps, size, time, bitrate, speed, est, eta)
- Defaults: Enter selects first TMDb result, playlist selection defaults to "all", filename confirmation with opt-in customization

## Error Handling

- `anyhow::Result` throughout for application errors
- Missing ffmpeg/ffprobe: check at startup, exit with clear message
- No disc: check device exists as block device, clear error
- TMDb network errors: show in TUI status bar or print in CLI, continue without episode data
- ffmpeg failure mid-rip: mark playlist as `Failed` in dashboard, continue to next
- Ctrl+C / `q`: crossterm restores terminal via drop guard. TUI wrapped in a catch that ensures terminal restoration before propagating errors.

## Dry Run

`--dry-run` runs the full setup flow (TUI wizard or CLI prompts) but skips ffmpeg. Shows a summary of what would be ripped and exits.

## What Gets Deleted

- `ripblu.py`
- `tests/test_ripblu.py`
- `__pycache__/` and `.pytest_cache/` if present

The `docs/superpowers/` directory stays (historical context).

## Testing

Unit tests for pure functions in their respective modules (`#[cfg(test)] mod tests`):
- `util.rs`: `duration_to_seconds`, `sanitize_filename`, `parse_selection`, `guess_start_episode`, `assign_episodes`
- `disc.rs`: `parse_volume_label`, `filter_episodes`
- `rip.rs`: `format_size`, `build_map_args`, `parse_progress`

No tests for subprocess/network/TUI code — tested manually.
