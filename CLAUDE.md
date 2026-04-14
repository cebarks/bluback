# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

A Rust CLI/TUI tool for backing up Blu-ray discs to MKV files using FFmpeg library bindings (`ffmpeg-the-third`) + libaacs, with optional TMDb integration for automatic episode naming. Supports both TV shows (sequential or manual episode assignment, including multi-episode playlists) and movies.

## Background & Context

### Why not MakeMKV?

MakeMKV doesn't work with USB Blu-ray drives using ASMedia USB-SATA bridge chips (e.g., ASUS BW-16D1X-U). The bridge mangles SCSI passthrough commands. MakeMKV v1.18.x also has a known bug where it silently fails to open discs on Linux with this drive (shows "No SDF" then exits). v1.17.7 reportedly works. Standard block-level reads via `/dev/sr0` work fine, so we use ffprobe/ffmpeg with libbluray instead.

### How it works

- **FFmpeg API** (via `ffmpeg-the-third` Rust bindings) with libbluray scans disc playlists and remuxes streams into MKV (lossless copy, no re-encoding). No CLI subprocess calls.
- **AACS decryption** via libaacs (KEYDB.cfg) or libmmbd (MakeMKV's LibreDrive backend), controlled by `aacs_backend` config
- **Chapters** are embedded during remux via the AVChapter API (no external tools needed)

### Build Requirements

FFmpeg development libraries and clang are required at build time (bindgen generates FFI bindings):

**Fedora/RHEL:** `sudo dnf install ffmpeg-free-devel clang clang-libs pkg-config` (or `ffmpeg-devel` from [RPMFusion](https://rpmfusion.org/) for broader codec support)
**Ubuntu/Debian:** `sudo apt install libavformat-dev libavcodec-dev libavutil-dev libswscale-dev libswresample-dev libavfilter-dev libavdevice-dev pkg-config clang libclang-dev`
**Arch:** `sudo pacman -S ffmpeg clang pkgconf`
**macOS:** `brew install ffmpeg llvm pkg-config` — Ensure llvm's clang is in PATH: `export PATH="/opt/homebrew/opt/llvm/bin:$PATH"`. **CRITICAL:** Homebrew's default FFmpeg does NOT include `--enable-libbluray`. You must patch the formula and rebuild from source (see `docs/macos-installation.md`).

### Runtime Requirements

- FFmpeg shared libraries (libavformat, libavcodec, libavutil, etc.) — typically installed with the dev packages above or the `ffmpeg` package. **macOS:** Must be compiled with `--enable-libbluray` for the `bluray://` protocol.
- **libaacs** + **libbluray** — for Blu-ray AACS decryption and playlist enumeration (macOS: `brew install libaacs libbluray`)
- `~/.config/aacs/KEYDB.cfg` — containing device keys, processing keys, and/or per-disc VUKs
- A Blu-ray drive accessible as a block device:
  - Linux: `/dev/sr0`, `/dev/sr1`, etc.
  - macOS: `/dev/disk2`, `/dev/disk3`, etc. (find with `diskutil list`)
- Platform-specific tools:
  - Linux: `udisksctl` (from `udisks2`), `eject`
  - macOS: `diskutil`, `drutil` (both built-in)
- **macOS dlopen workaround:** libbluray loads libaacs/libmmbd at runtime via `dlopen()`, but `/opt/homebrew/lib/` is not in macOS's default search path. Symlinks to `/usr/local/lib/` are required (see `docs/macos-installation.md`).

### AACS Decryption Details

**KEYDB.cfg** is sourced from the [FindVUK Online Database](http://fvonline-db.bplaced.net/) and lives at `~/.config/aacs/KEYDB.cfg`. It contains device keys (DK), processing keys (PK), host certificates (HC), and per-disc Volume Unique Keys (VUK).

**AACS backend selection (`aacs_backend` config option):**
- `auto` (default) — let libbluray decide. Preflight warns if libmmbd is masquerading as libaacs without makemkvcon available.
- `libaacs` — force system libaacs with KEYDB.cfg. Requires per-disc VUKs for MKBv72+ discs.
- `libmmbd` — force MakeMKV's libmmbd. Requires `makemkvcon` in PATH and a registered MakeMKV (beta key or purchased — trial version silently fails). Enables LibreDrive mode for drives with patched firmware.
- **CRITICAL:** `LIBAACS_PATH` must be set to a library NAME (`libmmbd`), not a full path. libbluray's `dl_dlopen` appends `.so.{version}`, so a path like `/lib64/libmmbd.so.0` becomes `/lib64/libmmbd.so.0.so.0` and silently fails.
- `scan_playlists` has a 120-second timeout (libmmbd + makemkvcon is slower than direct libaacs due to IPC overhead).
- Preflight checks (`src/aacs.rs`) run before any FFmpeg/libbluray calls: verify makemkvcon availability, detect libmmbd masquerading as libaacs, set LIBAACS_PATH/LIBBDPLUS_PATH env vars.

**Known limitations with the ASUS BW-16D1X-U (ASMedia USB bridge):**
- The only publicly available AACS host certificate is **revoked in MKBv72+**. Discs with MKBv72+ MKBs require a per-disc VUK in the KEYDB or libmmbd with LibreDrive.
- MMC SCSI commands (REPORT KEY / SEND KEY) for AACS authentication sometimes fail through the USB bridge. Using `aacs_backend = "libmmbd"` with LibreDrive firmware bypasses this entirely.

**Recommended drive replacement:** An internal SATA LG WH16NS60 or ASUS BW-16D1HT (same drive without USB enclosure) eliminates all bridge-related issues. The BW-16D1X-U can also be removed from its enclosure and connected directly via SATA.

## Config Fields

- `tmdb_api_key`, `preset`, `tv_format`, `movie_format`, `special_format`
- `eject`, `max_speed`, `min_duration`, `show_filtered`, `verbose_libbluray`
- `output_dir`, `device`, `stream_selection`, `reserve_index_space`
- `aacs_backend` (auto/libaacs/libmmbd), `overwrite`, `auto_detect`
- `[history]` section: `enabled` (default true), `path` (DB location), `retention` (auto-prune duration, e.g. "90d"), `retention_statuses` (array of statuses to prune)

## Build & Test Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build (binary at target/release/bluback)
cargo test                     # Run all tests
cargo test -- test_name        # Run a single test by name
cargo test -- --test-threads=1 # Run tests sequentially (useful for debugging)
cargo clippy                   # Lint
cargo fmt                      # Format code
```

## Pre-Commit Checklist

Before every commit, you MUST run all three of these and verify they pass:
1. `cargo test` — all tests must pass
2. `rustup run stable cargo fmt` — format all code (must use stable toolchain to match CI)
3. `cargo clippy -- -D warnings` — all warnings must be fixed (this matches CI)

## Architecture

### Data Flow

1. `main.rs` parses CLI args, loads config, runs AACS preflight (`aacs.rs`), registers ctrlc signal handler, sets `BD_DEBUG_MASK` based on `verbose_libbluray`, validates config, detects TTY, dispatches to TUI or CLI mode. Structured as `main()` → `run()` → `run_inner()` with exit codes.
2. Both modes follow the same workflow: scan disc → filter playlists → TMDb lookup (optional) → assign episodes → review/manual mapping (optional) → build filenames → rip (with chapters embedded during remux)
3. `disc.rs` handles volume label parsing, disc mount/unmount operations (via `udisksctl`), and delegates to `media` module for probing
4. `media/probe.rs` — FFmpeg API-based playlist scanning (custom log callback captures libbluray output), stream probing, and media info extraction
5. `media/remux.rs` — FFmpeg API-based lossless remux with progress callbacks, stream selection, and AVChapter injection from MPLS data
6. `media/error.rs` — MediaError enum with AACS error classification
7. `rip.rs` — orchestrates remux jobs with progress tracking via mpsc channel
8. `workflow.rs` — shared business logic: filename generation (`build_output_filename`), overwrite handling (`check_overwrite`), remux job setup (`prepare_remux_options`)
9. `util.rs` contains all pure functions (template rendering, selection parsing)
10. `config.rs` loads TOML config with path resolution (`--config` flag → `BLUBACK_CONFIG` env → default), saves with commented-out defaults, resolves filename format priority, validates on load (unknown keys, numeric bounds, template syntax)
11. `aacs.rs` — AACS backend preflight (library detection via ldconfig, makemkvcon availability, LIBAACS_PATH env var setup, zombie process reaping)
12. `hooks.rs` — post-rip/post-session hook execution: template expansion, `sh -c` execution, blocking/non-blocking modes, output logging
13. `verify.rs` — post-remux output validation: probe MKV headers (duration, stream counts, chapters), optional frame decode at seek points
14. `chapters.rs` — chapter extraction with missing paths/playlists
15. `index.rs` — Blu-ray `index.bdmv` parser: extracts title→playlist ordering for correct episode assignment
16. `streams.rs` — stream filtering (`StreamFilter::apply()`) and CLI track spec parsing (`parse_track_spec()`)
17. `detection.rs` — playlist type detection heuristics (duration, stream count, chapter count) and TMDb runtime matching with confidence levels
18. `history.rs` — SQLite-backed rip history database: schema/migrations, session/file recording (UPSERT), episode/special continuation queries (`json_each`), duplicate detection, listing/filtering/stats, management (delete/prune/clear), JSON export, stale session cleanup, retention auto-prune, `ConfigSnapshot` serialization
19. `history_cli.rs` — `bluback history` CLI subcommand: argv pre-check dispatch, `list`/`show`/`stats`/`delete`/`clear`/`export` handlers with table formatting
20. `duration.rs` — duration string parser for history retention and CLI filters (`30d`, `6months`, `1year`, `YYYY-MM-DD`)

### Two UI Modes

- **TUI mode** (default when stdout is TTY): ratatui wizard (5 screens in TV mode, 4 in movie mode, in `tui/wizard.rs`) → progress dashboard (`tui/dashboard.rs`). State machine in `tui/mod.rs`. App struct decomposed into `DiscState`, `TmdbState`, `WizardState`, `RipState` sub-structs. Settings overlay (`tui/settings.rs`) accessible via `Ctrl+S` from any screen, history overlay (`tui/history.rs`) accessible via `Ctrl+H`, both rendered on top of the current screen via `App.overlay: Option<Overlay>`.
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
- **MKV metadata embedding** — Auto-generated tags (TITLE, SHOW, SEASON_NUMBER, EPISODE_SORT, DATE_RELEASED, REMUXED_WITH) are embedded during remux via `octx.set_metadata()`. Uses `REMUXED_WITH` instead of `ENCODER` because FFmpeg's Matroska muxer overwrites `ENCODER` with its own version string. Configurable via `[metadata]` config section (`enabled` bool, `tags` table for custom key-value pairs). Disabled per-run with `--no-metadata`. Custom tags override auto-generated ones on conflict. Empty values are never written. Per-stream titles (e.g. track names) are a future enhancement alongside per-stream track selection.
- **Per-stream track selection** — Two-layer filtering: `[streams]` config section provides language/format defaults (`audio_languages`, `subtitle_languages`, `prefer_surround`), TUI inline track picker (`t` key in Playlist Manager) allows per-playlist manual overrides. Both resolve to `StreamSelection::Manual(Vec<usize>)` before remux. `StreamFilter` in `src/streams.rs` handles the filtering logic. Deprecated `stream_selection` config key auto-migrates on save.
- **MKV index reservation** — `reserve_index_space` config option (default 500 KB) reserves void space after the MKV header for the seek index (Cues) and in-place metadata edits. Cues at the front of the file enable faster seeking over HTTP byte-range requests, and the extra void space allows tools like `mkvpropedit` to update metadata without rewriting the entire file. If the actual Cues exceed the reserved space, they fall back to EOF (standard behavior). Passed to FFmpeg via `write_header_with` dictionary option.
- **libbluray stderr suppression** — `BD_DEBUG_MASK=0` set by default to prevent libbluray debug output from corrupting TUI. Controlled by `verbose_libbluray` config option.
- **Episode assignment** — Default: sequential with multi-episode detection (uses median playlist duration with 1.5x threshold to detect double-episode playlists). Volume label parsing guesses the starting episode from disc number. The Playlist Manager screen allows overriding individual playlist assignments inline (`e` hotkey), including assigning multiple episodes to a single playlist (e.g., `3-4` or `3,5`). Multi-episode playlists produce range-style filenames like `S01E03-E04_Title.mkv`. The `EpisodeAssignments` type is `HashMap<String, Vec<Episode>>` — each playlist maps to zero or more episodes.
- **Playlist ordering** — Playlists are reordered after scan using the title table from `BDMV/index.bdmv`, which reflects the disc author's intended playback order. Falls back to MPLS number sort if `index.bdmv` is unavailable or unparseable. Both `disc.playlists` and `disc.episodes_pl` are reordered before any episode assignment occurs.
- **Episode reassignment on special changes** — When playlists are marked/unmarked as specials (`s` key, auto-detection, `A` key), regular episode assignments are recalculated via `reassign_regular_episodes()`. This ensures episode numbers shift correctly instead of leaving gaps. The `r` key (reset single) does NOT trigger reassignment — manual edits are intentional.
- **Specials support** — Playlists can be marked as specials (`s` hotkey in TUI Playlist Manager, `--specials <SEL>` in CLI using filtered indices). Uses `S{season}SP{episode}` naming format (actual season, not S00) and a separate `special_format` naming template. TUI: `r` resets a single row's assignment, `R` resets all. CLI: specials auto-assigned SP01, SP02, etc.
- **All playlists visible** — The Playlist Manager shows all disc playlists, not just episode-length ones. Filtered playlists (below `min_duration`) are hidden by default but can be toggled with `f`. Controlled by `show_filtered` config option.
- **TMDb API key**: looked up from config TOML → flat file `~/.config/bluback/tmdb_api_key` → `TMDB_API_KEY` env var.
- **Settings overlay** — `App.overlay: Option<Overlay>` renders on top of the current screen. When active, all global key handlers except `Ctrl+C` are blocked; input routes to the overlay handler. `SettingsState` holds typed `SettingItem` variants (Toggle, Choice, Text, Number, Separator, Action). Choice variant has optional `custom_value` for the "Custom..." option (used by device dropdown). `Ctrl+S` in the overlay saves to `config.toml` with commented-out defaults and triggers workflow reset (rescan) unless mid-rip. Toggle/Choice changes apply to the session immediately without saving.
- **Config path resolution** — Priority: `--config` CLI flag → `BLUBACK_CONFIG` env var → `~/.config/bluback/config.toml`. The resolved path is stored as `config_path: PathBuf` on `App`.
- **Environment variable overrides** — On settings panel open, `BLUBACK_*` env vars are detected and applied to settings items. The import notification persists until user input. On save, a warning notes which env vars will override the config file. Supported: `BLUBACK_OUTPUT_DIR`, `BLUBACK_DEVICE`, `BLUBACK_EJECT`, `BLUBACK_MAX_SPEED`, `BLUBACK_MIN_DURATION`, `BLUBACK_PRESET`, `BLUBACK_TV_FORMAT`, `BLUBACK_MOVIE_FORMAT`, `BLUBACK_SPECIAL_FORMAT`, `BLUBACK_SHOW_FILTERED`, `BLUBACK_VERBOSE_LIBBLURAY`, `BLUBACK_RESERVE_INDEX_SPACE`, `BLUBACK_AACS_BACKEND`, `BLUBACK_OVERWRITE`, `BLUBACK_BATCH`, `BLUBACK_VERIFY`, `BLUBACK_VERIFY_LEVEL`, `BLUBACK_METADATA`, `BLUBACK_AUDIO_LANGUAGES`, `BLUBACK_SUBTITLE_LANGUAGES`, `BLUBACK_PREFER_SURROUND`, `BLUBACK_AUTO_DETECT`, `BLUBACK_HISTORY`, `BLUBACK_HISTORY_PATH`, `BLUBACK_HISTORY_RETENTION`, `TMDB_API_KEY`.
- **`--settings` standalone mode** — Opens settings panel without disc detection or dependency checks. Dirty close prompts to save. Exits after panel close.
- **Signal handling** — `ctrlc` crate registers handler for SIGINT/SIGTERM. First signal sets global `AtomicBool` cancel flag (propagated to remux cancel). Second signal within 2 seconds force-exits with code 130. Partial MKV files are deleted on cancel or error in both CLI and TUI modes.
- **MountGuard** — RAII struct in `disc.rs` with both explicit `cleanup()` and `Drop` impl for disc unmount. Primary cleanup is explicit (called before `std::process::exit()`); `Drop` is a safety net for panics.
- **Overwrite protection** — `--overwrite` flag + `overwrite` config option (default: false). Without flag: CLI prints skip message, TUI marks as `PlaylistStatus::Skipped(file_size)` (displayed dimmed). With flag: deletes existing file and re-rips.
- **Config validation** — `validate_raw_toml()` checks unknown keys against `KNOWN_KEYS`. `validate_config()` checks `min_duration > 0`, `reserve_index_space <= 10000`, unmatched braces in format templates. Warnings to stderr, never errors (forward-compatible).
- **Structured exit codes** — `main()` → `run()` → `run_inner()`. `classify_exit_code()` uses both string matching and `MediaError` downcast for error-to-code mapping.
- **Setup validation** — `--check` validates environment without requiring a disc: FFmpeg libs, libbluray, libaacs, KEYDB.cfg, libmmbd, makemkvcon, udisksctl, optical drives, drive permissions, output directory, TMDb API key, config file. Exit code 0 (all required pass) or 2 (any required fail). Dispatches before AACS preflight.
- **Headless progress** — Non-TTY stdout gets `println!` progress lines at 10-second wall-clock intervals instead of `\r` carriage returns. TTY keeps existing carriage return behavior.
- **Post-rip hooks** — `[post_rip]` and `[post_session]` config tables with `command`, `on_failure`, `blocking`, `log_output` fields. Commands run via `sh -c` with `{var}` template expansion (TODO(debt): shell injection risk from unescaped values). Per-file hook fires after each playlist remux; per-session hook fires after all jobs complete. Both called from CLI (`cli.rs`) and TUI (`tui/dashboard.rs`). `--no-hooks` disables for the run. Hook failures are logged but never fail the rip.
- **Rip verification** — Optional post-remux validation. `verify` config (default false) + `verify_level` ("quick" or "full"). Quick: probe output MKV headers — check duration (2% tolerance), stream counts, chapter count. Full: adds sample frame decode at 5 seek points. `--verify`, `--verify-level`, `--no-verify` CLI flags. TUI prompts on failure (delete & retry / keep / skip). CLI logs warning. Hook vars: `{verify}` (passed/failed/skipped), `{verify_detail}` (comma-separated failed check names).
- **Batch mode** — `--batch` flag + `batch` config option enables continuous multi-disc ripping. After rip completes, auto-ejects and waits for next disc. TUI: auto-restarts wizard on disc detection (skip popup). CLI: outer loop with disc polling via `get_volume_label()`, 2-second interval. Episode numbers auto-advance across discs (specials excluded). `--batch` conflicts with `--dry-run`, `--list-playlists`, `--check`, `--settings`, `--no-eject`. Settings panel toggle + `BLUBACK_BATCH` env var. Disc counter shown in TUI block title.
- **Rip history** — SQLite database at `$XDG_DATA_HOME/bluback/history.db` (default `~/.local/share/bluback/history.db`). Records every disc scan and rip session. `HistoryDb` wraps `rusqlite::Connection` with `&self` methods (interior mutability). Each TUI session thread opens its own connection (WAL mode for safe concurrency); CLI is single-threaded. Episode/special continuation queries use `json_each()` over JSON episode arrays. Duplicate detection checks volume label (exact) and TMDb ID (historical). Stale sessions (in_progress > 4 hours) auto-cleaned on open. Retention auto-prune runs on startup if configured. `--no-history` disables all DB access; `--ignore-history` disables reads but still records. `bluback history` subcommand uses argv pre-check before clap parsing (avoids `--title` collision with flatten approach). Schema versioned via `schema_version` table with embedded migration array.
- **Auto-detection** — Optional heuristic system (`auto_detect` config, `--auto-detect` CLI) that pre-marks likely specials and multi-episode playlists. Layer 1: duration outliers (<50% median = high special, 50-75% = medium, >200% = multi-episode), stream count anomalies, chapter count anomalies. Layer 2: TMDb runtime matching (±10% or ±3min tolerance) with season 0 fetch for specials. Three confidence levels (High/Medium/Low); high-confidence pre-marked in TUI, auto-applied in headless CLI. `--specials` takes precedence over auto-detection.

## Testing

Unit tests live in `#[cfg(test)] mod tests` blocks within each module. Integration tests in `tests/` directory. No tests require hardware or network access.

**Unit tests (625):**
- `util.rs` — duration parsing, filename sanitization, selection parsing, episode input parsing, episode assignment, multi-episode filename rendering, template rendering, episode counting for batch auto-advance
- `disc.rs` — volume label parsing, playlist filtering
- `detection.rs` — duration heuristics, stream/chapter count analysis, TMDb runtime matching, confidence stacking, edge cases
- `media/probe.rs` — HDR classification, channel layout formatting, framerate/aspect ratio formatting, playlist log line parsing, GCD, DTS profile formatting
- `media/remux.rs` — stream selection logic (all, prefer_surround, manual), map arg building, progress line parsing, size/ETA estimation, chapter OGM formatting
- `config.rs` — TOML parsing, format resolution priority chain, config path resolution, save/load roundtrip, commented-defaults output, validation (unknown keys, numeric bounds, template braces), aacs_backend parsing, overwrite/batch option, should_batch resolution, history config parsing/defaults/known_keys/validation
- `types.rs` — MediaInfo field mapping, ChapterMark struct, SettingsState construction/roundtrip, cursor navigation, env var overrides, batch toggle
- `tui/settings.rs` — truncate/mask helpers, input handling (toggle, choice, text edit, number validation, cursor movement, confirm close prompt)
- `tui/dashboard.rs` — rendering modes, key hints, progress display, done screen layout
- `tui/wizard.rs` — playlist manager rendering, key handling, focus states
- `tui/coordinator.rs` — session lifecycle, tab management, history overlay open/close
- `tui/tab_bar.rs` — tab rendering, active tab switching
- `tui/history.rs` — overlay navigation (up/down/wrap), detail view open/close, delete confirm/cancel, clear all confirm/cancel, empty state, format helpers
- `cli.rs` — stream selection integration, batch summary formatting
- `streams.rs` — stream filtering by language, surround preference, track spec parsing
- `hooks.rs` — template expansion, hook execution config, command building
- `logging.rs` — session header formatting, log level parsing
- `verify.rs` — verification level config, result classification, check logic
- `workflow.rs` — filename building, overwrite handling, remux option setup
- `rip.rs` — progress tracking, job status transitions
- `session.rs` — state machine transitions, rescan preservation, batch field survival, history config survival across reset
- `aacs.rs` — command_exists, is_libmmbd path detection
- `chapters.rs` — chapter extraction with missing paths/playlists
- `index.rs` — Blu-ray index.bdmv parsing, playlist reordering logic
- `drive_monitor.rs` — drive detection, event classification
- `check.rs` — environment validation
- `history.rs` — schema creation/migration, WAL mode, CHECK/UNIQUE constraints, CASCADE delete, session lifecycle (start/finish), disc playlist recording, file recording with UPSERT (ID preservation), episode continuation (TMDb match, label match, multi-episode, specials, cross-season prevention), duplicate detection (label match, TMDb match, multiple matches), session listing (status filter, title search), session detail, stats aggregation, display_status partial derivation (excludes skipped), delete/clear/prune with cascade, stale session cleanup
- `duration.rs` — days/months/years parsing (singular/plural), absolute date parsing, invalid input rejection, cutoff date conversion (relative/absolute)

**Integration tests (20):**
- `tests/tmdb_parsing.rs` — TMDb JSON deserialization from fixture files (`tests/fixtures/tmdb/`)
- `tests/cli_batch_conflicts.rs` — clap argument conflict validation for --batch
- `tests/cli_flag_conflicts.rs` — clap argument conflict validation for stream flags
- `tests/history_integration.rs` — history CLI subcommand end-to-end (list/stats/export on empty DB, default subcommand, help output, show nonexistent, clear confirmation), clap regression (existing flags + `--title history` not confused with subcommand)

**Test fixtures:**
- `tests/fixtures/media/` — synthetic MKV files generated via `tests/generate_fixtures.sh`
- `tests/fixtures/tmdb/` — canned TMDb API JSON responses

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
      --specials <SEL>         Mark playlists as specials (uses filtered indices, e.g. 1,3)
      --list-playlists         Print playlist info and exit
  -v, --verbose                Verbose output (with --list-playlists: show stream details)
      --overwrite              Overwrite existing output files instead of skipping
      --no-metadata            Don't embed metadata tags in output MKV files
      --no-hooks               Disable post-rip/post-session hooks for this run
      --verify                 Verify output files after ripping
      --verify-level <LEVEL>   Verification level: quick or full
      --no-verify              Disable verification (overrides config)
      --auto-detect            Enable automatic episode/special detection heuristics
      --no-auto-detect         Disable auto-detection (overrides config)
      --audio-lang <LANGS>     Filter audio by language (e.g. "eng,jpn")
      --subtitle-lang <LANGS>  Filter subtitles by language (e.g. "eng")
      --tracks <SPEC>          Select streams by type-local index (e.g. "a:0,2;s:0-1")
      --prefer-surround        Prefer surround audio over stereo
      --all-streams            Include all streams, ignoring config filters
      --aacs-backend <BACKEND> AACS decryption backend: auto, libaacs, or libmmbd
      --log-level <LEVEL>      Stderr log verbosity: error, warn, info, debug, trace [default: warn]
      --no-log                 Disable log file output
      --log-file <PATH>        Custom log file path (overrides default location)
      --check                  Validate environment setup and exit (no disc required)
      --settings               Open settings panel (no disc/ffmpeg required)
      --batch                  Batch mode: rip → eject → wait → repeat
      --no-batch               Disable batch mode (overrides config)
      --no-history             Disable history for this run (no recording, no queries)
      --ignore-history         Skip duplicate detection and episode continuation (still records)
      --config <PATH>          Path to config file (also: BLUBACK_CONFIG env var)
```

**History subcommand:**
```
bluback history                            # alias for 'list'
bluback history list [OPTIONS]             # list past sessions
    --limit <N>                            # default 20
    --status <STATUS>                      # completed, failed, cancelled, scanned
    --title <SEARCH>                       # fuzzy match
    --since <DURATION>                     # "2026-04-01", "7d", "1month"
    --season <N>
    --batch-id <UUID>                      # filter by batch run
    --json                                 # machine-readable output
bluback history show <ID>                  # full session detail + files
bluback history stats                      # aggregate summary
bluback history delete <ID> [<ID>...]      # delete sessions (with confirmation)
bluback history clear                      # delete all (with confirmation)
    --older-than <DURATION>                # prune by age
    --status <STATUS>                      # prune by status
    --yes                                  # skip confirmation
bluback history export                     # JSON dump to stdout
```

`--format` and `--format-preset` are mutually exclusive (clap argument group).
`--yes` auto-enables when stdin is not a TTY (headless/scripted contexts).
`--auto-detect` conflicts with `--movie`.
`--no-history` and `--ignore-history` are mutually exclusive.

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Runtime error (rip failure, FFmpeg error, I/O) |
| 2 | Usage/config error |
| 3 | No disc/device |
| 4 | User cancelled |

## TUI Keybindings

**Global (all screens):**
- `Ctrl+S` — Open settings panel (overlay; all other globals blocked while open)
- `Ctrl+H` — Open history overlay (overlay; browse/manage rip history)
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
- `t` — Expand/collapse track list (shows video/audio/subtitle streams)
- `f` — Show/hide filtered (short) playlists
- `A` — Accept all auto-detected suggestions (medium+ confidence)
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

**History overlay (`Ctrl+H`):**
- `Up/Down` — Navigate session list
- `Enter` — Toggle detail view (show/hide files)
- `d` — Delete selected session (with confirmation)
- `D` — Clear all sessions (with confirmation)
- `y/n` — Confirm/cancel when prompted
- `Esc` — Close detail view, or close overlay

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
| `ctrlc` | Signal handling (SIGINT/SIGTERM) |
| `libc` | POSIX waitpid for zombie process reaping |
| `rusqlite` (bundled) | SQLite bindings for rip history database |
| `uuid` | Batch ID generation (v4 random UUIDs) |
