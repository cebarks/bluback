# bluback TODO

See [docs/ROADMAP-1.0.md](docs/ROADMAP-1.0.md) for the full 1.0 roadmap (37 items, 8 milestones).

## Current: v0.6 — Stability & Safety

- [ ] Fix `detect_optical_drives()` panic on empty vec
- [ ] Error handling audit — replace production `.unwrap()` with proper propagation
- [ ] Signal handling + partial file cleanup on Ctrl+C/error during remux
- [ ] Overwrite protection — `--overwrite` flag, default skip with warning
- [ ] TMDb request timeout (15s)
- [ ] Config validation on load — warn on unknown keys, validate values
- [ ] Structured exit codes (0/1/2/3/4)
- [ ] Output directory auto-creation

## Next: v0.7 — Architecture & CLI Completeness

- [ ] Workflow extraction (`workflow.rs` with `WorkflowUI` trait) for GUI-readiness
- [ ] Specials: CLI parity (`--specials`) + TUI batch marking
- [ ] Headless progress output (line-based for non-TTY)
- [ ] `--list-playlists` stream info (codec details)
- [ ] `--check` setup validation

## Upcoming Milestones

- **v0.8** — Quality of Life: log files, pause/resume, MKV metadata, post-rip hooks, rip verification, per-stream track selection
- **v0.9** — DVD Support: disc type abstraction, title enumeration, chapter extraction, CSS errors
- **v0.10** — UHD Blu-ray: AACS 2.0, HDR metadata verification
- **v0.11** — Multi-Drive & Automation: parallel ripping, batch mode, disc history
- **v0.12** — Intelligence & Distribution: TMDb S00 auto-matching, shell completions, man page
- **v1.0** — Final Release: README rewrite, investigation spikes, integration testing

## Post-1.0

- Resume partial rips (investigate FFmpeg MKV muxer seek support)
- macOS/Windows support (platform abstraction for udisksctl/lsblk/eject)
- GUI frontend (architecture prepared via v0.7 workflow extraction)
- Desktop notifications on rip completion

## Done

- ~~full headless run via CLI (no user input needed)~~ Done: --yes/-y flag with --title, --year, --playlists, --list-playlists
- ~~pure Rust MKV/ffprobe integration~~ Done: migrated to `ffmpeg-the-third` library bindings
    - ~~ffmpeg bindings~~ Done: all probe/remux via FFmpeg API
    - ~~chapter writing via `mkv-element` crate~~ Done: chapters injected via AVChapter during remux
