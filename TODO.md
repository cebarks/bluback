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

## Completed: v0.7 — Architecture & CLI Completeness

- [x] Workflow extraction (`workflow.rs` with shared functions) for GUI-readiness
- [x] Specials: CLI parity (`--specials`) + TUI marking
- [x] Headless progress output (line-based for non-TTY)
- [x] `--list-playlists` stream info (codec details with `-v`)
- [x] `--check` setup validation

## Completed: v0.8 — macOS Support

- [x] Platform-specific disc operations (detect, mount, unmount, eject, volume label, speed control)
- [x] FFmpeg compatibility (pipe2 → pipe+fcntl, AVStream.side_data gating for FFmpeg 7.0+)
- [x] Fork-free disc scanning on macOS (ObjC runtime not fork-safe)
- [x] AACS library discovery with Homebrew .dylib paths + DYLD_LIBRARY_PATH
- [x] Platform-specific `--check` validation (diskutil on macOS, udisksctl on Linux)
- [x] macOS CI workflow + release builds (aarch64-apple-darwin)
- [x] macOS installation guide (`docs/macos-installation.md`)

## Completed: v0.9 — Multi-Drive & CI

- [x] DriveMonitor — background drive polling with event channel
- [x] Multi-session coordinator — spawns/kills per-drive sessions, routes input
- [x] DriveSession — per-drive rip workflow with independent state
- [x] Tab bar UI — device name, state, live rip progress per session
- [x] Inter-session linking (Ctrl+L) — copy TMDb context between sessions
- [x] Episode overlap detection across sessions
- [x] `multi_drive` config option (auto/manual)
- [x] View-based render architecture replacing App-based rendering
- [x] CI consolidation — 5-platform matrix (Ubuntu/Fedora x86_64+aarch64, macOS aarch64)
- [x] Code cleanup — dead code removal, clippy fixes

## In Progress: v0.10 — Quality of Life & Automation

- [x] Log files
- [x] MKV metadata embedding (TITLE, SHOW, SEASON_NUMBER, EPISODE_SORT, DATE_RELEASED, REMUXED_WITH + custom tags)
- [ ] Pause/resume during ripping
- [ ] Per-stream track titles (alongside per-stream track selection)
- [ ] Post-rip hooks
- [ ] Rip verification
- [ ] Per-stream track selection
- [ ] Continuous batch mode
- [ ] Disc history / rip database

## Upcoming Milestones
- **v0.11** — DVD Support: disc type abstraction, title enumeration, chapter extraction, CSS errors
- **v0.12** — UHD Blu-ray: AACS 2.0, HDR metadata verification
- **v0.13** — Intelligence & Distribution: TMDb S00 auto-matching, shell completions, man page
- **v1.0** — Final Release: README rewrite, investigation spikes, integration testing

## Post-1.0

- Resume partial rips (investigate FFmpeg MKV muxer seek support)
- Windows support (platform abstraction for WMI/PowerShell equivalents)
- GUI frontend (architecture prepared via v0.7 workflow extraction)
- Desktop notifications on rip completion
- Native LibreDrive support
