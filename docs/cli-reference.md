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
