# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

A Rust CLI/TUI tool for backing up Blu-ray discs to MKV files using ffmpeg + libaacs, with optional TMDb integration for automatic episode naming. Supports both TV shows (sequential episode assignment) and movies.

## Background & Context

### Why not MakeMKV?

MakeMKV doesn't work with USB Blu-ray drives using ASMedia USB-SATA bridge chips (e.g., ASUS BW-16D1X-U). The bridge mangles SCSI passthrough commands. MakeMKV v1.18.x also has a known bug where it silently fails to open discs on Linux with this drive (shows "No SDF" then exits). v1.17.7 reportedly works. Standard block-level reads via `/dev/sr0` work fine, so we use ffprobe/ffmpeg with libbluray instead.

### How it works

- **ffprobe** (with libbluray) scans disc playlists
- **libaacs** + `~/.config/aacs/KEYDB.cfg` handles AACS decryption
- **ffmpeg** remuxes decrypted streams into MKV (lossless, `-c copy` always, no re-encoding)

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

1. `main.rs` parses CLI args, loads config, detects TTY, dispatches to TUI or CLI mode
2. Both modes follow the same workflow: scan disc → filter playlists → TMDb lookup (optional) → assign episodes → build filenames → rip
3. `disc.rs` handles all ffprobe interactions and volume label parsing
4. `rip.rs` spawns ffmpeg processes and parses progress output via reader thread + mpsc channel
5. `util.rs` contains all pure functions (filename generation, template rendering, selection parsing)
6. `config.rs` loads TOML config from `~/.config/bluback/config.toml` and resolves filename format priority

### Two UI Modes

- **TUI mode** (default when stdout is TTY): ratatui wizard (5 screens in `tui/wizard.rs`) → progress dashboard (`tui/dashboard.rs`). State machine in `tui/mod.rs`.
- **CLI mode** (`--no-tui` or non-TTY): plain-text interactive prompts in `cli.rs`.

Both modes use the same underlying disc/rip/tmdb/util functions.

### Filename Format Resolution

Priority chain (highest to lowest): `--format` CLI flag → `--format-preset` CLI flag → `tv_format`/`movie_format` in config → `preset` in config → "default" preset. Templates use `{placeholder}` syntax with bracket groups `[...]` that auto-collapse when contents are empty.

### Key Design Decisions

- **Disc auto-detect** — When no `-d` flag is given, scans `lsblk` for all devices of type `rom` and polls each for a volume label every 2 seconds. The scanning screen shows each tried device as a dimmed log line. The device that has a disc is used for the session. Works on startup and after rescan (Ctrl+R / Enter on Done screen).
- **Blocking I/O** — no async runtime. ffmpeg progress read via reader thread + mpsc channel.
- **Audio selection**: Prefers 5.1/7.1 surround, includes stereo as secondary track. All subtitle streams included.
- **Sequential episode assignment** — user specifies starting episode, playlists assigned in order. Volume label parsing guesses the start from disc number.
- **TMDb API key**: looked up from config TOML → flat file `~/.config/bluback/tmdb_api_key` → `TMDB_API_KEY` env var.

## Testing

Unit tests live in `#[cfg(test)] mod tests` blocks within each module. All tests are for pure functions — no tests require hardware or network access.

- `util.rs` — duration parsing, filename sanitization, selection parsing, episode assignment, template rendering
- `disc.rs` — volume label parsing, playlist filtering, media info JSON parsing
- `rip.rs` — ffmpeg map arg building, progress line parsing, size/ETA estimation
- `config.rs` — TOML parsing, format resolution priority chain
- `types.rs` — MediaInfo field mapping

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
```

`--format` and `--format-preset` are mutually exclusive (clap argument group).

## TUI Keybindings

**Global (all screens):**
- `Ctrl+R` — Rescan disc and restart wizard (confirms first during ripping)
- `Ctrl+C` — Quit immediately
- `q` — Quit (except during text input or ripping)

**Wizard screens:**
- `Enter` — Confirm / submit
- `Esc` — Go back one step (or skip TMDb on search screen)
- `Up/Down` — Navigate lists
- `Space` — Toggle playlist selection
- `Tab` — Switch between fields or toggle movie/TV mode

**Ripping dashboard:**
- `q` — Abort (with confirmation)

**Done screen:**
- `Enter` — Rescan disc and restart wizard
- Any other key — Exit

## Dependencies

| Crate | Purpose |
|---|---|
| `clap` (derive) | Argument parsing |
| `ratatui` + `crossterm` | TUI framework + terminal backend |
| `ureq` | Blocking HTTP for TMDb API |
| `serde` + `serde_json` | TMDb JSON deserialization |
| `toml` | Config file parsing |
| `regex` | Volume label parsing, ffprobe output parsing |
| `anyhow` | Application error handling |
| `which` | Check for ffmpeg/ffprobe on PATH |
