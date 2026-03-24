# bluback 1.0 Roadmap

## Context

bluback is at v0.5.0 with solid core functionality (FFmpeg-based Blu-ray remux, TUI wizard, headless CLI, chapter preservation, TMDb integration). A comprehensive audit identified stability gaps, feature gaps, and architectural improvements needed for a production-quality 1.0 release. The goal is a feature-complete release delivered through incremental milestone releases, with architecture that supports a future GUI frontend.

## Architectural Principles

- **TUI is the primary interactive interface.** The CLI is declarative — users specify exactly what they want via flags, no interactive discovery needed.
- **GUI-readiness.** All business logic must be separated from presentation. Core workflow orchestration lives in a shared library layer (`workflow.rs`), with TUI and CLI as thin adapters. Progress reporting, user prompts, and status updates use trait-based callbacks.
- **Each milestone is a usable release.** No milestone should leave the tool in a broken intermediate state.

## Milestone Overview

| Version | Theme | Items |
|---------|-------|-------|
| **v0.6** | Stability & Safety | Bug fixes, error handling, signal handling, overwrite, exit codes, output dir auto-creation |
| **v0.7** | Architecture & CLI Completeness | Workflow extraction, specials CLI, headless progress, `--check`, `--list-playlists` stream info |
| **v0.8** | Quality of Life | Log files, pause/resume, MKV metadata, post-rip hooks, rip verification, per-stream track selection |
| **v0.9** | DVD Support | Disc type abstraction, title enumeration, chapter extraction, CSS errors |
| **v0.10** | UHD Blu-ray | AACS 2.0, HDR metadata verification |
| **v0.11** | Multi-Drive & Automation | Parallel ripping, drive selection, continuous batch mode, disc history |
| **v0.12** | Intelligence & Distribution | TMDb S00 auto-matching, shell completions, man page |
| **v1.0** | Final Release | README rewrite, investigation spikes, integration testing, release |

---

## v0.6 — Stability & Safety Foundation

*Prerequisite for building confidently on top.*

### 1. Fix `detect_optical_drives()` panic
- **Bug:** `main.rs:157` — `drives[0]` panics if vec is empty (masked by `/dev/sr0` fallback)
- **Fix:** `.first()` with proper error bail: `"No optical drives detected"`
- **Files:** `src/main.rs`, `src/disc.rs`

### 2. Error handling audit
- **Goal:** Replace production `.unwrap()` with proper error propagation or `.expect()` with context
- **Key targets:** `types.rs:660-821` (13 settings state unwraps), `cli.rs:754,785` (file_name unwraps)
- **Approach:** Grep all `.unwrap()` outside `#[cfg(test)]`, evaluate each
- **Files:** All `src/**/*.rs`

### 3. Signal handling + partial file cleanup
- **Goal:** Delete partial MKV files on Ctrl+C/error during remux; ensure disc unmount on all exit paths
- **Design:** Track output path in remux context; delete partial file on `RemuxFailed`/`Cancelled` in caller; register panic hook for cleanup
- **Files:** `src/media/remux.rs`, `src/rip.rs`, `src/cli.rs`, `src/tui/dashboard.rs`

### 4. Overwrite protection
- **Goal:** `--overwrite` CLI flag + `overwrite` config option (default: false)
- **Behavior:** Without flag: skip existing files with clear warning and file size. With flag: delete and re-rip.
- **Files:** `src/main.rs`, `src/config.rs`, `src/cli.rs`, `src/tui/dashboard.rs`

### 5. TMDb request timeout
- **Fix:** `.timeout(Duration::from_secs(15))` on all ureq calls
- **Files:** `src/tmdb.rs`

### 6. Config validation on load
- **Goal:** Warn on unknown keys (typos), validate values (`min_duration > 0`, `output_dir` writable)
- **Behavior:** Warnings to stderr, don't fail (forward-compat)
- **Files:** `src/config.rs`

### 7. Structured exit codes
- **Codes:** `0` success, `1` runtime error, `2` usage/config error, `3` no disc/device, `4` user cancelled
- **Implementation:** Explicit `std::process::exit()` in `main()` based on error type
- **Files:** `src/main.rs`, `src/media/error.rs`

### 8. Output directory auto-creation
- **Goal:** Auto-create parent directories when output path doesn't exist
- **Fix:** `std::fs::create_dir_all()` before writing output file
- **Files:** `src/media/remux.rs` or `src/rip.rs` (wherever output path is resolved)

---

## v0.7 — Architecture & CLI Completeness

*Extract shared workflow layer; round out CLI feature parity.*

### 9. Workflow extraction (GUI-readiness)
- **Goal:** Extract orchestration logic from `cli.rs` into shared `workflow.rs` module
- **What moves:** TMDb lookup flow, playlist selection/filtering, filename generation, rip orchestration
- **Design:** Trait-based callbacks for UI interaction (`WorkflowUI` trait)
- TUI and CLI become thin adapters implementing this trait
- **Files:** New `src/workflow.rs`, refactor `src/cli.rs`, refactor `src/tui/mod.rs`

### 10. Specials: CLI parity + batch marking
- **CLI:** `--specials <SEL>` flag (e.g., `--specials 3,5`) marks playlists as S00 episodes
- **TUI:** Batch marking — select multiple rows, press `s` to toggle all
- **Headless:** Auto-assign S00E01, S00E02, etc. to specified playlists
- **Files:** `src/main.rs`, `src/cli.rs`, `src/tui/wizard.rs`, `src/util.rs`

### 11. Headless progress output
- **Goal:** Non-TTY stdout gets line-based progress instead of `\r` carriage returns
- **Design:** Print `[playlist] 45% 120MB/s ETA 2:30` lines at intervals (every 5% or 10s)
- **Files:** `src/rip.rs`, `src/cli.rs`

### 12. `--list-playlists` stream info
- **Goal:** Show video codec, resolution, audio codecs/channels per playlist
- **Design:** Per-playlist FFmpeg probe. Default: duration/size. `--verbose`: codec details.
- **Files:** `src/cli.rs`, `src/media/probe.rs`

### 13. `--check` setup validation
- **Goal:** Validate environment without requiring a disc
- **Checks:** FFmpeg libs, libaacs, KEYDB.cfg, optical drives, output dir writable, TMDb API key
- **Output:** Checklist with pass/fail/warn per item
- **Files:** `src/main.rs`, `src/disc.rs`, `src/config.rs`

---

## v0.8 — Quality of Life

*Features that make daily use more pleasant and reliable.*

### 14. Log file support
- `--log-file <PATH>` or auto-log to `~/.local/share/bluback/logs/`
- Captures: FFmpeg, libbluray, disc detection, AACS, rip progress
- `--log-level` or config for verbosity; rotate (keep last 10)
- **Files:** New `src/logging.rs`, `src/main.rs`, `src/media/probe.rs`

### 15. Pause/resume during ripping
- `AtomicBool` pause flag; remux loop sleeps until unpaused
- TUI: `p` to toggle, "PAUSED" indicator
- **Files:** `src/media/remux.rs`, `src/rip.rs`, `src/tui/dashboard.rs`

### 16. MKV metadata embedding
- Write title, season, episode, show name into MKV container metadata
- Set `AVFormatContext` metadata dict before `write_header()`
- **Files:** `src/media/remux.rs`, `src/types.rs`

### 17. Post-rip hooks
- `post_rip_command` config with template variables (`{file}`, `{title}`, `{season}`, `{episode}`)
- Run via `std::process::Command`; don't fail rip on hook failure
- **Files:** `src/config.rs`, `src/workflow.rs`

### 18. Rip verification
- Post-remux: probe output file, compare expected vs actual duration, verify streams present
- Warn on mismatch; option to auto-delete failed files
- **Files:** New `src/verify.rs`, `src/rip.rs`

### 19. Per-stream track selection
- **TUI:** Track picker with codec, language, channels; checkboxes
- **CLI:** `--audio "eng,5.1"` / `--subtitle "eng"` flags
- **Config:** `audio_languages`, `subtitle_languages` defaults
- **Files:** `src/media/remux.rs`, `src/tui/wizard.rs`, `src/main.rs`, `src/config.rs`

---

## v0.9 — DVD Support

*Requires its own detailed design spec before implementation.*

### 20. Disc type abstraction
- `enum DiscType { BluRay, Dvd }`, `DiscInfo` struct
- Detection via filesystem probe (`BDMV/` vs `VIDEO_TS/`) or protocol attempt
- **Files:** `src/disc.rs`, `src/media/probe.rs`, `src/main.rs`

### 21. DVD title enumeration
- FFmpeg `dvd://` protocol; log capture or sequential probing fallback
- **Files:** `src/media/probe.rs`

### 22. DVD chapter extraction
- Preferred: FFmpeg `AVChapter` from `dvd://` inputs
- Fallback: minimal IFO parser or libdvdread FFI
- **Files:** `src/chapters.rs`, potentially `src/ifo.rs`

### 23. DVD error handling + volume labels
- Errors: `CssDecryptionFailed`, `DvdRegionLocked`, `DvdTitleNotFound`
- DVD label patterns (32 char max, different conventions)
- `--check` validates libdvdcss/libdvdread
- **Files:** `src/media/error.rs`, `src/disc.rs`

---

## v0.10 — UHD Blu-ray

*Verify and improve support for 4K UHD Blu-ray discs.*

### 24. AACS 2.0 investigation
- Test with physical UHD disc; document key requirements
- Determine if libaacs handles AACS 2.0 or if additional libraries needed

### 25. HDR metadata preservation
- Verify Dolby Vision, HDR10, HDR10+ metadata survives remux
- Test and fix if metadata is dropped
- **Files:** `src/media/remux.rs`, `src/media/probe.rs`

### 26. UHD-specific UX
- Show HDR type prominently in playlist info and TUI
- Warn on Dolby Vision profile compatibility issues
- **Files:** `src/tui/wizard.rs`, `src/cli.rs`

---

## v0.11 — Multi-Drive & Automation

### 27. Multi-drive detection + selection UI
- TUI: scanning screen shows all drives with status
- CLI: multiple `--device` flags or `--device all`
- **Files:** `src/disc.rs`, `src/tui/mod.rs`, `src/types.rs`

### 28. Parallel ripping
- Per-drive remux thread + mpsc; per-drive progress bars; independent cancellation
- **Files:** `src/rip.rs`, `src/tui/dashboard.rs`, `src/tui/mod.rs`, `src/workflow.rs`

### 29. Continuous batch mode
- Rip → eject → wait for next disc → auto-start
- TUI: "continuous mode" toggle; CLI: `--batch` flag
- Disc history integration: skip already-ripped discs
- **Files:** `src/tui/mod.rs`, `src/cli.rs`, `src/workflow.rs`

### 30. Disc history / rip database
- Track ripped discs (volume label, date, output files, success/failure)
- Storage: `~/.local/share/bluback/history.json`
- `--history` to list; `--force` to override duplicate detection
- **Files:** New `src/history.rs`, `src/config.rs`, `src/workflow.rs`

---

## v0.12 — Intelligence & Distribution

### 31. TMDb specials (S00) auto-matching
- Fetch season 0 from TMDb alongside regular season
- Auto-suggest marking playlists that don't match episode-length pattern
- **Files:** `src/tmdb.rs`, `src/tui/wizard.rs`, `src/workflow.rs`

### 32. Shell completions
- `clap_complete` for bash/zsh/fish; include in release artifacts
- **Files:** `Cargo.toml`, build script, `.github/workflows/release.yml`

### 33. Man page
- `clap_mangen`; include in release artifacts
- **Files:** `Cargo.toml`, build script, `.github/workflows/release.yml`

---

## v1.0 — Final Release

### 34. README rewrite
- Document all features with workflow examples
- Config reference, build/runtime requirements update

### 35. Investigation spikes
- **Resume partial rips:** Test FFmpeg MKV muxer seek support. Document for 1.1.
- **macOS/Windows:** Document platform abstraction needs. Estimate effort for 1.1+.

### 36. Integration testing
- End-to-end testing of all features; regression testing; edge cases

### 37. Release
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

### macOS / Windows Support
- Replace Linux-specific tools: `udisksctl` → `diskutil` (macOS) / WMI (Windows), `lsblk` → platform equivalents, `eject` → platform equivalents
- Platform abstraction layer in `disc.rs`
- New CI targets (macOS runners, Windows cross-compilation)
- Depends on v1.0 investigation spike for effort estimate

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
      ├─► v0.8 (quality of life)
      │    └─► v0.9 (DVD)
      │         ├─► v0.10 (UHD)
      │         └─► v0.11 (multi-drive + batch)
      │              └─► v0.12 (intelligence + distro)
      │                   └─► v1.0 (release)
      └─────────────────────────────────────────┘
```

## Key Risks

| Risk | Mitigation |
|------|-----------|
| FFmpeg `dvd://` log output unparseable | Fallback: sequential title probing |
| No Rust IFO parser for DVD chapters | Check if FFmpeg populates AVChapter from DVD input first |
| AACS 2.0 may require unavailable libraries | Document limitation; focus on discs with known VUKs |
| Parallel ripping TUI complexity | Per-drive tab UI; careful RipState decomposition |
| Scope creep | Each milestone gets its own design spec; strict scope gates |
| Workflow extraction too disruptive | Incremental; start with rip orchestration, expand |

## Process

Each milestone:
1. **Design spec** — complex features get their own brainstorming → design cycle
2. **Implementation** — feature branches per item, merge to main as complete
3. **Testing** — unit tests for pure functions, manual testing for I/O-dependent features
4. **Release** — bump version, tag, push, CI builds release artifacts
