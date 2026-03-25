# bluback TODO

See [docs/ROADMAP-1.0.md](docs/ROADMAP-1.0.md) for the full 1.0 roadmap (38 items, 8 milestones).

## Completed: v0.6 — Stability & Safety

- [x] Fix `detect_optical_drives()` panic on empty vec
- [x] Error handling audit — replace production `.unwrap()` with `.expect()` context
- [x] Signal handling (ctrlc) + partial file cleanup on Ctrl+C/error during remux
- [x] Overwrite protection — `--overwrite` flag + `PlaylistStatus::Skipped` in TUI
- [x] TMDb request timeout (15s via ureq agent)
- [x] Config validation on load — warn on unknown keys, validate values
- [x] Structured exit codes (0=success, 1=runtime, 2=usage, 3=no device, 4=cancelled)
- [x] Output directory auto-creation (error propagation fix in TUI)
- [x] Test fixtures + integration tests (synthetic media, TMDb JSON, chapter extraction)
- [x] AACS backend detection — `aacs_backend` config (auto/libaacs/libmmbd), preflight checks
- [x] MountGuard for guaranteed disc unmount on all exit paths
- [x] Zombie makemkvcon process reaping on exit

## Current: v0.7 — Architecture & CLI Completeness

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
