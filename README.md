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
