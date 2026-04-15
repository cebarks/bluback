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
