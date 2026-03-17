# bluback

A CLI/TUI tool for backing up Blu-ray discs to MKV files using ffmpeg + libaacs, with optional TMDb integration for automatic episode naming.

Supports TV shows (sequential or manual episode assignment, including multi-episode playlists) and movies. All rips are lossless remuxes (`-c copy`) — no re-encoding.

## Why not MakeMKV?

MakeMKV doesn't work reliably with USB Blu-ray drives using ASMedia USB-SATA bridge chips (e.g., ASUS BW-16D1X-U). The bridge mangles SCSI passthrough commands needed for disc access. Standard block-level reads via `/dev/sr0` work fine, so bluback uses ffprobe/ffmpeg with libbluray instead.

## Requirements

- **ffmpeg** and **ffprobe** (with libbluray support)
- **libaacs** with a populated `~/.config/aacs/KEYDB.cfg` (from the [FindVUK Online Database](http://fvonline-db.bplaced.net/))
- A Blu-ray drive accessible as a block device (e.g., `/dev/sr0`)

Optional:
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

## Configuration

Config file: `~/.config/bluback/config.toml`

```toml
# TMDb API key (also checked in ~/.config/bluback/tmdb_api_key and TMDB_API_KEY env var)
tmdb_api_key = "your-key-here"

# Filename preset: "default", "plex", or "jellyfin"
preset = "plex"

# Or custom format templates (overrides preset)
tv_format = "S{season}E{episode}_{title}.mkv"
movie_format = "{title}_({year}).mkv"

# Auto-eject disc after rip
eject = true

# Set drive to max read speed
max_speed = true
```

### Filename Templates

Templates use `{placeholder}` syntax. Available placeholders: `{show}`, `{season}`, `{episode}`, `{title}`, `{year}`, `{resolution}`, `{audio}`, `{channels}`, `{codec}`.

Bracket groups `[...]` auto-collapse when their contents are empty (useful for optional metadata).

**Priority chain** (highest to lowest): `--format` CLI flag > `--format-preset` > `tv_format`/`movie_format` in config > `preset` in config > `"default"` preset.

## TUI Keybindings

| Key | Action |
|---|---|
| `Enter` | Confirm / submit |
| `Esc` | Go back / skip TMDb |
| `Up/Down` | Navigate lists |
| `Space` | Toggle playlist selection |
| `Tab` | Switch fields / toggle movie-TV mode |
| `e` | Edit episode assignment (mapping screen) |
| `Ctrl+R` | Rescan disc and restart wizard |
| `Ctrl+E` | Eject disc |
| `Ctrl+C` | Quit immediately |
| `q` | Quit (except during input/ripping) |

## AACS Decryption Notes

bluback relies on libaacs for AACS decryption. You need a `KEYDB.cfg` file at `~/.config/aacs/KEYDB.cfg` containing device keys, processing keys, host certificates, and/or per-disc Volume Unique Keys (VUKs).

**USB drive caveat:** The only publicly available AACS host certificate is revoked in MKBv72+. Discs with newer MKBs require a per-disc VUK entry in the KEYDB. If ffprobe hangs during disc scanning, check for an orphaned `libmmbd.so.0` on your system — if present without a working MakeMKV backend, libaacs can hang indefinitely.

## AI Disclosure

Portions of this codebase were developed with the assistance of generative AI (Claude, Anthropic).

## License

[AGPL-3.0-or-later](LICENSE)
