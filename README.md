# bluback

A CLI/TUI tool for backing up Blu-ray discs to MKV files using ffmpeg + libaacs, with optional TMDb integration for automatic episode naming.

Supports TV shows (sequential or manual episode assignment, including multi-episode playlists) and movies. All rips are lossless remuxes (`-c copy`) — no re-encoding. Automatically preserves Blu-ray chapter markers when mkvpropedit is available.

## Why not MakeMKV?

MakeMKV doesn't work reliably with USB Blu-ray drives using ASMedia USB-SATA bridge chips (e.g., ASUS BW-16D1X-U). The bridge mangles SCSI passthrough commands needed for disc access. Standard block-level reads via `/dev/sr0` work fine, so bluback uses ffprobe/ffmpeg with libbluray instead.

## Requirements

- **ffmpeg** and **ffprobe** (with libbluray support)
- **libaacs** with a populated `~/.config/aacs/KEYDB.cfg` (containing device keys, processing keys, and/or per-disc VUKs)
- A Blu-ray drive accessible as a block device (e.g., `/dev/sr0`)

Optional:
- **mkvpropedit** (from [mkvtoolnix](https://mkvtoolnix.download/)) — for preserving Blu-ray chapter markers in ripped MKVs
- A [TMDb API key](https://www.themoviedb.org/settings/api) for automatic show/episode metadata lookup

## Installation

### From crates.io

```bash
cargo install bluback
```

### From source

```bash
git clone https://github.com/cebarks/bluback.git
cd bluback
cargo build --release
# Binary at target/release/bluback
```

## Usage

```
bluback [OPTIONS]
```

By default, bluback auto-detects your Blu-ray drive and launches a TUI wizard that walks you through the ripping process.

### Examples

```bash
# Auto-detect drive, interactive TUI
bluback

# Specify device and output directory
bluback -d /dev/sr0 -o ~/rips

# TV show: pre-set season 2, starting at episode 5
bluback -s 2 -e 5 -o ~/rips

# Movie mode
bluback --movie -o ~/movies

# Dry run (show what would be ripped)
bluback --dry-run

# Use Plex-style filenames
bluback --format-preset plex -o ~/media

# Custom filename template
bluback --format "S{season}E{episode}_{title}.mkv"

# Plain text mode (no TUI)
bluback --no-tui

# Open settings panel (no disc required)
bluback --settings

# Use a custom config file
bluback --config ~/my-config.toml
```

### Options

| Flag | Description |
|---|---|
| `-d, --device <PATH>` | Blu-ray device path (default: auto-detect) |
| `-o, --output <DIR>` | Output directory (default: `.`) |
| `-s, --season <NUM>` | Season number (skips prompt) |
| `-e, --start-episode <NUM>` | Starting episode number (skips prompt) |
| `--min-duration <SECS>` | Minimum seconds for episode detection (default: 900) |
| `--movie` | Movie mode (skip episode assignment) |
| `--format <TEMPLATE>` | Custom filename template |
| `--format-preset <NAME>` | Built-in preset: `default`, `plex`, `jellyfin` |
| `--dry-run` | Show what would be ripped |
| `--no-tui` | Plain text mode (auto if not a TTY) |
| `--eject` | Eject disc after successful rip |
| `--no-eject` | Don't eject disc after rip (overrides config) |
| `--no-max-speed` | Don't set drive to maximum read speed |
| `--settings` | Open settings panel (no disc/ffmpeg required) |
| `--config <PATH>` | Path to config file (also: `BLUBACK_CONFIG` env var) |

## Configuration

Config file: `~/.config/bluback/config.toml` (override with `--config <PATH>` or `BLUBACK_CONFIG` env var)

You can edit the config interactively with `bluback --settings` or by pressing `Ctrl+S` during any TUI screen. Saving from the settings panel writes all fields with defaults commented out for reference.

```toml
# Default output directory
# output_dir = "."

# Default device (or "auto-detect")
# device = "auto-detect"

# Auto-eject disc after rip
eject = true

# Set drive to max read speed
# max_speed = true

# Minimum playlist duration (seconds) for episode detection (default: 900)
min_duration = 900

# Filename preset: "default", "plex", or "jellyfin"
# preset = "plex"

# Or custom format templates (overrides preset)
# tv_format = "S{season}E{episode}_{title}.mkv"
# movie_format = "{title}_({year}).mkv"
# special_format = "{show} S00E{episode} {title}.mkv"

# Show playlists below min_duration by default in Playlist Manager
# show_filtered = false

# TMDb API key (also checked in ~/.config/bluback/tmdb_api_key and TMDB_API_KEY env var)
# tmdb_api_key = "your-key-here"
```

### Environment Variables

Settings can also be set via environment variables. When the settings panel opens, it detects and imports any set `BLUBACK_*` variables:

| Variable | Config Key |
|---|---|
| `BLUBACK_OUTPUT_DIR` | `output_dir` |
| `BLUBACK_DEVICE` | `device` |
| `BLUBACK_EJECT` | `eject` |
| `BLUBACK_MAX_SPEED` | `max_speed` |
| `BLUBACK_MIN_DURATION` | `min_duration` |
| `BLUBACK_PRESET` | `preset` |
| `BLUBACK_TV_FORMAT` | `tv_format` |
| `BLUBACK_MOVIE_FORMAT` | `movie_format` |
| `BLUBACK_SPECIAL_FORMAT` | `special_format` |
| `BLUBACK_SHOW_FILTERED` | `show_filtered` |
| `TMDB_API_KEY` | `tmdb_api_key` |

Environment variables take precedence over config file values at runtime. When saving, a warning notes which env vars will override the saved config.

### Filename Templates

Templates use `{placeholder}` syntax. Available placeholders: `{show}`, `{season}`, `{episode}`, `{title}`, `{year}`, `{resolution}`, `{audio}`, `{channels}`, `{codec}`.

Bracket groups `[...]` auto-collapse when their contents are empty (useful for optional metadata).

**Priority chain** (highest to lowest): `--format` CLI flag > `--format-preset` > `tv_format`/`movie_format` in config > `preset` in config > `"default"` preset.

## TUI Keybindings

| Key | Action |
|---|---|
| `Enter` | Confirm / submit / search / select |
| `Esc` | Go back / skip TMDb / cancel edit |
| `Up/Down` | Navigate lists / scroll episodes |
| `Space` | Toggle playlist selection |
| `Tab` | Toggle movie/TV mode (TMDb search) |
| `e` | Edit episode assignment inline |
| `s` | Toggle special (season 0) marking |
| `r` / `R` | Reset current / all episode assignments |
| `f` | Show/hide filtered (short) playlists |
| `Ctrl+S` | Open settings panel |
| `Ctrl+R` | Rescan disc and restart wizard |
| `Ctrl+E` | Eject disc |
| `Ctrl+C` | Quit immediately |
| `q` | Quit (except during input/ripping) |

## Chapter Preservation

When `mkvpropedit` (from mkvtoolnix) is installed, bluback automatically extracts chapter markers from the Blu-ray's MPLS playlist files and embeds them into the ripped MKV files. The disc is temporarily mounted via `udisksctl` to read the playlist data, then unmounted after extraction.

Chapter counts are displayed alongside each playlist during selection in TUI mode. If `mkvpropedit` is not found, ripping proceeds normally without chapters.

## AACS Decryption Notes

bluback relies on libaacs for AACS decryption. You need a `KEYDB.cfg` file at `~/.config/aacs/KEYDB.cfg` containing device keys, processing keys, host certificates, and/or per-disc Volume Unique Keys (VUKs).

**USB drive caveat:** The only publicly available AACS host certificate is revoked in MKBv72+. Discs with newer MKBs require a per-disc VUK entry in the KEYDB. If ffprobe hangs during disc scanning, check for an orphaned `libmmbd.so.0` on your system — if present without a working MakeMKV backend, libaacs can hang indefinitely.

## AI Disclosure

Portions of this codebase were developed with the assistance of generative AI (Claude, Anthropic).

## License

[AGPL-3.0-or-later](LICENSE)
