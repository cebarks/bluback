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
