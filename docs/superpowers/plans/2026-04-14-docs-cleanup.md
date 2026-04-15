# Documentation Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure bluback documentation — slim README to a landing page, move reference material to focused docs pages, trim CLAUDE.md, rewrite the roadmap, explain internal dev artifacts.

**Architecture:** No code changes. All work is documentation reorganization. Content moves from README.md and CLAUDE.md to new files in `docs/`. Stale content is corrected during the move (not copied verbatim). The spec is at `docs/superpowers/specs/2026-04-14-docs-cleanup-design.md`.

**Tech Stack:** Markdown files only.

---

### Task 1: Create `docs/cli-reference.md`

**Files:**
- Create: `docs/cli-reference.md`

This is the most mechanical task — generate from `--help` output, not from stale README.

- [ ] **Step 1: Create the CLI reference document**

```markdown
# CLI Reference

## Usage

```
bluback [OPTIONS]
```

By default, bluback auto-detects your Blu-ray drive and launches a TUI wizard.

## Options

| Flag | Description |
|------|-------------|
| `-d, --device <DEVICE>` | Blu-ray device path (default: auto-detect) |
| `-o, --output <DIR>` | Output directory (default: `.`) |
| `-s, --season <NUM>` | Season number (skips prompt) |
| `-e, --start-episode <NUM>` | Starting episode number (skips prompt) |
| `--min-probe-duration <SECS>` | Min seconds to probe playlist, filters menu clips (default: 30) |
| `--movie` | Movie mode (skip episode assignment) |
| `--dry-run` | Show what would be ripped without ripping |
| `--no-tui` | Plain text mode (auto if not a TTY) |
| `--format <TEMPLATE>` | Custom filename template |
| `--format-preset <NAME>` | Built-in preset: `default`, `plex`, `jellyfin` |
| `--eject` | Eject disc after successful rip |
| `--no-eject` | Don't eject disc after rip (overrides config) |
| `--no-max-speed` | Don't set drive to maximum read speed |
| `--settings` | Open settings panel without starting a rip |
| `--config <PATH>` | Path to config file (also: `BLUBACK_CONFIG` env var) |
| `-y, --yes` | Accept all defaults without prompting (auto if stdin not a TTY) |
| `--title <STRING>` | Set show/movie title directly, skipping TMDb lookup |
| `--year <STRING>` | Movie release year (with `--title` in `--movie` mode) |
| `--playlists <SEL>` | Select specific playlists (e.g. `1,2,3` or `1-3` or `all`) |
| `--specials <SEL>` | Mark playlists as specials (e.g. `4,5` or `4-5`) |
| `--hide-specials` | Hide detected specials from ripping (skips all specials) |
| `--overwrite` | Overwrite existing output files instead of skipping |
| `--list-playlists` | Scan disc and print playlist info, then exit |
| `-v, --verbose` | Verbose output (with `--list-playlists`: show stream details) |
| `--aacs-backend <BACKEND>` | AACS decryption backend: `auto`, `libaacs`, or `libmmbd` |
| `--check` | Validate environment and configuration, then exit |
| `--log-level <LEVEL>` | Stderr log verbosity: `error`, `warn`, `info`, `debug`, `trace` (default: `warn`) |
| `--no-log` | Disable log file output |
| `--log-file <PATH>` | Custom log file path (overrides default location) |
| `--no-metadata` | Don't embed metadata tags in output MKV files |
| `--no-hooks` | Disable post-rip and post-session hooks for this run |
| `--verify` | Verify output files after ripping |
| `--verify-level <LEVEL>` | Verification level: `quick` (header probe) or `full` (+ frame decode) |
| `--no-verify` | Disable verification (overrides config) |
| `--batch` | Enable batch mode (rip → eject → wait → repeat) |
| `--no-batch` | Disable batch mode (overrides config) |
| `--auto-detect` | Enable automatic episode/special detection heuristics |
| `--no-auto-detect` | Disable auto-detection (overrides config) |
| `--audio-lang <LANGS>` | Filter audio streams by language (e.g. `eng,jpn`) |
| `--subtitle-lang <LANGS>` | Filter subtitle streams by language (e.g. `eng`) |
| `--prefer-surround` | Prefer surround audio (select surround + one stereo) |
| `--all-streams` | Include all streams, ignoring config filters |
| `--tracks <SPEC>` | Select streams by type-local index (e.g. `a:0,2;s:0-1`) |
| `--no-history` | Disable history for this run |
| `--ignore-history` | Ignore history (skip duplicate detection and episode continuation, still records) |

### Flag Interactions

- `--format` and `--format-preset` are mutually exclusive.
- `--yes` auto-enables when stdin is not a TTY (headless/scripted contexts).
- `--auto-detect` conflicts with `--movie`.
- `--no-history` and `--ignore-history` are mutually exclusive.
- `--batch` conflicts with `--dry-run`, `--list-playlists`, `--check`, `--settings`, `--no-eject`.

### Headless / Scripted Usage

When stdin is not a TTY, `--yes` auto-enables and bluback accepts all defaults. Combine with `--title`, `--playlists`, `--season`, and `--start-episode` for fully scripted rips:

```bash
bluback --title "Breaking Bad" -s 1 -e 1 --playlists 1-5 -o ~/rips
```

## History Subcommand

```
bluback history [COMMAND]
```

| Command | Description |
|---------|-------------|
| `list` | List past sessions (default if no command given) |
| `show <ID>` | Show full details for a session |
| `stats` | Show aggregate statistics |
| `delete <ID>...` | Delete specific sessions |
| `clear` | Clear history |
| `export` | Export history as JSON |

### `history list` Options

| Flag | Description |
|------|-------------|
| `--limit <N>` | Max sessions to show (default: 20) |
| `--status <STATUS>` | Filter by status: `completed`, `failed`, `cancelled`, `scanned` |
| `--title <SEARCH>` | Filter by title |
| `--since <DURATION>` | Filter by age (e.g. `7d`, `1month`, `2026-04-01`) |
| `--season <N>` | Filter by season number |
| `--batch-id <UUID>` | Filter by batch run |
| `--json` | Machine-readable JSON output |

### `history clear` Options

| Flag | Description |
|------|-------------|
| `--older-than <DURATION>` | Prune by age |
| `--status <STATUS>` | Prune by status |
| `-y, --yes` | Skip confirmation (must be explicit — does NOT auto-enable on non-TTY) |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Runtime error (rip failure, FFmpeg error, I/O) |
| 2 | Usage/config error |
| 3 | No disc/device |
| 4 | User cancelled |
```

- [ ] **Step 2: Verify the document renders correctly**

Run: `head -20 docs/cli-reference.md`
Expected: The markdown header and table start rendering.

- [ ] **Step 3: Commit**

```bash
git add docs/cli-reference.md
git commit -m "docs: add CLI reference page"
```

---

### Task 2: Create `docs/configuration.md`

**Files:**
- Create: `docs/configuration.md`
- Reference: `src/config.rs` (KNOWN_KEYS at line 536), `src/types.rs` (env vars at line 953)

- [ ] **Step 1: Create the configuration document**

```markdown
# Configuration

## Config File

Default location: `~/.config/bluback/config.toml`

Override with `--config <PATH>` or the `BLUBACK_CONFIG` environment variable.

You can edit the config interactively with `bluback --settings` or by pressing `Ctrl+S` during any TUI screen. Saving from the settings panel writes all fields with defaults commented out for reference.

## Config Reference

```toml
# Default output directory
# output_dir = "."

# Default device (or "auto-detect")
# device = "auto-detect"

# Auto-eject disc after rip
# eject = false

# Set drive to max read speed
# max_speed = true

# Minimum playlist duration (seconds) for probe filtering (default: 30)
# min_probe_duration = 30

# Filename preset: "default", "plex", or "jellyfin"
# preset = "default"

# Custom format templates (override preset)
# tv_format = "S{season}E{episode}_{title}.mkv"
# movie_format = "{title}_({year}).mkv"
# special_format = "{show} S{season}SP{episode} {title}.mkv"

# Show playlists below min_probe_duration by default in Playlist Manager
# show_filtered = false

# Show libbluray debug output on stderr (default: false)
# verbose_libbluray = false

# Reserve space in MKV header for seek index (KB, default: 500, max: 10000)
# reserve_index_space = 500

# Overwrite existing output files instead of skipping
# overwrite = false

# Enable batch mode (rip → eject → wait → repeat)
# batch = false

# Enable automatic episode/special detection heuristics
# auto_detect = false

# AACS decryption backend: "auto", "libaacs", or "libmmbd"
# aacs_backend = "auto"

# Verify output files after ripping
# verify = false
# verify_level = "quick"

# Multi-drive mode: "auto" or "manual"
# multi_drive = "auto"

# TMDb API key (also checked in ~/.config/bluback/tmdb_api_key and TMDB_API_KEY env var)
# tmdb_api_key = "your-key-here"

# Logging
# log_file = true
# log_level = "warn"
# log_dir = "~/.local/share/bluback/logs"
# max_log_files = 10

# Stream selection
[streams]
# audio_languages = ["eng", "jpn"]
# subtitle_languages = ["eng"]
# prefer_surround = false

# MKV metadata embedding
[metadata]
# enabled = true
# [metadata.tags]
# CUSTOM_TAG = "custom value"

# Post-rip hook (runs after each file)
[post_rip]
# command = "echo 'Ripped {file}'"
# on_failure = false
# blocking = true
# log_output = false

# Post-session hook (runs after all files)
[post_session]
# command = "echo 'Session complete: {succeeded}/{total} succeeded'"
# on_failure = false
# blocking = true
# log_output = false

# Rip history
[history]
# enabled = true
# path = "~/.local/share/bluback/history.db"
# retention = "90d"
# retention_statuses = ["completed", "scanned"]
```

## Environment Variables

Settings can also be set via environment variables. When the settings panel opens, it detects and imports any set `BLUBACK_*` variables. Environment variables take precedence over config file values at runtime.

| Variable | Config Key |
|----------|------------|
| `BLUBACK_OUTPUT_DIR` | `output_dir` |
| `BLUBACK_DEVICE` | `device` |
| `BLUBACK_EJECT` | `eject` |
| `BLUBACK_MAX_SPEED` | `max_speed` |
| `BLUBACK_MIN_PROBE_DURATION` | `min_probe_duration` |
| `BLUBACK_PRESET` | `preset` |
| `BLUBACK_TV_FORMAT` | `tv_format` |
| `BLUBACK_MOVIE_FORMAT` | `movie_format` |
| `BLUBACK_SPECIAL_FORMAT` | `special_format` |
| `BLUBACK_SHOW_FILTERED` | `show_filtered` |
| `BLUBACK_VERBOSE_LIBBLURAY` | `verbose_libbluray` |
| `BLUBACK_RESERVE_INDEX_SPACE` | `reserve_index_space` |
| `BLUBACK_OVERWRITE` | `overwrite` |
| `BLUBACK_BATCH` | `batch` |
| `BLUBACK_AUTO_DETECT` | `auto_detect` |
| `BLUBACK_VERIFY` | `verify` |
| `BLUBACK_VERIFY_LEVEL` | `verify_level` |
| `BLUBACK_AACS_BACKEND` | `aacs_backend` |
| `BLUBACK_METADATA` | `metadata.enabled` |
| `BLUBACK_AUDIO_LANGUAGES` | `streams.audio_languages` |
| `BLUBACK_SUBTITLE_LANGUAGES` | `streams.subtitle_languages` |
| `BLUBACK_PREFER_SURROUND` | `streams.prefer_surround` |
| `BLUBACK_HISTORY` | `history.enabled` |
| `BLUBACK_HISTORY_RETENTION` | `history.retention` |
| `TMDB_API_KEY` | `tmdb_api_key` |

When saving from the settings panel, a warning notes which env vars will override the saved config.

## TMDb API Key

The TMDb API key is looked up in this order:
1. `tmdb_api_key` in config file
2. `~/.config/bluback/tmdb_api_key` (plain text file)
3. `TMDB_API_KEY` environment variable

Get a free API key at [themoviedb.org/settings/api](https://www.themoviedb.org/settings/api).

## Filename Templates

Templates use `{placeholder}` syntax. Available placeholders:

| Placeholder | Description |
|-------------|-------------|
| `{show}` | Show or movie title |
| `{season}` | Season number (zero-padded) |
| `{episode}` | Episode number (zero-padded) |
| `{title}` | Episode title |
| `{year}` | Release year |
| `{resolution}` | Video resolution (e.g. `1080p`) |
| `{audio}` | Primary audio codec |
| `{channels}` | Audio channel layout |
| `{codec}` | Video codec |

Bracket groups `[...]` auto-collapse when their contents are empty. Useful for optional metadata:

```
{show} S{season}E{episode} {title}[ ({resolution})][ {audio}].mkv
```

### Priority Chain

Filename format is resolved (highest to lowest):
1. `--format` CLI flag
2. `--format-preset` CLI flag
3. `tv_format` / `movie_format` / `special_format` in config
4. `preset` in config
5. `"default"` built-in preset
```

- [ ] **Step 2: Commit**

```bash
git add docs/configuration.md
git commit -m "docs: add configuration reference page"
```

---

### Task 3: Create `docs/keybindings.md`

**Files:**
- Create: `docs/keybindings.md`

- [ ] **Step 1: Create the keybindings document**

```markdown
# TUI Keybindings

## Screen Flow

**TV mode:** Scanning → TMDb Search → Season → Playlist Manager → Confirm → Ripping → Done

**Movie mode:** Scanning → TMDb Search → Playlist Manager → Confirm → Ripping → Done

## Global (all screens)

| Key | Action |
|-----|--------|
| `Ctrl+S` | Open settings panel (overlay) |
| `Ctrl+H` | Open history overlay |
| `Ctrl+R` | Rescan disc and restart wizard (confirms during ripping) |
| `Ctrl+E` | Eject disc |
| `Ctrl+C` | Quit immediately |
| `q` | Quit (except during text input or ripping) |

When an overlay (settings or history) is open, all global keys except `Ctrl+C` are blocked — input routes to the overlay.

## TMDb Search

| Key | Action |
|-----|--------|
| `Enter` | Search (in input) / Select (in results) |
| `Up/Down` | Navigate between input and results |
| `Tab` | Toggle Movie/TV mode |
| `Esc` | Skip TMDb |

## Season (TV mode)

| Key | Action |
|-----|--------|
| `Enter` | Fetch episodes / Confirm and proceed |
| `Up/Down` | Scroll episode list |
| `Esc` | Go back to TMDb Search |

## Playlist Manager

| Key | Action |
|-----|--------|
| `Space` | Toggle playlist selection |
| `e` | Edit episode assignment inline (format: `3`, `3-4`, or `3,5`) |
| `s` | Toggle special marking (TV mode only) |
| `r` | Reset current row's assignment |
| `R` | Reset all episode assignments |
| `t` | Expand/collapse track list (video/audio/subtitle streams) |
| `f` | Show/hide filtered (short) playlists |
| `A` | Accept all auto-detected suggestions (medium+ confidence) |
| `Enter` | Confirm and proceed |
| `Esc` | Go back |

## Ripping Dashboard

| Key | Action |
|-----|--------|
| `q` | Abort (with confirmation) |

## Done Screen

| Key | Action |
|-----|--------|
| `Enter` | Rescan disc and restart wizard |
| Any other key | Exit |

The Done screen auto-detects disc insertion and shows a popup prompt.

## Settings Panel (overlay)

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate settings (skips separators) |
| `Enter/Space` | Toggle (bool), cycle (choice), enter edit (text/number), save (action) |
| `Left/Right` | Cycle choice backward/forward |
| `Esc` | Cancel edit (if editing), otherwise close panel |
| `Ctrl+S` | Save to config file |

## History Overlay (Ctrl+H)

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate session list |
| `Enter` | Toggle detail view (show/hide files) |
| `d` | Delete selected session (with confirmation) |
| `D` | Clear all sessions (with confirmation) |
| `y/n` | Confirm/cancel when prompted |
| `Esc` | Close detail view, or close overlay |
```

- [ ] **Step 2: Commit**

```bash
git add docs/keybindings.md
git commit -m "docs: add TUI keybindings reference page"
```

---

### Task 4: Create `docs/features.md`

**Files:**
- Create: `docs/features.md`
- Reference: `CLAUDE.md` Key Design Decisions section, `src/hooks.rs`, `src/verify.rs`, `src/detection.rs`, `src/history.rs`

This is the largest new document. Content for stream selection, verification, and chapters is moved from the README. Everything else is new content written from CLAUDE.md and code.

- [ ] **Step 1: Create the features document**

```markdown
# Features Guide

## Chapter Preservation

bluback automatically extracts chapter markers from the Blu-ray's MPLS playlist files and embeds them directly into the output MKV files during remux via the FFmpeg AVChapter API. No external tools are needed.

The disc is temporarily mounted to read the playlist data from `BDMV/PLAYLIST/`, then unmounted after extraction. Chapter counts are displayed alongside each playlist in TUI mode.

## Stream Selection

By default, bluback includes all video, audio, and subtitle streams from each playlist. You can filter streams in three ways:

**Config defaults** — Set language preferences in `config.toml` that apply to every rip:

```toml
[streams]
audio_languages = ["eng", "jpn"]
subtitle_languages = ["eng"]
prefer_surround = true
```

**CLI flags** — Override config for a single run:

```bash
# Keep only English and Japanese audio, English subtitles
bluback --audio-lang eng,jpn --subtitle-lang eng

# Select specific streams by index (use --list-playlists -v to see indices)
bluback --tracks "a:1;s:0-1"

# Override config filters, include everything
bluback --all-streams
```

**TUI track picker** — Press `t` on any playlist in the Playlist Manager to expand its stream list. Toggle individual streams with `Space`. Custom selections are shown in the Ch column as `1v 2a 3s*`.

Language filters are preferences, not hard requirements: if no streams match a configured language, all streams of that type are included with a warning. Streams without language tags (`und`) are always included.

## Rip Verification

bluback can verify output files after ripping to catch corruption:

```bash
bluback --verify                      # Quick: probe headers (duration, streams, chapters)
bluback --verify --verify-level full  # Full: headers + sample frame decode at 5 seek points
```

Enable by default in config: `verify = true`, `verify_level = "quick"`.

Quick verification checks:
- Duration matches expected (2% tolerance)
- Stream counts match source
- Chapter count matches source

Full verification adds sample frame decode at 5 evenly-spaced seek points.

In TUI mode, failed verification prompts to delete & retry, keep, or skip. In CLI mode, a warning is logged and the file is kept.

Hook variables `{verify}` (passed/failed/skipped) and `{verify_detail}` (failed check names) are available for post-rip hooks.

## Batch Mode

Continuous multi-disc ripping: rip → eject → wait for next disc → auto-start.

```bash
bluback --batch -o ~/rips
```

Or enable in config: `batch = true`.

In batch mode:
- Episode numbers auto-advance across discs (specials excluded)
- TUI auto-restarts the wizard when a new disc is detected
- CLI uses an outer loop with disc polling (2-second interval)
- A disc counter is shown in the TUI title

`--batch` conflicts with `--dry-run`, `--list-playlists`, `--check`, `--settings`, and `--no-eject`.

## Post-Rip Hooks

User-configurable shell commands that run after individual rips and/or session completion.

```toml
# Runs after each file is ripped
[post_rip]
command = "notify-send 'Ripped {filename}'"
on_failure = false    # Also run on failed rips
blocking = true       # Wait for hook to finish
log_output = false    # Log hook stdout/stderr

# Runs after all files are done
[post_session]
command = "echo '{succeeded}/{total} files ripped from {label}'"
```

Disable for a run with `--no-hooks`.

### Per-File Template Variables

`{file}`, `{filename}`, `{dir}`, `{size}`, `{chapters}`, `{title}`, `{season}`, `{episode}`, `{episode_name}`, `{playlist}`, `{label}`, `{mode}`, `{device}`, `{status}`, `{error}`, `{verify}`, `{verify_detail}`

### Per-Session Template Variables

`{total}`, `{succeeded}`, `{failed}`, `{skipped}`, `{label}`, `{mode}`, `{device}`

Hook failures are logged but never fail the rip.

## MKV Metadata

bluback auto-generates and embeds metadata tags during remux:

| Tag | Value |
|-----|-------|
| `TITLE` | Episode or movie title |
| `SHOW` | Show name (TV mode) |
| `SEASON_NUMBER` | Season number (TV mode) |
| `EPISODE_SORT` | Episode number (TV mode) |
| `DATE_RELEASED` | Release date (from TMDb) |
| `REMUXED_WITH` | `bluback vX.Y.Z` |

Custom tags can be added via config:

```toml
[metadata]
enabled = true
[metadata.tags]
CUSTOM_TAG = "custom value"
```

Custom tags override auto-generated ones on name conflict. Empty values are never written.

Disable per-run with `--no-metadata`.

## Rip History

SQLite database tracking every disc scan and rip session. Default location: `~/.local/share/bluback/history.db`.

### Features

- **Episode continuation** — remembers the last episode ripped for a show/season, auto-advances the starting episode for the next disc
- **Duplicate detection** — warns when re-ripping a disc that's already been ripped (by volume label or TMDb ID)
- **Retention auto-prune** — automatically cleans old sessions on startup based on configured retention period

### CLI Management

```bash
bluback history                          # List recent sessions
bluback history list --status completed  # Filter by status
bluback history show 42                  # Session details + files
bluback history stats                    # Aggregate summary
bluback history delete 42 43             # Delete sessions
bluback history clear --older-than 90d   # Prune old sessions
bluback history export                   # JSON dump
```

See [CLI Reference](cli-reference.md) for full flag details.

### TUI Integration

Press `Ctrl+H` during any screen to open the history overlay. Browse sessions, view details, and delete entries without leaving the wizard.

Contextual hints appear on wizard screens showing continuation info and duplicate warnings.

### Configuration

```toml
[history]
enabled = true
path = "~/.local/share/bluback/history.db"
retention = "90d"
retention_statuses = ["completed", "scanned"]
```

Disable per-run with `--no-history`. Skip duplicate detection/continuation (but still record) with `--ignore-history`.

## Auto-Detection

Optional heuristic system that pre-marks likely specials and multi-episode playlists.

```bash
bluback --auto-detect
```

Or enable in config: `auto_detect = true`.

### How It Works

**Layer 1 — Duration heuristics:**
- Playlists <50% of median duration → high-confidence special
- Playlists 50-75% of median → medium-confidence special
- Playlists >200% of median → likely multi-episode

Additional signals: stream count anomalies, chapter count anomalies.

**Layer 2 — TMDb runtime matching:**
- Compares playlist durations against TMDb episode runtimes (±10% or ±3min tolerance)
- Fetches season 0 for special matching

Three confidence levels (High/Medium/Low). In TUI mode, high-confidence suggestions are pre-marked. Press `A` in the Playlist Manager to accept all medium+ suggestions. In headless CLI mode, high-confidence suggestions are auto-applied.

`--specials` takes precedence over auto-detection. Conflicts with `--movie`.

## Multi-Drive Support

bluback supports parallel ripping from multiple Blu-ray drives simultaneously.

### How It Works

A background drive monitor polls for optical drives every 2 seconds. Each detected drive gets its own independent session with a dedicated tab in the TUI.

- **Tab switching:** `Tab` / `Shift+Tab` to switch between drive sessions
- **Inter-session linking:** `Ctrl+L` copies TMDb context (show name, season, next episode) from one session to another — avoids redundant TMDb lookups across discs of the same show
- **Episode overlap detection:** warns if two sessions assign the same episode for the same show/season

### Configuration

```toml
# "auto" (default): TUI auto-detects all drives
# "manual": single device mode
multi_drive = "auto"
```

Multi-drive is TUI-only. CLI mode uses a single drive specified by `-d`.
```

- [ ] **Step 2: Commit**

```bash
git add docs/features.md
git commit -m "docs: add features guide with stream selection, verification, batch, hooks, metadata, history, auto-detection, and multi-drive"
```

---

### Task 5: Create `docs/superpowers/README.md`

**Files:**
- Create: `docs/superpowers/README.md`

- [ ] **Step 1: Create the explainer document**

```markdown
# Development Specs & Plans

Internal design specifications and implementation plans generated during bluback development. Each feature goes through a brainstorming → spec → plan → implementation cycle.

- `specs/` — Design documents describing what to build and why
- `plans/` — Step-by-step implementation plans derived from specs
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/README.md
git commit -m "docs: add explainer for internal spec/plan files"
```

---

### Task 6: Archive and rewrite the roadmap

**Files:**
- Rename: `docs/ROADMAP-1.0.md` → `docs/ROADMAP-1.0-original.md`
- Create: `docs/ROADMAP-1.0.md`

- [ ] **Step 1: Archive the original roadmap**

```bash
git mv docs/ROADMAP-1.0.md docs/ROADMAP-1.0-original.md
```

- [ ] **Step 2: Create the new roadmap**

```markdown
# bluback Roadmap

Current version: v0.11.0

## Released

| Version | Theme | Highlights |
|---------|-------|------------|
| v0.6 | Stability & Safety | Error handling, signal handling, overwrite protection, exit codes, AACS backend detection |
| v0.7 | Architecture & CLI | Workflow extraction, specials CLI, headless progress, `--check`, `--list-playlists` |
| v0.8 | macOS Support | Platform-specific disc ops, Homebrew library discovery, macOS CI + release builds |
| v0.9 | Multi-Drive & CI | Parallel sessions, tab UI, drive monitor, 5-platform CI, crates.io publishing |
| v0.10 | Quality of Life | Log files, MKV metadata, post-rip hooks, rip verification, per-stream track selection, batch mode, auto-detection heuristics |
| v0.11 | History | SQLite rip history, episode continuation, duplicate detection, retention auto-prune |

## Upcoming

### v0.12 — DVD Support

Disc type abstraction, DVD title enumeration via FFmpeg `dvd://` protocol, chapter extraction, CSS error handling. Requires its own design spec.

### v0.13 — UHD Blu-ray

AACS 2.0 investigation, HDR metadata preservation verification (Dolby Vision, HDR10, HDR10+), UHD-specific UX (HDR type display, compatibility warnings).

### v0.14 — Distribution & Polish

Shell completions (`clap_complete` for bash/zsh/fish), man page (`clap_mangen`), included in release artifacts.

### v1.0 — Release

Documentation rewrite (in progress), integration testing, final release.

## Post-1.0

- **GUI frontend** — architecture prepared by workflow extraction in v0.7
- **Windows support** — platform abstraction layer established by macOS work in v0.8
- **Resume partial rips** — depends on FFmpeg MKV muxer seek support investigation
- **Pause/resume during ripping** — `AtomicBool` pause flag, TUI indicator
- **Desktop notifications** — `notify-send` (Linux), native APIs (macOS/Windows)
- **Transcoding profiles** — optional re-encoding (H.265, AAC) during rip

## Original Planning Document

See [ROADMAP-1.0-original.md](ROADMAP-1.0-original.md) for the detailed milestone planning document from March 2026.
```

- [ ] **Step 3: Commit**

```bash
git add docs/ROADMAP-1.0-original.md docs/ROADMAP-1.0.md
git commit -m "docs: archive original roadmap, write fresh version reflecting v0.11"
```

---

### Task 7: Rewrite `README.md`

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Rewrite README as a landing page**

Replace the entire contents of `README.md` with:

```markdown
# bluback

A CLI/TUI tool for backing up Blu-ray discs to MKV files using FFmpeg library bindings + libaacs. All rips are lossless remuxes — no re-encoding.

## Features

- **Lossless remux** — stream copy via FFmpeg, no quality loss
- **TUI wizard** — interactive disc scanning, TMDb lookup, episode assignment, progress dashboard
- **Headless CLI** — fully scriptable with `--yes`, `--title`, `--playlists`
- **TMDb integration** — automatic show/episode metadata lookup and naming
- **Chapter preservation** — Blu-ray chapter markers embedded during remux
- **Per-stream track selection** — filter by language, format, or manual selection
- **Batch mode** — continuous multi-disc ripping with episode auto-advance
- **Multi-drive support** — parallel sessions with per-drive tabs in TUI
- **Rip verification** — post-remux header and frame validation
- **Post-rip hooks** — configurable shell commands after each rip or session
- **MKV metadata embedding** — auto-generated tags (title, show, season, episode)
- **Rip history** — SQLite database with episode continuation and duplicate detection
- **Auto-detection** — heuristic identification of specials and multi-episode playlists
- **AACS backend selection** — libaacs (KEYDB.cfg) or libmmbd (MakeMKV LibreDrive)
- **Linux + macOS**

## Requirements

- FFmpeg shared libraries (libavformat, libavcodec, libavutil, etc.) with libbluray support
- libaacs with a populated `~/.config/aacs/KEYDB.cfg`
- A Blu-ray drive (Linux: `/dev/sr0`; macOS: `/dev/diskN`)
- Optional: [TMDb API key](https://www.themoviedb.org/settings/api) for episode metadata

## Installation

### From crates.io

```bash
cargo install bluback
```

### Pre-built binaries

Linux x86_64 and aarch64 binaries are available on the [releases page](https://github.com/cebarks/bluback/releases).

### From source

Requires FFmpeg development libraries and clang for FFI binding generation:

| Distro | Packages |
|--------|----------|
| **Fedora/RHEL** | `sudo dnf install ffmpeg-free-devel clang clang-libs pkg-config` |
| **Ubuntu/Debian** | `sudo apt install libavformat-dev libavcodec-dev libavutil-dev libswscale-dev libswresample-dev libavfilter-dev libavdevice-dev pkg-config clang libclang-dev` |
| **Arch** | `sudo pacman -S ffmpeg clang pkgconf` |
| **macOS** | See [macOS Installation Guide](docs/macos-installation.md) |

```bash
git clone https://github.com/cebarks/bluback.git
cd bluback
cargo build --release
# Binary at target/release/bluback
```

## Quick Start

```bash
# Auto-detect drive, interactive TUI
bluback

# Specify device and output directory
bluback -d /dev/sr0 -o ~/rips

# Movie mode
bluback --movie -o ~/movies

# Continuous batch ripping
bluback --batch -o ~/rips

# Validate environment setup
bluback --check

# List playlists with stream details
bluback --list-playlists -v
```

## Configuration

Config file at `~/.config/bluback/config.toml`. Edit interactively with `bluback --settings` or `Ctrl+S` in the TUI. Settings can also be overridden via `BLUBACK_*` environment variables.

See [Configuration](docs/configuration.md) for full reference.

## Documentation

| Guide | Description |
|-------|-------------|
| [Configuration](docs/configuration.md) | Config file, environment variables, filename templates |
| [CLI Reference](docs/cli-reference.md) | All flags, history subcommand, exit codes |
| [Features Guide](docs/features.md) | Stream selection, verification, batch mode, hooks, history, and more |
| [TUI Keybindings](docs/keybindings.md) | Keyboard shortcuts and screen flow |
| [macOS Installation](docs/macos-installation.md) | Homebrew setup, FFmpeg rebuild, troubleshooting |
| [Roadmap](docs/ROADMAP-1.0.md) | Release history and upcoming milestones |

## AACS Decryption

bluback relies on libaacs for AACS decryption. You need a `KEYDB.cfg` file at `~/.config/aacs/KEYDB.cfg` containing device keys, processing keys, and/or per-disc Volume Unique Keys. The KEYDB can be sourced from the [FindVUK Online Database](http://fvonline-db.bplaced.net/).

Discs with MKBv72+ MKBs may require a per-disc VUK in the KEYDB, or the libmmbd backend (`--aacs-backend libmmbd`) with a registered MakeMKV installation. See the [macOS guide](docs/macos-installation.md) for libmmbd setup.

## AI Disclosure

Portions of this codebase were developed with the assistance of generative AI (Claude Code). This project was started as a test of Claude Code's limits, but turned out to be a genuinely useful tool.

## License

[AGPL-3.0-or-later](LICENSE)
```

- [ ] **Step 2: Verify line count is in target range**

Run: `wc -l README.md`
Expected: ~120-150 lines.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: rewrite README as concise landing page with links to docs/"
```

---

### Task 8: Trim `CLAUDE.md`

**Files:**
- Modify: `CLAUDE.md`

Remove sections that are now in docs/ or are derivable from code. Keep architecture, design decisions (non-obvious only), testing summary, build info, and AACS details.

- [ ] **Step 1: Remove the "Config Fields" section (lines 61-67)**

Delete the entire `## Config Fields` section. This is now in `docs/configuration.md`.

- [ ] **Step 2: Remove the "CLI Flags" section (lines 207-281)**

Delete everything from `## CLI Flags` through the end of the `--no-history` and `--ignore-history` notes. This is now in `docs/cli-reference.md`.

- [ ] **Step 3: Remove the "Exit Codes" subsection (lines 283-291)**

Delete the `### Exit Codes` table. This is now in `docs/cli-reference.md`.

- [ ] **Step 4: Remove the "TUI Keybindings" section (lines 293-347)**

Delete everything from `## TUI Keybindings` through the History overlay keybindings. This is now in `docs/keybindings.md`.

- [ ] **Step 5: Remove the "Dependencies" section (lines 349-365)**

Delete the `## Dependencies` table. `Cargo.toml` is authoritative.

- [ ] **Step 6: Trim "Key Design Decisions" — remove user-facing feature descriptions**

Remove these entries from the Key Design Decisions section (they describe straightforward user-facing features now covered in `docs/features.md`, `docs/keybindings.md`, or `docs/configuration.md`):

- "All playlists visible" (line 144)
- "TMDb API key" (line 145)
- "Settings overlay" (line 146)
- "Config path resolution" (line 147)
- "Environment variable overrides" (line 148)
- "`--settings` standalone mode" (line 149)
- "Overwrite protection" (line 152)
- "Config validation" (line 153)
- "Structured exit codes" (line 154)
- "Setup validation" (line 155)
- "Headless progress" (line 156)

Keep these entries (they document gotchas, workarounds, or non-obvious rationale):

- "Disc auto-detect" — documents polling behavior not obvious from UI
- "Blocking I/O" — architectural decision
- "Chapter preservation" — documents mount/unmount lifecycle
- "MKV metadata embedding" — documents the `REMUXED_WITH` vs `ENCODER` workaround
- "Per-stream track selection" — documents the two-layer resolution
- "MKV index reservation" — documents the `reserve_index_space` trade-off
- "libbluray stderr suppression" — critical gotcha
- "Episode assignment" — documents the median/threshold algorithm
- "Playlist ordering" — documents index.bdmv usage
- "Episode reassignment on special changes" — documents reassignment logic
- "Specials support" — documents naming format decision
- "Signal handling" — documents double-signal and cleanup behavior
- "MountGuard" — documents RAII + explicit cleanup pattern
- "Post-rip hooks" — documents the shell injection debt
- "Rip verification" — documents tolerance and behavior differences
- "Batch mode" — documents episode auto-advance and conflicts
- "Rip history" — documents WAL mode, stale cleanup, argv pre-check
- "Auto-detection" — documents heuristic layers and confidence

- [ ] **Step 7: Fix stale references in remaining content**

In CLAUDE.md, find and fix:
- `min_duration` → `min_probe_duration` (in any remaining mentions)
- Line 64 reference to `min_duration` in config fields is already deleted in step 1
- Line 144 "All playlists visible" references `min_duration` — already deleted in step 6
- Line 153 "Config validation" references `min_duration > 0` — already deleted in step 6
- Check the Architecture Data Flow and other remaining sections for any `min_duration` references

- [ ] **Step 8: Update test counts if stale**

Run: `cargo test 2>&1 | grep "^test result:"` and sum the passed counts.
Compare against the "Unit tests (625)" and "Integration tests (20)" claims in the Testing section. Update the numbers if they've changed.

- [ ] **Step 9: Verify the trimmed file**

Run: `wc -l CLAUDE.md`
Expected: ~230-260 lines (down from 365).

- [ ] **Step 10: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: trim CLAUDE.md — remove sections now in docs/, fix stale references"
```

---

### Task 9: Review `docs/macos-installation.md`

**Files:**
- Modify: `docs/macos-installation.md` (if changes needed)

- [ ] **Step 1: Fix the placeholder version in the release download section**

At line 95-96 of `docs/macos-installation.md`, the download URL has `vVERSION` as a placeholder. Replace with a note to check the releases page:

Change:
```markdown
```bash
# Download (replace VERSION with actual version)
curl -LO https://github.com/cebarks/bluback/releases/download/vVERSION/bluback-aarch64-apple-darwin.tar.gz
tar xzf bluback-aarch64-apple-darwin.tar.gz
sudo mv bluback /usr/local/bin/
```
```

To:
```markdown
Download the latest macOS binary from the [releases page](https://github.com/cebarks/bluback/releases) and extract it:

```bash
tar xzf bluback-aarch64-apple-darwin.tar.gz
sudo mv bluback /usr/local/bin/
```
```

- [ ] **Step 2: Add `cargo install` as an alternative**

After the "From GitHub Releases" section and before "From Source", add:

```markdown
### From crates.io

```bash
cargo install bluback
```

Requires FFmpeg development libraries and clang (see [From Source](#from-source) below for dependencies).
```

- [ ] **Step 3: Commit (if changes were made)**

```bash
git add docs/macos-installation.md
git commit -m "docs: fix macOS guide version placeholder, add cargo install option"
```

---

### Task 10: Final verification

- [ ] **Step 1: Verify all links between documents are valid**

Check that all relative links in README.md resolve:

```bash
grep -oP '\(docs/[^)]+\)' README.md | tr -d '()' | while read f; do
  [ -f "$f" ] && echo "OK: $f" || echo "MISSING: $f"
done
```

Expected: All links report OK.

- [ ] **Step 2: Check cross-references within docs/**

```bash
grep -rhoP '\[[^\]]+\]\([^)]+\.md\)' docs/*.md | grep -v http | sort -u
```

Verify any relative links between docs pages point to files that exist.

- [ ] **Step 3: Verify no content was accidentally lost**

Spot-check that key content from the old README exists somewhere:
- Stream selection config example → `docs/features.md`
- `--verify` flag → `docs/cli-reference.md`
- `Ctrl+S` keybinding → `docs/keybindings.md`
- Env vars table → `docs/configuration.md`
- Exit codes → `docs/cli-reference.md`

- [ ] **Step 4: Run pre-commit checks**

```bash
cargo test && rustup run stable cargo fmt -- --check && cargo clippy -- -D warnings
```

Expected: All pass (no code changes, but good to verify nothing was accidentally modified).

- [ ] **Step 5: Final commit count check**

```bash
git log --oneline HEAD~10..HEAD
```

Expected: ~8-9 commits from this work.
