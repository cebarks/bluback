# bluback 1.0 Roadmap

## Context

bluback is at v0.9.0 with solid core functionality, multi-drive support, cross-platform coverage (Linux + macOS), and 5-platform CI. Core features: FFmpeg-based Blu-ray remux, TUI wizard with multi-drive tab UI, headless CLI, chapter preservation, TMDb integration, AACS backend selection, signal handling, overwrite protection. The goal is a feature-complete 1.0 release delivered through incremental milestone releases, with architecture that supports a future GUI frontend.

## Architectural Principles

- **TUI is the primary interactive interface.** The CLI is declarative — users specify exactly what they want via flags, no interactive discovery needed.
- **GUI-readiness.** All business logic must be separated from presentation. Core workflow orchestration lives in a shared library layer (`workflow.rs`), with TUI and CLI as thin adapters. Progress reporting, user prompts, and status updates use trait-based callbacks.
- **Each milestone is a usable release.** No milestone should leave the tool in a broken intermediate state.

## Milestone Overview

| Version | Theme | Items |
|---------|-------|-------|
| **v0.6** | Stability & Safety | Bug fixes, error handling, signal handling, overwrite, exit codes, output dir auto-creation |
| **v0.7** | Architecture & CLI Completeness | Workflow extraction, specials CLI, headless progress, `--check`, `--list-playlists` stream info |
| **v0.8** | macOS Support | Platform-specific disc ops, FFmpeg 7.0+ compat, fork-free scanning, Homebrew library discovery, macOS CI + release builds |
| **v0.9** | Multi-Drive & CI | Multi-drive detection, parallel sessions, tab UI, drive monitor, inter-session linking, episode overlap detection, 5-platform CI |
| **v0.10** | Quality of Life & Automation | Log files, pause/resume, MKV metadata, post-rip hooks, rip verification, per-stream track selection, continuous batch mode, disc history |
| **v0.11** | DVD Support | Disc type abstraction, title enumeration, chapter extraction, CSS errors |
| **v0.12** | UHD Blu-ray | AACS 2.0, HDR metadata verification |
| **v0.13** | Intelligence & Distribution | TMDb S00 auto-matching, shell completions, man page |
| **v1.0** | Final Release | README rewrite, investigation spikes, integration testing, release |

---

## v0.6 — Stability & Safety Foundation (RELEASED)

*Prerequisite for building confidently on top. Released 2026-03-24.*

All items complete. See `docs/superpowers/specs/2026-03-24-v0.6-stability-safety-design.md` for full design spec.

**What shipped:**
- Fix `detect_optical_drives()` panic — use `.first()` with bail, removed `/dev/sr0` fallback
- Error handling audit — 11 production `.unwrap()` → `.expect()` with context
- Signal handling — `ctrlc` crate, double-signal force exit, partial MKV cleanup, `MountGuard` for disc unmount
- Overwrite protection — `--overwrite` flag, `PlaylistStatus::Skipped` in TUI
- TMDb request timeout — 15s via `ureq::Agent` with `timeout_global`
- Config validation — unknown key detection, numeric bounds, format template brace matching
- Structured exit codes — 0 success, 1 runtime, 2 usage, 3 no device, 4 cancelled
- Output directory error propagation — TUI no longer silently swallows `create_dir_all` errors
- Test fixtures — synthetic media files, canned TMDb JSON, chapter extraction unit tests, integration tests
- **AACS backend detection** (added during v0.6) — `aacs_backend` config (auto/libaacs/libmmbd), preflight checks for makemkvcon availability, library path detection via ldconfig, improved AACS error messages, settings panel integration
- Zombie makemkvcon process reaping on exit via `waitpid`

**Key discovery:** `LIBAACS_PATH` env var must be a library NAME (`libmmbd`), not a full path — libbluray's `dl_dlopen` appends `.so.{version}`.

---

## v0.7 — Architecture & CLI Completeness (RELEASED)

*Extract shared workflow layer; round out CLI feature parity. Released 2026-03-26.*

### 10. Workflow extraction (GUI-readiness) ✓
- **Goal:** Extract orchestration logic from `cli.rs` into shared `workflow.rs` module
- **Implementation:** Extracted 3 shared functions into `src/workflow.rs` (no trait abstraction yet)
  - `build_output_filename()` — unified filename generation for CLI and TUI
  - `check_overwrite()` — file existence + overwrite decision with `OverwriteAction` enum
  - `prepare_remux_options()` — chapter extraction + RemuxOptions construction
  - `detect_movie_mode()` dropped (one-liner not worth extracting)
- CLI and TUI refactored to use workflow functions, eliminating ~150 lines of duplication
- **Deferred:** Full trait-based abstraction (`WorkflowUI`) — will be designed when GUI work begins
- **Files:** New `src/workflow.rs`, refactored `src/cli.rs`, refactored `src/tui/mod.rs`

### 11. Specials: CLI parity + batch marking ✓
- **CLI:** `--specials <SEL>` flag (e.g., `--specials 3,5`) marks playlists as specials using filtered indices
- **Naming:** Changed from `S00E{episode}` to `S{season}SP{episode}` (uses actual season, not S00)
- **TUI:** Individual marking with `s` hotkey implemented; batch marking (select multiple rows) deferred
- **Headless:** Auto-assign SP01, SP02, etc. to specified playlists
- **Files:** `src/main.rs`, `src/cli.rs`, `src/tui/wizard.rs`, `src/util.rs`

### 12. Headless progress output ✓
- **Goal:** Non-TTY stdout gets line-based progress instead of `\r` carriage returns
- **Design:** Print `[playlist] 45% 120MB/s ETA 2:30` lines at 10-second wall-clock intervals
- **Implementation:** TTY detection via `stdout().is_terminal()`, interval-based `println!` for non-TTY
- **Files:** `src/rip.rs`, `src/cli.rs`

### 13. `--list-playlists` stream info ✓
- **Goal:** Show video codec, resolution, audio codecs/channels per playlist
- **Design:** Per-playlist FFmpeg probe. Default: duration/size. `--verbose`/`-v` flag: codec details.
- **Implementation:** `--verbose` adds Video (codec, resolution, framerate) and Audio (all streams with codec + channel layout) columns
- **Files:** `src/cli.rs`, `src/media/probe.rs`

### 14. `--check` setup validation ✓
- **Goal:** Validate environment without requiring a disc
- **Checks:** 12 total — FFmpeg libs, libbluray, libaacs, KEYDB.cfg, libmmbd, makemkvcon, udisksctl, optical drives, drive permissions, output dir writable, TMDb API key, config file
- **Output:** Checklist with pass/fail/warn per item; exit code 0 (all required pass) or 2 (any required fail)
- **Implementation:** Dispatches before AACS preflight, validates all runtime dependencies
- **Files:** `src/main.rs`, `src/disc.rs`, `src/config.rs`

---

## v0.8 — macOS Support (RELEASED)

*Cross-platform support for macOS. Released 2026-03-28.*

**What shipped:**
- Platform-specific disc operations via `#[cfg(target_os)]`: `detect_optical_drives` (drutil), `get_volume_label` (diskutil info), `mount_disc`/`unmount_disc` (diskutil), `eject_disc` (diskutil eject), `set_max_speed` (no-op on macOS)
- FFmpeg compatibility: `pipe2` → `pipe`+`fcntl` (libc crate portability), `AVStream.side_data` gated behind `ff_api_avstream_side_data` cfg (removed in FFmpeg 7.0+)
- Fork-free disc scanning on macOS — Objective-C runtime crashes on `fork()` without `exec()`; macOS IOKit doesn't have the Linux D-state hang issue
- AACS library discovery with Homebrew `.dylib` paths + `DYLD_LIBRARY_PATH` injection for libbluray's dlopen
- Platform-specific `--check` validation (diskutil on macOS, udisksctl on Linux)
- macOS CI workflow + aarch64-apple-darwin release builds
- macOS installation guide (`docs/macos-installation.md`)

**Key discovery:** macOS's Objective-C runtime is not fork-safe — any process that loads ObjC frameworks (VideoToolbox, AudioToolbox via FFmpeg) will crash in the child after `fork()`. The fork-based scan isolation (for Linux kernel D-state hangs) must be skipped on macOS.

**Key discovery:** Homebrew's `/opt/homebrew/lib/` is not in macOS's default `dlopen` search path. libbluray's runtime library loading requires `DYLD_LIBRARY_PATH` or symlinks to `/usr/local/lib/`.

---

## v0.9 — Multi-Drive & CI (RELEASED)

*Multi-drive support with parallel sessions and 5-platform CI. Released 2026-03-28.*

**What shipped:**

### Multi-Drive Architecture
- **DriveMonitor** (`src/drive_monitor.rs`): Background thread polling optical drives every 2 seconds, tracking drive appearances/disappearances, disc insertions/ejections via `DriveEvent` channel
- **Coordinator** (`src/tui/coordinator.rs`): Central multi-session orchestrator — spawns/kills `DriveSession` instances based on drive monitor events, routes keyboard input to active session, handles tab switching (Tab/Shift+Tab)
- **DriveSession** (`src/session.rs`): Per-drive session encapsulating the complete rip workflow (scanning → TMDb → wizard → ripping), with independent state and configuration
- **Tab Bar** (`src/tui/tab_bar.rs`): Multi-session display showing device name, session state (Idle/Scanning/Wizard/Ripping/Done/Error), and live rip progress per session
- **Inter-Session Linking** (Ctrl+L): Copy TMDb context (show name, season, next episode) from one session to another — avoids redundant TMDb lookups across discs of the same show
- **Episode Overlap Detection**: Coordinator tracks episode assignments across sessions, warns if two sessions assign the same episode for the same show/season
- **`multi_drive` Config Option**: `"auto"` (default, TUI auto-detects all drives) or `"manual"` (single device mode)
- Core types: `SessionId`, `TabState`, `TabSummary`, `SessionCommand`, `SessionMessage`, `SharedContext`, `DriveEvent`, `Notification`

### CI Consolidation
- Consolidated `ci.yml` + `macos.yml` into single unified workflow
- Lint (fmt + clippy) runs once on Ubuntu
- Test matrix: Ubuntu x86_64/aarch64, Fedora x86_64/aarch64, macOS aarch64
- Fedora jobs use `container: fedora:43` on Ubuntu runners
- All 5 platforms are hard gates

### Code Cleanup
- Removed dead `App`-based rendering code superseded by `View`-based architecture
- Fixed clippy warnings

**Known incomplete items (deferred to future milestones):**
- `Notification` variants defined but not emitted by sessions (infrastructure ready)
- `SessionMessage::Progress` incremental updates not flowing (sessions emit full snapshots)
- Concurrent CLI mode (`// TODO(multi-drive)` in `cli.rs`)
- Per-session output directories
- Cross-session filename collision detection

---

## v0.10 — Quality of Life & Automation

*Features that make daily use more pleasant and reliable, plus batch automation.*

### 15. Log file support
- `--log-file <PATH>` or auto-log to `~/.local/share/bluback/logs/`
- Captures: FFmpeg, libbluray, disc detection, AACS, rip progress
- `--log-level` or config for verbosity; rotate (keep last 10)
- **Files:** New `src/logging.rs`, `src/main.rs`, `src/media/probe.rs`

### 16. Pause/resume during ripping
- `AtomicBool` pause flag; remux loop sleeps until unpaused
- TUI: `p` to toggle, "PAUSED" indicator
- **Files:** `src/media/remux.rs`, `src/rip.rs`, `src/tui/dashboard.rs`

### 17. MKV metadata embedding
- Write title, season, episode, show name into MKV container metadata
- Set `AVFormatContext` metadata dict before `write_header()`
- **Files:** `src/media/remux.rs`, `src/types.rs`

### 18. Post-rip hooks
- `post_rip_command` config with template variables (`{file}`, `{title}`, `{season}`, `{episode}`)
- Run via `std::process::Command`; don't fail rip on hook failure
- **Files:** `src/config.rs`, `src/workflow.rs`

### 19. Rip verification
- Post-remux: probe output file, compare expected vs actual duration, verify streams present
- Warn on mismatch; option to auto-delete failed files
- **Files:** New `src/verify.rs`, `src/rip.rs`

### 20. Per-stream track selection
- **TUI:** Track picker with codec, language, channels; checkboxes
- **CLI:** `--audio "eng,5.1"` / `--subtitle "eng"` flags
- **Config:** `audio_languages`, `subtitle_languages` defaults
- **Files:** `src/media/remux.rs`, `src/tui/wizard.rs`, `src/main.rs`, `src/config.rs`

### 30. Continuous batch mode
- Rip → eject → wait for next disc → auto-start
- TUI: "continuous mode" toggle; CLI: `--batch` flag
- Disc history integration: skip already-ripped discs
- **Files:** `src/tui/mod.rs`, `src/cli.rs`, `src/workflow.rs`

### 31. Disc history / rip database
- Track ripped discs (volume label, date, output files, success/failure)
- Storage: `~/.local/share/bluback/history.json`
- `--history` to list; `--force` to override duplicate detection
- **Files:** New `src/history.rs`, `src/config.rs`, `src/workflow.rs`

---

## v0.11 — DVD Support

*Requires its own detailed design spec before implementation.*

### 21. Disc type abstraction
- `enum DiscType { BluRay, Dvd }`, `DiscInfo` struct
- Detection via filesystem probe (`BDMV/` vs `VIDEO_TS/`) or protocol attempt
- **Files:** `src/disc.rs`, `src/media/probe.rs`, `src/main.rs`

### 22. DVD title enumeration
- FFmpeg `dvd://` protocol; log capture or sequential probing fallback
- **Files:** `src/media/probe.rs`

### 23. DVD chapter extraction
- Preferred: FFmpeg `AVChapter` from `dvd://` inputs
- Fallback: minimal IFO parser or libdvdread FFI
- **Files:** `src/chapters.rs`, potentially `src/ifo.rs`

### 24. DVD error handling + volume labels
- Errors: `CssDecryptionFailed`, `DvdRegionLocked`, `DvdTitleNotFound`
- DVD label patterns (32 char max, different conventions)
- `--check` validates libdvdcss/libdvdread
- **Files:** `src/media/error.rs`, `src/disc.rs`

---

## v0.12 — UHD Blu-ray

*Verify and improve support for 4K UHD Blu-ray discs.*

### 25. AACS 2.0 investigation
- Test with physical UHD disc; document key requirements
- Determine if libaacs handles AACS 2.0 or if additional libraries needed

### 26. HDR metadata preservation
- Verify Dolby Vision, HDR10, HDR10+ metadata survives remux
- Test and fix if metadata is dropped
- **Files:** `src/media/remux.rs`, `src/media/probe.rs`

### 27. UHD-specific UX
- Show HDR type prominently in playlist info and TUI
- Warn on Dolby Vision profile compatibility issues
- **Files:** `src/tui/wizard.rs`, `src/cli.rs`

---

## v0.13 — Intelligence & Distribution

### 32. TMDb specials (S00) auto-matching
- Fetch season 0 from TMDb alongside regular season
- Auto-suggest marking playlists that don't match episode-length pattern
- **Files:** `src/tmdb.rs`, `src/tui/wizard.rs`, `src/workflow.rs`

### 33. Shell completions
- `clap_complete` for bash/zsh/fish; include in release artifacts
- **Files:** `Cargo.toml`, build script, `.github/workflows/release.yml`

### 34. Man page
- `clap_mangen`; include in release artifacts
- **Files:** `Cargo.toml`, build script, `.github/workflows/release.yml`

---

## v1.0 — Final Release

### 35. README rewrite
- Document all features with workflow examples
- Config reference, build/runtime requirements update

### 36. Investigation spikes
- **Resume partial rips:** Test FFmpeg MKV muxer seek support. Document for 1.1.
- **Windows:** Document platform abstraction needs. Estimate effort for 1.1+. (macOS shipped in v0.8.)

### 37. Integration testing
- End-to-end testing of all features; regression testing; edge cases

### 38. Release
- Bump to 1.0.0; CHANGELOG; tag + push
- CI: completions, man page, multi-arch binaries

---

## Post-1.0

*Items discussed during 1.0 planning but deferred. Informed by v1.0 investigation spikes where noted.*

### GUI Frontend
- Architecture prepared by v0.7 workflow extraction (`WorkflowUI` trait, shared `workflow.rs`)
- Core modules (~70-80% of codebase) are already GUI-agnostic
- Remaining work: choose framework (egui, GTK, Tauri), implement UI screens, integrate background task spawning
- Estimated ~1-2 weeks of integration once framework is chosen (excluding learning curve)

### Resume Partial Rips
- Detect existing partial MKV files from a previous interrupted rip
- Offer to resume from where it left off or overwrite
- Depends on v1.0 investigation spike into FFmpeg MKV muxer seek support
- May require tracking progress externally (byte offset or timestamp) if FFmpeg can't seek into existing containers

### Windows Support
- macOS support shipped in v0.8
- Windows remains: replace Linux-specific tools with WMI/PowerShell equivalents
- Platform abstraction layer in `disc.rs` (pattern established by macOS `#[cfg]` approach)
- Windows CI targets and cross-compilation

### Desktop Notifications
- Notify via `notify-send` (Linux), native APIs (macOS/Windows) when a long rip finishes
- Useful when ripping in background; optional, off by default
- Config: `notify_on_complete = true`

### Auto-Detect Drive Read Speeds
- Populate settings dropdown with supported read speeds from the drive
- Requires SCSI/MMC GET PERFORMANCE or MODE SENSE commands
- Known limitation: unreliable through USB bridges (ASMedia chips), which is the primary use case
- May not be worth pursuing given the reliability issues

### TMDb Artwork Download
- Download poster/backdrop from TMDb alongside ripped files (`poster.jpg`, `fanart.jpg`)
- Media servers (Jellyfin, Plex) auto-populate artwork from these files
- TMDb already provides poster URLs in search results — minimal API work

### crates.io Publishing
- Publish bluback as a crate for `cargo install bluback`
- Requires stabilizing the public API surface (currently all internal)
- Consider splitting into `bluback-core` library crate + `bluback` binary crate

### Transcoding Profiles
- Optional re-encoding during rip (e.g., H.265 for space savings, AAC for compatibility)
- Profile system: "archive" (lossless, current behavior), "compact" (H.265 + AAC), "streaming" (optimized for network playback)
- Significant scope — FFmpeg encoding is much more complex than remuxing

---

## Dependency Graph

```
v0.6 (stability)
 └─► v0.7 (architecture + CLI)
      └─► v0.8 (macOS support)
           └─► v0.9 (multi-drive + CI)
                └─► v0.10 (quality of life + automation)
                     └─► v0.11 (DVD)
                          ├─► v0.12 (UHD)
                          └─► v0.13 (intelligence + distro)
                               └─► v1.0 (release)
```

## Key Risks

| Risk | Mitigation |
|------|-----------|
| FFmpeg `dvd://` log output unparseable | Fallback: sequential title probing |
| No Rust IFO parser for DVD chapters | Check if FFmpeg populates AVChapter from DVD input first |
| AACS 2.0 may require unavailable libraries | Document limitation; focus on discs with known VUKs |
| Parallel ripping TUI complexity | Per-drive tab UI; careful RipState decomposition (delivered in v0.9) |
| Scope creep | Each milestone gets its own design spec; strict scope gates |
| Workflow extraction too disruptive | Incremental; start with rip orchestration, expand |

## Process

Each milestone:
1. **Design spec** — complex features get their own brainstorming → design cycle
2. **Implementation** — feature branches per item, merge to main as complete
3. **Testing** — unit tests for pure functions, manual testing for I/O-dependent features
4. **Release** — bump version, tag, push, CI builds release artifacts
