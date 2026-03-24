# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

A Rust CLI/TUI tool for backing up Blu-ray discs to MKV files using FFmpeg library bindings (`ffmpeg-the-third`) + libaacs, with optional TMDb integration for automatic episode naming. Supports both TV shows (sequential or manual episode assignment, including multi-episode playlists) and movies.

## Background & Context

### Why not MakeMKV?

MakeMKV doesn't work with USB Blu-ray drives using ASMedia USB-SATA bridge chips (e.g., ASUS BW-16D1X-U). The bridge mangles SCSI passthrough commands. MakeMKV v1.18.x also has a known bug where it silently fails to open discs on Linux with this drive (shows "No SDF" then exits). v1.17.7 reportedly works. Standard block-level reads via `/dev/sr0` work fine, so we use ffprobe/ffmpeg with libbluray instead.

### How it works

- **FFmpeg API** (via `ffmpeg-the-third` Rust bindings) with libbluray scans disc playlists and remuxes streams into MKV (lossless copy, no re-encoding). No CLI subprocess calls.
- **libaacs** + `~/.config/aacs/KEYDB.cfg` handles AACS decryption
- **Chapters** are embedded during remux via the AVChapter API (no external tools needed)

### Build Requirements

FFmpeg development libraries and clang are required at build time (bindgen generates FFI bindings):

**Fedora/RHEL:** `sudo dnf install ffmpeg-free-devel clang clang-libs pkg-config` (or `ffmpeg-devel` from [RPMFusion](https://rpmfusion.org/) for broader codec support)
**Ubuntu/Debian:** `sudo apt install libavformat-dev libavcodec-dev libavutil-dev libswscale-dev libswresample-dev libavfilter-dev libavdevice-dev pkg-config clang libclang-dev`
**Arch:** `sudo pacman -S ffmpeg clang pkgconf`

### Runtime Requirements

- FFmpeg shared libraries (libavformat, libavcodec, libavutil, etc.) — typically installed with the dev packages above or the `ffmpeg` package
- **libaacs** + **libbluray** — for Blu-ray AACS decryption and playlist enumeration
- `~/.config/aacs/KEYDB.cfg` — containing device keys, processing keys, and/or per-disc VUKs
- A Blu-ray drive accessible as a block device (e.g., `/dev/sr0`)

### AACS Decryption Details

**KEYDB.cfg** is sourced from the [FindVUK Online Database](http://fvonline-db.bplaced.net/) and lives at `~/.config/aacs/KEYDB.cfg`. It contains device keys (DK), processing keys (PK), host certificates (HC), and per-disc Volume Unique Keys (VUK).

**Known limitations with the ASUS BW-16D1X-U (ASMedia USB bridge):**
- The only publicly available AACS host certificate is **revoked in MKBv72+**. Discs with MKBv72+ MKBs require a per-disc VUK in the KEYDB to decrypt.
- MMC SCSI commands (REPORT KEY / SEND KEY) for AACS authentication sometimes fail through the USB bridge. Basic commands like AGID requests work, but host certificate exchange can get I/O errors or authentication failures depending on the disc.
- **libmmbd.so.0** (MakeMKV's libaacs bridge library) must NOT be installed system-wide (`/usr/lib64/libmmbd.so.0`) — if present without a working MakeMKV backend, libaacs hangs indefinitely during AACS init. If ffprobe hangs on disc scan, check for orphaned libmmbd.so.0.
- `scan_playlists` has a 60-second timeout to prevent infinite hangs when AACS fails.

**Recommended drive replacement:** An internal SATA LG WH16NS60 or ASUS BW-16D1HT (same drive without USB enclosure) eliminates all bridge-related issues. The BW-16D1X-U can also be removed from its enclosure and connected directly via SATA.

## Build & Test Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build (binary at target/release/bluback)
cargo test                     # Run all tests
cargo test -- test_name        # Run a single test by name
cargo test -- --test-threads=1 # Run tests sequentially (useful for debugging)
cargo clippy                   # Lint
```

## Architecture

### Data Flow

1. `main.rs` parses CLI args, loads config, sets `BD_DEBUG_MASK` based on `verbose_libbluray`, detects TTY, dispatches to TUI or CLI mode
2. Both modes follow the same workflow: scan disc → filter playlists → TMDb lookup (optional) → assign episodes → review/manual mapping (optional) → build filenames → rip (with chapters embedded during remux)
3. `disc.rs` handles volume label parsing, disc mount/unmount operations (via `udisksctl`), and delegates to `media` module for probing
4. `media/probe.rs` — FFmpeg API-based playlist scanning (custom log callback captures libbluray output), stream probing, and media info extraction
5. `media/remux.rs` — FFmpeg API-based lossless remux with progress callbacks, stream selection, and AVChapter injection from MPLS data
6. `media/error.rs` — MediaError enum with AACS error classification
7. `rip.rs` — orchestrates remux jobs with progress tracking via mpsc channel
8. `util.rs` contains all pure functions (filename generation, template rendering, selection parsing)
9. `config.rs` loads TOML config with path resolution (`--config` flag → `BLUBACK_CONFIG` env → default), saves with commented-out defaults, resolves filename format priority

### Two UI Modes

- **TUI mode** (default when stdout is TTY): ratatui wizard (5 screens in TV mode, 4 in movie mode, in `tui/wizard.rs`) → progress dashboard (`tui/dashboard.rs`). State machine in `tui/mod.rs`. App struct decomposed into `DiscState`, `TmdbState`, `WizardState`, `RipState` sub-structs. Settings overlay (`tui/settings.rs`) accessible via `Ctrl+S` from any screen, rendered on top of the current screen via `App.overlay: Option<Overlay>`.
- **CLI mode** (`--no-tui` or non-TTY): plain-text interactive prompts in `cli.rs`. Supports headless operation via `--yes`/`-y` (auto-enabled when stdin is not a TTY). `--title`, `--year`, `--playlists`, `--list-playlists` flags enable fully scripted workflows.

Both modes use the same underlying disc/rip/tmdb/util functions.

### TUI Screen Flow

- **TV mode**: Scanning → TMDb Search (inline results) → Season → Playlist Manager → Confirm → Ripping → Done
- **Movie mode**: Scanning → TMDb Search (inline results) → Playlist Manager → Confirm → Ripping → Done

The `InputFocus` enum (`TextInput`, `List`, `InlineEdit(usize)`) tracks which UI element has focus, replacing the old `input_active: bool` and `mapping_edit_row: Option<usize>` pattern.

### Filename Format Resolution

Priority chain (highest to lowest): `--format` CLI flag → `--format-preset` CLI flag → `tv_format`/`movie_format` in config → `preset` in config → "default" preset. Templates use `{placeholder}` syntax with bracket groups `[...]` that auto-collapse when contents are empty.

### Key Design Decisions

- **Disc auto-detect** — When no `-d` flag is given, scans `lsblk` for all devices of type `rom` and polls each for a volume label every 2 seconds. The scanning screen shows each tried device as a dimmed log line. The device that has a disc is used for the session. Works on startup and after rescan (Ctrl+R / Enter on Done screen).
- **Blocking I/O** — no async runtime. Remux progress via callback + mpsc channel.
- **Chapter preservation** — During remux, bluback mounts the disc via `udisksctl`, reads MPLS playlist files from `BDMV/PLAYLIST/` to extract chapter marks, and injects them as AVChapter entries in the output MKV. The disc is unmounted afterward if bluback mounted it. Chapter counts are displayed on the playlist selection screen in TUI mode.
- **Stream selection** — Configurable via `stream_selection` in config: `all` (default, maps every stream) or `prefer_surround` (prefers 5.1/7.1, includes stereo as secondary). All subtitle streams always included.
- **MKV index reservation** — `reserve_index_space` config option (default 500 KB) reserves void space after the MKV header for the seek index (Cues) and in-place metadata edits. Cues at the front of the file enable faster seeking over HTTP byte-range requests, and the extra void space allows tools like `mkvpropedit` to update metadata without rewriting the entire file. If the actual Cues exceed the reserved space, they fall back to EOF (standard behavior). Passed to FFmpeg via `write_header_with` dictionary option.
- **libbluray stderr suppression** — `BD_DEBUG_MASK=0` set by default to prevent libbluray debug output from corrupting TUI. Controlled by `verbose_libbluray` config option.
- **Episode assignment** — Default: sequential with multi-episode detection (uses median playlist duration with 1.5x threshold to detect double-episode playlists). Volume label parsing guesses the starting episode from disc number. The Playlist Manager screen allows overriding individual playlist assignments inline (`e` hotkey), including assigning multiple episodes to a single playlist (e.g., `3-4` or `3,5`). Multi-episode playlists produce range-style filenames like `S01E03-E04_Title.mkv`. The `EpisodeAssignments` type is `HashMap<String, Vec<Episode>>` — each playlist maps to zero or more episodes.
- **Specials support** — Playlists can be marked as specials (`s` hotkey in Playlist Manager), which assigns them season 0 episode numbers and uses a separate `special_format` naming template. `r` resets a single row's assignment, `R` resets all.
- **All playlists visible** — The Playlist Manager shows all disc playlists, not just episode-length ones. Filtered playlists (below `min_duration`) are hidden by default but can be toggled with `f`. Controlled by `show_filtered` config option.
- **TMDb API key**: looked up from config TOML → flat file `~/.config/bluback/tmdb_api_key` → `TMDB_API_KEY` env var.
- **Settings overlay** — `App.overlay: Option<Overlay>` renders on top of the current screen. When active, all global key handlers except `Ctrl+C` are blocked; input routes to the overlay handler. `SettingsState` holds typed `SettingItem` variants (Toggle, Choice, Text, Number, Separator, Action). Choice variant has optional `custom_value` for the "Custom..." option (used by device dropdown). `Ctrl+S` in the overlay saves to `config.toml` with commented-out defaults and triggers workflow reset (rescan) unless mid-rip. Toggle/Choice changes apply to the session immediately without saving.
- **Config path resolution** — Priority: `--config` CLI flag → `BLUBACK_CONFIG` env var → `~/.config/bluback/config.toml`. The resolved path is stored as `config_path: PathBuf` on `App`.
- **Environment variable overrides** — On settings panel open, `BLUBACK_*` env vars are detected and applied to settings items. The import notification persists until user input. On save, a warning notes which env vars will override the config file. Supported: `BLUBACK_OUTPUT_DIR`, `BLUBACK_DEVICE`, `BLUBACK_EJECT`, `BLUBACK_MAX_SPEED`, `BLUBACK_MIN_DURATION`, `BLUBACK_PRESET`, `BLUBACK_TV_FORMAT`, `BLUBACK_MOVIE_FORMAT`, `BLUBACK_SPECIAL_FORMAT`, `BLUBACK_SHOW_FILTERED`, `BLUBACK_VERBOSE_LIBBLURAY`, `BLUBACK_RESERVE_INDEX_SPACE`, `TMDB_API_KEY`.
- **`--settings` standalone mode** — Opens settings panel without disc detection or dependency checks. Dirty close prompts to save. Exits after panel close.

## Testing

Unit tests live in `#[cfg(test)] mod tests` blocks within each module. All tests are for pure functions — no tests require hardware or network access.

- `util.rs` — duration parsing, filename sanitization, selection parsing, episode input parsing, episode assignment, multi-episode filename rendering, template rendering
- `disc.rs` — volume label parsing, playlist filtering
- `media/probe.rs` — HDR classification, channel layout formatting, framerate/aspect ratio formatting, playlist log line parsing, GCD, DTS profile formatting
- `media/remux.rs` — stream selection logic (all, prefer_surround, manual), map arg building, progress line parsing, size/ETA estimation, chapter OGM formatting
- `config.rs` — TOML parsing, format resolution priority chain, config path resolution, save/load roundtrip, commented-defaults output
- `types.rs` — MediaInfo field mapping, ChapterMark struct, SettingsState construction/roundtrip, cursor navigation, env var overrides
- `tui/settings.rs` — truncate/mask helpers, input handling (toggle, choice, text edit, number validation, cursor movement, confirm close prompt)

## CLI Flags

```
bluback [OPTIONS]
  -d, --device <PATH>          Blu-ray device [default: auto-detect]
  -o, --output <DIR>           Output directory [default: .]
  -s, --season <NUM>           Season number (skips prompt)
  -e, --start-episode <NUM>    Starting episode number (skips prompt)
      --min-duration <SECS>    Min seconds for episode detection [default: 900]
      --movie                  Movie mode (skip episode assignment)
      --format <TEMPLATE>      Custom filename template
      --format-preset <NAME>   Built-in preset: default, plex, jellyfin
      --dry-run                Show what would be ripped
      --no-tui                 Plain text mode
      --eject                  Eject disc after successful rip
      --no-eject               Don't eject disc after rip (overrides config)
      --no-max-speed           Don't set drive to maximum read speed
  -y, --yes                    Accept all defaults without prompting (auto if stdin not a TTY)
      --title <STRING>         Set show/movie title directly (skips TMDb)
      --year <STRING>          Movie release year (with --title in --movie mode)
      --playlists <SEL>        Select specific playlists (e.g. 1,2,3 or 1-3 or all)
      --list-playlists         Print playlist info and exit
      --settings               Open settings panel (no disc/ffmpeg required)
      --config <PATH>          Path to config file (also: BLUBACK_CONFIG env var)
```

`--format` and `--format-preset` are mutually exclusive (clap argument group).
`--yes` auto-enables when stdin is not a TTY (headless/scripted contexts).

## TUI Keybindings

**Global (all screens):**
- `Ctrl+S` — Open settings panel (overlay; all other globals blocked while open)
- `Ctrl+R` — Rescan disc and restart wizard (confirms first during ripping)
- `Ctrl+E` — Eject disc
- `Ctrl+C` — Quit immediately
- `q` — Quit (except during text input or ripping)

**TMDb Search screen:**
- `Enter` — Search (in input) / Select (in results)
- `Up/Down` — Navigate between input and results
- `Tab` — Toggle Movie/TV mode
- `Esc` — Skip TMDb

**Season screen (TV mode):**
- `Enter` — Fetch episodes / Confirm and proceed
- `Up/Down` — Scroll episode list
- `Esc` — Go back to TMDb Search

**Playlist Manager:**
- `Space` — Toggle playlist selection
- `e` — Edit episode assignment inline (format: `3`, `3-4`, or `3,5`)
- `s` — Toggle special (season 0) marking (TV mode only)
- `r` — Reset current row's assignment
- `R` — Reset all episode assignments
- `f` — Show/hide filtered (short) playlists
- `Enter` — Confirm and proceed
- `Esc` — Go back

**Ripping dashboard:**
- `q` — Abort (with confirmation)

**Done screen:**
- `Enter` — Rescan disc and restart wizard
- Auto-detects new disc with popup prompt
- Any other key — Exit

**Settings panel (overlay):**
- `Up/Down` — Navigate settings (skips separators)
- `Enter/Space` — Toggle (bool), cycle (choice), enter edit (text/number), save (action)
- `Left/Right` — Cycle choice backward/forward
- `Esc` — Cancel edit (if editing), otherwise close panel
- `Ctrl+S` — Save to config (confirms edit first if editing)

## Dependencies

| Crate | Purpose |
|---|---|
| `ffmpeg-the-third` | FFmpeg library bindings (probe, remux, log capture) |
| `clap` (derive) | Argument parsing |
| `ratatui` + `crossterm` | TUI framework + terminal backend |
| `ureq` | Blocking HTTP for TMDb API |
| `serde` + `serde_json` | TMDb JSON deserialization |
| `toml` | Config file parsing |
| `regex` | Volume label parsing, log line parsing |
| `anyhow` | Application error handling |
| `mpls` | MPLS playlist parsing for chapter extraction |
