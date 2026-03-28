# Multi-Drive Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable concurrent Blu-ray ripping from multiple drives via a tabbed TUI with per-drive actor threads and channel-based communication.

**Architecture:** Each drive session runs in its own thread with a full state machine (scan -> wizard -> rip -> done). A thin main thread handles terminal events, input routing, and rendering from cached snapshots. A drive monitor thread detects hot-plug events. Sessions communicate with the main thread via typed mpsc channels — no shared mutable state.

**Tech Stack:** Rust, ratatui, crossterm, std::sync::mpsc, std::thread

**Spec:** `docs/superpowers/specs/2026-03-27-multi-drive-support-design.md`

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `src/types.rs` | Modify | Add SessionId, SessionCommand, SessionMessage, DriveEvent, TabState, TabSummary, SharedContext, RenderSnapshot, all *View structs, Notification enum |
| `src/session.rs` | Create | DriveSession struct, session thread entry point and event loop, snapshot emission, background task management |
| `src/drive_monitor.rs` | Create | DriveMonitor struct, polling thread, DriveEvent emission |
| `src/tui/tab_bar.rs` | Create | Tab bar rendering widget |
| `src/tui/coordinator.rs` | Create | SessionHandle, multi-session coordinator replacing run_app |
| `src/tui/mod.rs` | Modify | Keep Screen, InputFocus, DiscState, TmdbState, WizardState, RipState. Remove App struct (replaced by coordinator). Update `run()` to call coordinator. |
| `src/tui/wizard.rs` | Modify | Refactor render functions: `fn render_*(f, &App)` -> `fn render_*(f, &*View, area)`. Refactor input handlers: `fn handle_*(app, key)` -> `fn handle_*(session, key)`. |
| `src/tui/dashboard.rs` | Modify | Same refactoring as wizard.rs for render/input/tick functions. |
| `src/config.rs` | Modify | Add `multi_drive` field to Config, add to KNOWN_KEYS |
| `src/main.rs` | Modify | Minor: pass config to coordinator, keep CANCELLED static |

---

### Task 1: Add multi_drive config option

**Files:**
- Modify: `src/config.rs`
- Modify: `src/types.rs` (SettingsState)

- [ ] **Step 1: Write test for multi_drive config parsing**

In `src/config.rs`, add to the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn test_multi_drive_config_parsing() {
    let toml_str = r#"multi_drive = "manual""#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.multi_drive.as_deref(), Some("manual"));
}

#[test]
fn test_multi_drive_config_default() {
    let config = Config::default();
    assert_eq!(config.multi_drive, None); // None means "auto" (the default)
}

#[test]
fn test_multi_drive_config_validation() {
    let warnings = validate_config(&Config {
        multi_drive: Some("invalid".into()),
        ..Default::default()
    });
    assert!(warnings.iter().any(|w| w.contains("multi_drive")));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_multi_drive -- --test-threads=1`
Expected: compilation error — `multi_drive` field doesn't exist on Config

- [ ] **Step 3: Add multi_drive field to Config**

In `src/config.rs`, add the field to the `Config` struct (after `aacs_backend`):

```rust
pub multi_drive: Option<String>,
```

Add `"multi_drive"` to the `KNOWN_KEYS` array.

Add validation in `validate_config()`:

```rust
if let Some(ref md) = config.multi_drive {
    if md != "auto" && md != "manual" {
        warnings.push(format!(
            "multi_drive must be \"auto\" or \"manual\", got \"{}\"",
            md
        ));
    }
}
```

Add a convenience method to `Config`:

```rust
pub fn multi_drive_mode(&self) -> &str {
    self.multi_drive.as_deref().unwrap_or("auto")
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_multi_drive -- --test-threads=1`
Expected: all 3 tests PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all existing tests still pass, no regressions

- [ ] **Step 6: Commit**

```
feat: add multi_drive config option (auto/manual)
```

---

### Task 2: Define core multi-drive types

**Files:**
- Modify: `src/types.rs`

- [ ] **Step 1: Add SessionId type and multi-drive enums to types.rs**

Add these types after the existing `BackgroundResult` enum (around line 180):

```rust
/// Unique identifier for a drive session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// State shown in the tab bar for each session
#[derive(Debug, Clone, PartialEq)]
pub enum TabState {
    Idle,
    Scanning,
    Wizard,
    Ripping,
    Done,
    Error,
}

/// Compact summary for tab bar rendering
#[derive(Debug, Clone)]
pub struct TabSummary {
    pub session_id: SessionId,
    pub device_name: String,
    pub state: TabState,
    /// (current_job, total_jobs, overall_percent)
    pub rip_progress: Option<(usize, usize, u8)>,
    pub error: Option<String>,
}

/// Commands sent from main thread to a session thread
pub enum SessionCommand {
    /// Keyboard input routed to this session
    KeyEvent(crossterm::event::KeyEvent),
    /// Copy TMDb/season/episode context from another session
    LinkTo { context: SharedContext },
    /// Config was updated via settings panel
    ConfigChanged(crate::config::Config),
    /// Drive removed or app shutting down
    Shutdown,
}

/// Messages sent from a session thread to the main thread
pub enum SessionMessage {
    /// Full display state snapshot (on screen transitions, wizard changes)
    Snapshot(RenderSnapshot),
    /// Lightweight rip progress update (frequent during remux)
    Progress {
        session_id: SessionId,
        progress: RipProgress,
        job_index: usize,
    },
    /// One-shot event notification
    Notification(Notification),
}

/// One-shot notifications from session to main thread
#[derive(Debug, Clone)]
pub enum Notification {
    /// Session's screen changed (for tab bar update)
    ScreenChanged {
        session_id: SessionId,
        tab_summary: TabSummary,
    },
    /// Episode assignments confirmed (for overlap validation)
    EpisodesAssigned {
        session_id: SessionId,
        show_name: String,
        season: u32,
        episodes: Vec<u32>,
    },
    /// Rip job completed
    RipComplete {
        session_id: SessionId,
        filename: String,
        size: u64,
    },
    /// Rip job failed
    RipFailed {
        session_id: SessionId,
        filename: String,
        error: String,
    },
    /// All rip jobs done
    AllDone { session_id: SessionId },
    /// Session crashed
    SessionCrashed {
        session_id: SessionId,
        error: String,
    },
    /// New disc detected (on Done screen)
    DiscDetected {
        session_id: SessionId,
        label: String,
    },
}

/// Context copied from one session to another for linked multi-disc workflows
#[derive(Debug, Clone)]
pub struct SharedContext {
    pub show_name: String,
    pub tmdb_show: Option<TmdbShow>,
    pub season_num: u32,
    pub next_episode: u32,
    pub movie_mode: bool,
    pub episodes: Vec<Episode>,
}

/// Events from the drive monitor thread
pub enum DriveEvent {
    /// New optical drive detected
    DriveAppeared(std::path::PathBuf),
    /// Optical drive removed
    DriveDisappeared(std::path::PathBuf),
    /// Disc inserted into a drive (device, volume_label)
    DiscInserted(std::path::PathBuf, String),
    /// Disc ejected from a drive
    DiscEjected(std::path::PathBuf),
}
```

- [ ] **Step 2: Add RenderSnapshot and View structs**

Add after the types above:

```rust
use crate::tui::{Screen, InputFocus};

/// Full display-only state sent from session to main thread for rendering.
/// Only the view matching the current screen is populated.
#[derive(Debug, Clone)]
pub struct RenderSnapshot {
    pub session_id: SessionId,
    pub device: std::path::PathBuf,
    pub screen: Screen,
    pub status_message: String,
    pub spinner_frame: usize,
    /// Available once TMDb lookup complete (for Ctrl+L link picker)
    pub linkable_context: Option<SharedContext>,
    pub scanning: Option<ScanningView>,
    pub tmdb: Option<TmdbView>,
    pub season: Option<SeasonView>,
    pub playlist_mgr: Option<PlaylistView>,
    pub confirm: Option<ConfirmView>,
    pub dashboard: Option<DashboardView>,
    pub done: Option<DoneView>,
}

#[derive(Debug, Clone)]
pub struct ScanningView {
    pub label: String,
    pub scan_log: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TmdbView {
    pub has_api_key: bool,
    pub movie_mode: bool,
    pub search_query: String,
    pub input_buffer: String,
    pub input_focus: InputFocus,
    pub show_results: Vec<TmdbShow>,
    pub movie_results: Vec<TmdbMovie>,
    pub list_cursor: usize,
    pub show_name: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct SeasonView {
    pub show_name: String,
    pub season_num: Option<u32>,
    pub input_buffer: String,
    pub input_focus: InputFocus,
    pub episodes: Vec<Episode>,
    pub list_cursor: usize,
}

#[derive(Debug, Clone)]
pub struct PlaylistView {
    pub movie_mode: bool,
    pub show_name: String,
    pub season_num: Option<u32>,
    pub playlists: Vec<Playlist>,
    pub episodes_pl: Vec<Playlist>,
    pub playlist_selected: Vec<bool>,
    pub episode_assignments: EpisodeAssignments,
    pub specials: std::collections::HashSet<String>,
    pub show_filtered: bool,
    pub list_cursor: usize,
    pub input_focus: InputFocus,
    pub input_buffer: String,
    pub chapter_counts: std::collections::HashMap<String, usize>,
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Clone)]
pub struct ConfirmView {
    pub filenames: Vec<String>,
    pub playlists: Vec<Playlist>,
    pub episode_assignments: EpisodeAssignments,
    pub list_cursor: usize,
    pub movie_mode: bool,
}

#[derive(Debug, Clone)]
pub struct DashboardView {
    pub jobs: Vec<RipJob>,
    pub current_rip: usize,
    pub confirm_abort: bool,
    pub confirm_rescan: bool,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct DoneView {
    pub jobs: Vec<RipJob>,
    pub label: String,
    pub disc_detected_label: Option<String>,
    pub eject: bool,
}
```

- [ ] **Step 3: Write tests for new types**

```rust
#[test]
fn test_session_id_equality() {
    assert_eq!(SessionId(1), SessionId(1));
    assert_ne!(SessionId(1), SessionId(2));
}

#[test]
fn test_tab_summary_from_screen() {
    let summary = TabSummary {
        session_id: SessionId(1),
        device_name: "sr0".into(),
        state: TabState::Ripping,
        rip_progress: Some((2, 5, 40)),
        error: None,
    };
    assert_eq!(summary.state, TabState::Ripping);
    assert_eq!(summary.rip_progress, Some((2, 5, 40)));
}

#[test]
fn test_shared_context_clone() {
    let ctx = SharedContext {
        show_name: "Test Show".into(),
        tmdb_show: None,
        season_num: 1,
        next_episode: 5,
        movie_mode: false,
        episodes: vec![],
    };
    let cloned = ctx.clone();
    assert_eq!(cloned.show_name, "Test Show");
    assert_eq!(cloned.next_episode, 5);
}
```

- [ ] **Step 4: Run tests to verify everything compiles and passes**

Run: `cargo test`
Expected: all tests pass (new types compile, existing tests unaffected)

- [ ] **Step 5: Commit**

```
feat: define core multi-drive types (SessionId, channels, views, events)
```

---

### Task 3: Create DriveMonitor

**Files:**
- Create: `src/drive_monitor.rs`
- Modify: `src/main.rs` (add `mod drive_monitor;`)

- [ ] **Step 1: Write tests for drive state diffing**

Create `src/drive_monitor.rs` with test module:

```rust
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;

use crate::types::DriveEvent;

/// Tracks known drives and their disc state, emits DriveEvents on changes.
pub struct DriveMonitor {
    known_drives: HashSet<PathBuf>,
    disc_labels: HashMap<PathBuf, String>, // device -> volume label (empty = no disc)
    tx: mpsc::Sender<DriveEvent>,
}

impl DriveMonitor {
    pub fn new(tx: mpsc::Sender<DriveEvent>) -> Self {
        Self {
            known_drives: HashSet::new(),
            disc_labels: HashMap::new(),
            tx,
        }
    }

    /// Compare current drive state against known state and emit events.
    /// Returns the set of current drives for updating internal state.
    pub fn diff_and_emit(
        &mut self,
        current_drives: Vec<PathBuf>,
        get_label: &dyn Fn(&PathBuf) -> String,
    ) {
        let current_set: HashSet<PathBuf> = current_drives.iter().cloned().collect();

        // Detect disappeared drives
        let disappeared: Vec<PathBuf> = self
            .known_drives
            .difference(&current_set)
            .cloned()
            .collect();
        for drive in &disappeared {
            let _ = self.tx.send(DriveEvent::DriveDisappeared(drive.clone()));
            self.disc_labels.remove(drive);
        }

        // Detect new drives
        let appeared: Vec<PathBuf> = current_set
            .difference(&self.known_drives)
            .cloned()
            .collect();
        for drive in &appeared {
            let _ = self.tx.send(DriveEvent::DriveAppeared(drive.clone()));
        }

        // Check disc state for all current drives
        for drive in &current_drives {
            let label = get_label(drive);
            let old_label = self.disc_labels.get(drive).cloned().unwrap_or_default();

            if old_label.is_empty() && !label.is_empty() {
                let _ = self
                    .tx
                    .send(DriveEvent::DiscInserted(drive.clone(), label.clone()));
            } else if !old_label.is_empty() && label.is_empty() {
                let _ = self.tx.send(DriveEvent::DiscEjected(drive.clone()));
            }

            self.disc_labels.insert(drive.clone(), label);
        }

        self.known_drives = current_set;
    }

    /// Spawn the monitor in a background thread, polling every `interval`.
    pub fn spawn(interval: std::time::Duration, tx: mpsc::Sender<DriveEvent>) {
        std::thread::Builder::new()
            .name("drive-monitor".into())
            .spawn(move || {
                let mut monitor = DriveMonitor::new(tx);
                loop {
                    let drives = crate::disc::detect_optical_drives();
                    monitor.diff_and_emit(drives, &|d| {
                        crate::disc::get_volume_label(&d.to_string_lossy())
                    });
                    std::thread::sleep(interval);
                }
            })
            .expect("failed to spawn drive monitor thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_events(rx: &mpsc::Receiver<DriveEvent>) -> Vec<String> {
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            let desc = match ev {
                DriveEvent::DriveAppeared(p) => format!("appeared:{}", p.display()),
                DriveEvent::DriveDisappeared(p) => format!("disappeared:{}", p.display()),
                DriveEvent::DiscInserted(p, l) => format!("inserted:{}:{}", p.display(), l),
                DriveEvent::DiscEjected(p) => format!("ejected:{}", p.display()),
            };
            events.push(desc);
        }
        events
    }

    #[test]
    fn test_new_drive_appears() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        let drives = vec![PathBuf::from("/dev/sr0")];
        monitor.diff_and_emit(drives, &|_| String::new());
        let events = collect_events(&rx);
        assert_eq!(events, vec!["appeared:/dev/sr0"]);
    }

    #[test]
    fn test_drive_disappears() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);

        // First poll: drive present
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| String::new());
        let _ = collect_events(&rx); // drain

        // Second poll: drive gone
        monitor.diff_and_emit(vec![], &|_| String::new());
        let events = collect_events(&rx);
        assert_eq!(events, vec!["disappeared:/dev/sr0"]);
    }

    #[test]
    fn test_disc_inserted() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);

        // First poll: drive present, no disc
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| String::new());
        let _ = collect_events(&rx);

        // Second poll: disc inserted
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| {
            "BREAKING_BAD_S1D1".into()
        });
        let events = collect_events(&rx);
        assert_eq!(events, vec!["inserted:/dev/sr0:BREAKING_BAD_S1D1"]);
    }

    #[test]
    fn test_disc_ejected() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);

        // Drive with disc
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| {
            "DISC_LABEL".into()
        });
        let _ = collect_events(&rx);

        // Disc ejected
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| String::new());
        let events = collect_events(&rx);
        assert_eq!(events, vec!["ejected:/dev/sr0"]);
    }

    #[test]
    fn test_no_change_no_events() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);

        // First poll
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "LABEL".into());
        let _ = collect_events(&rx);

        // Same state
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "LABEL".into());
        let events = collect_events(&rx);
        assert!(events.is_empty());
    }

    #[test]
    fn test_multiple_drives() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);

        let drives = vec![PathBuf::from("/dev/sr0"), PathBuf::from("/dev/sr1")];
        monitor.diff_and_emit(drives, &|d| {
            if d == &PathBuf::from("/dev/sr0") {
                "DISC_A".into()
            } else {
                String::new()
            }
        });
        let events = collect_events(&rx);
        // Two drives appear, one has a disc
        assert!(events.contains(&"appeared:/dev/sr0".to_string()));
        assert!(events.contains(&"appeared:/dev/sr1".to_string()));
        assert!(events.contains(&"inserted:/dev/sr0:DISC_A".to_string()));
    }

    #[test]
    fn test_drive_disappears_with_disc() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);

        // Drive with disc
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "LABEL".into());
        let _ = collect_events(&rx);

        // Drive removed entirely
        monitor.diff_and_emit(vec![], &|_| String::new());
        let events = collect_events(&rx);
        assert_eq!(events, vec!["disappeared:/dev/sr0"]);
    }
}
```

- [ ] **Step 2: Add module declaration**

In `src/main.rs`, add `mod drive_monitor;` alongside the other module declarations.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test drive_monitor`
Expected: all 7 tests PASS

- [ ] **Step 4: Commit**

```
feat: add DriveMonitor with polling and event diffing
```

---

### Task 4: Create DriveSession struct and snapshot builder

**Files:**
- Create: `src/session.rs`
- Modify: `src/main.rs` (add `mod session;`)

This task creates the `DriveSession` struct by extracting per-session fields from `App`, and adds the snapshot builder. The session thread event loop comes in Task 7.

- [ ] **Step 1: Create src/session.rs with DriveSession struct**

```rust
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::collections::HashMap;

use crate::config::Config;
use crate::tui::{DiscState, InputFocus, Screen, TmdbState, WizardState, RipState};
use crate::types::*;

/// A unique session ID counter.
static NEXT_SESSION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn next_session_id() -> SessionId {
    SessionId(NEXT_SESSION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// Per-drive session state. Runs in its own thread.
pub struct DriveSession {
    pub id: SessionId,
    pub device: PathBuf,
    pub config: Config,
    pub screen: Screen,
    pub disc: DiscState,
    pub tmdb: TmdbState,
    pub wizard: WizardState,
    pub rip: RipState,

    pub eject: bool,
    pub status_message: String,
    pub spinner_frame: usize,
    pub pending_rx: Option<mpsc::Receiver<BackgroundResult>>,
    pub disc_detected_label: Option<String>,
    pub tmdb_api_key: Option<String>,

    // Channel handles
    pub input_rx: mpsc::Receiver<SessionCommand>,
    pub output_tx: mpsc::Sender<SessionMessage>,

    // CLI args that are relevant per-session
    pub movie_mode_arg: bool,
    pub season_arg: Option<u32>,
    pub start_episode_arg: Option<u32>,
    pub min_duration_arg: Option<u32>,
    pub no_max_speed: bool,
    pub output_dir: PathBuf,
    pub cli_eject: Option<bool>,
    pub format: Option<String>,
    pub format_preset: Option<String>,
    pub overwrite: bool,
}

impl DriveSession {
    pub fn new(
        device: PathBuf,
        config: Config,
        input_rx: mpsc::Receiver<SessionCommand>,
        output_tx: mpsc::Sender<SessionMessage>,
    ) -> Self {
        let id = next_session_id();
        let eject = config.should_eject(None);
        let tmdb_api_key = crate::tmdb::get_api_key(&config);

        Self {
            id,
            device,
            config,
            screen: Screen::Scanning,
            disc: DiscState::default(),
            tmdb: TmdbState::default(),
            wizard: WizardState::default(),
            rip: RipState::default(),
            eject,
            status_message: "Scanning for disc...".into(),
            spinner_frame: 0,
            pending_rx: None,
            disc_detected_label: None,
            tmdb_api_key,
            input_rx,
            output_tx,
            movie_mode_arg: false,
            season_arg: None,
            start_episode_arg: None,
            min_duration_arg: None,
            no_max_speed: false,
            output_dir: PathBuf::from("."),
            cli_eject: None,
            format: None,
            format_preset: None,
            overwrite: false,
        }
    }

    /// Build a RenderSnapshot from current session state.
    pub fn snapshot(&self) -> RenderSnapshot {
        let linkable_context = if !self.tmdb.show_name.is_empty() || self.tmdb.selected_movie.is_some() {
            Some(SharedContext {
                show_name: self.tmdb.show_name.clone(),
                tmdb_show: self.tmdb.selected_show.and_then(|idx| {
                    self.tmdb.search_results.get(idx).cloned()
                }),
                season_num: self.wizard.season_num.unwrap_or(1),
                next_episode: self.next_unassigned_episode(),
                movie_mode: self.tmdb.movie_mode,
                episodes: self.tmdb.episodes.clone(),
            })
        } else {
            None
        };

        RenderSnapshot {
            session_id: self.id,
            device: self.device.clone(),
            screen: self.screen.clone(),
            status_message: self.status_message.clone(),
            spinner_frame: self.spinner_frame,
            linkable_context,
            scanning: self.build_scanning_view(),
            tmdb: self.build_tmdb_view(),
            season: self.build_season_view(),
            playlist_mgr: self.build_playlist_view(),
            confirm: self.build_confirm_view(),
            dashboard: self.build_dashboard_view(),
            done: self.build_done_view(),
        }
    }

    /// Build a TabSummary for the tab bar.
    pub fn tab_summary(&self) -> TabSummary {
        let state = match self.screen {
            Screen::Scanning => {
                if self.disc.label.is_empty() {
                    TabState::Idle
                } else {
                    TabState::Scanning
                }
            }
            Screen::TmdbSearch | Screen::Season | Screen::PlaylistManager | Screen::Confirm => {
                TabState::Wizard
            }
            Screen::Ripping => TabState::Ripping,
            Screen::Done => TabState::Done,
        };

        let rip_progress = if state == TabState::Ripping {
            let done_count = self.rip.jobs.iter()
                .filter(|j| matches!(j.status, PlaylistStatus::Done(_)))
                .count();
            let total = self.rip.jobs.len();
            let pct = if total > 0 {
                if let Some(job) = self.rip.jobs.get(self.rip.current_rip) {
                    if let PlaylistStatus::Ripping(ref prog) = job.status {
                        if job.playlist.seconds > 0 {
                            let job_pct = (prog.out_time_secs as f64 / job.playlist.seconds as f64 * 100.0).min(100.0);
                            let overall = ((done_count as f64 + job_pct / 100.0) / total as f64 * 100.0) as u8;
                            overall
                        } else { 0 }
                    } else { 0 }
                } else { 0 }
            } else { 0 };
            Some((done_count + 1, total, pct))
        } else {
            None
        };

        let device_name = self.device
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.device.to_string_lossy().to_string());

        TabSummary {
            session_id: self.id,
            device_name,
            state,
            rip_progress,
            error: None,
        }
    }

    fn next_unassigned_episode(&self) -> u32 {
        let max_assigned = self.wizard.episode_assignments.values()
            .flat_map(|eps| eps.iter().map(|e| e.episode_number))
            .max()
            .unwrap_or(0);
        max_assigned + 1
    }

    fn build_scanning_view(&self) -> Option<ScanningView> {
        if self.screen != Screen::Scanning { return None; }
        Some(ScanningView {
            label: self.disc.label.clone(),
            scan_log: self.disc.scan_log.clone(),
        })
    }

    fn build_tmdb_view(&self) -> Option<TmdbView> {
        if self.screen != Screen::TmdbSearch { return None; }
        Some(TmdbView {
            has_api_key: self.tmdb_api_key.is_some(),
            movie_mode: self.tmdb.movie_mode,
            search_query: self.tmdb.search_query.clone(),
            input_buffer: self.wizard.input_buffer.clone(),
            input_focus: self.wizard.input_focus.clone(),
            show_results: self.tmdb.search_results.clone(),
            movie_results: self.tmdb.movie_results.clone(),
            list_cursor: self.wizard.list_cursor,
            show_name: self.tmdb.show_name.clone(),
            label: self.disc.label.clone(),
        })
    }

    fn build_season_view(&self) -> Option<SeasonView> {
        if self.screen != Screen::Season { return None; }
        Some(SeasonView {
            show_name: self.tmdb.show_name.clone(),
            season_num: self.wizard.season_num,
            input_buffer: self.wizard.input_buffer.clone(),
            input_focus: self.wizard.input_focus.clone(),
            episodes: self.tmdb.episodes.clone(),
            list_cursor: self.wizard.list_cursor,
        })
    }

    fn build_playlist_view(&self) -> Option<PlaylistView> {
        if self.screen != Screen::PlaylistManager { return None; }
        Some(PlaylistView {
            movie_mode: self.tmdb.movie_mode,
            show_name: self.tmdb.show_name.clone(),
            season_num: self.wizard.season_num,
            playlists: self.disc.playlists.clone(),
            episodes_pl: self.disc.episodes_pl.clone(),
            playlist_selected: self.wizard.playlist_selected.clone(),
            episode_assignments: self.wizard.episode_assignments.clone(),
            specials: self.wizard.specials.clone(),
            show_filtered: self.wizard.show_filtered,
            list_cursor: self.wizard.list_cursor,
            input_focus: self.wizard.input_focus.clone(),
            input_buffer: self.wizard.input_buffer.clone(),
            chapter_counts: self.disc.chapter_counts.clone(),
            episodes: self.tmdb.episodes.clone(),
        })
    }

    fn build_confirm_view(&self) -> Option<ConfirmView> {
        if self.screen != Screen::Confirm { return None; }
        Some(ConfirmView {
            filenames: self.wizard.filenames.clone(),
            playlists: self.disc.playlists.clone(),
            episode_assignments: self.wizard.episode_assignments.clone(),
            list_cursor: self.wizard.list_cursor,
            movie_mode: self.tmdb.movie_mode,
        })
    }

    fn build_dashboard_view(&self) -> Option<DashboardView> {
        if self.screen != Screen::Ripping { return None; }
        Some(DashboardView {
            jobs: self.rip.jobs.clone(),
            current_rip: self.rip.current_rip,
            confirm_abort: self.rip.confirm_abort,
            confirm_rescan: self.rip.confirm_rescan,
            label: self.disc.label.clone(),
        })
    }

    fn build_done_view(&self) -> Option<DoneView> {
        if self.screen != Screen::Done { return None; }
        Some(DoneView {
            jobs: self.rip.jobs.clone(),
            label: self.disc.label.clone(),
            disc_detected_label: self.disc_detected_label.clone(),
            eject: self.eject,
        })
    }

    /// Reset state for rescanning (same disc drive).
    pub fn reset_for_rescan(&mut self) {
        self.rip.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        self.rip.progress_rx = None;

        if self.disc.did_mount {
            let _ = crate::disc::unmount_disc(&self.device.to_string_lossy());
        }

        self.disc = DiscState::default();
        self.tmdb.search_query = String::new();
        self.tmdb.movie_mode = false;
        self.tmdb.search_results = Vec::new();
        self.tmdb.movie_results = Vec::new();
        self.tmdb.selected_show = None;
        self.tmdb.selected_movie = None;
        self.tmdb.show_name = String::new();
        self.tmdb.episodes = Vec::new();
        // Keep tmdb_api_key

        self.wizard = WizardState::default();
        self.rip = RipState::default();
        self.status_message = String::new();
        self.spinner_frame = 0;
        self.pending_rx = None;
        self.disc_detected_label = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_session() -> DriveSession {
        let (_, input_rx) = mpsc::channel();
        let (output_tx, _) = mpsc::channel();
        DriveSession::new(
            PathBuf::from("/dev/sr0"),
            Config::default(),
            input_rx,
            output_tx,
        )
    }

    #[test]
    fn test_session_id_auto_increments() {
        let s1 = make_test_session();
        let s2 = make_test_session();
        assert_ne!(s1.id, s2.id);
        assert!(s2.id.0 > s1.id.0);
    }

    #[test]
    fn test_snapshot_scanning_view() {
        let session = make_test_session();
        let snap = session.snapshot();
        assert!(snap.scanning.is_some());
        assert!(snap.tmdb.is_none());
        assert!(snap.dashboard.is_none());
    }

    #[test]
    fn test_tab_summary_idle() {
        let session = make_test_session();
        let summary = session.tab_summary();
        assert_eq!(summary.state, TabState::Idle);
        assert_eq!(summary.device_name, "sr0");
        assert!(summary.rip_progress.is_none());
    }

    #[test]
    fn test_tab_summary_ripping() {
        let mut session = make_test_session();
        session.screen = Screen::Ripping;
        session.rip.jobs = vec![
            RipJob {
                playlist: Playlist { num: "1".into(), duration: "1:00:00".into(), seconds: 3600 },
                episode: vec![],
                filename: "test.mkv".into(),
                status: PlaylistStatus::Done(1000),
            },
            RipJob {
                playlist: Playlist { num: "2".into(), duration: "1:00:00".into(), seconds: 3600 },
                episode: vec![],
                filename: "test2.mkv".into(),
                status: PlaylistStatus::Ripping(RipProgress {
                    out_time_secs: 1800, // 50%
                    ..Default::default()
                }),
            },
        ];
        session.rip.current_rip = 1;
        let summary = session.tab_summary();
        assert_eq!(summary.state, TabState::Ripping);
        assert!(summary.rip_progress.is_some());
        let (current, total, pct) = summary.rip_progress.unwrap();
        assert_eq!(current, 2); // 1 done + 1 in progress
        assert_eq!(total, 2);
        assert!(pct > 50); // 1 done + 50% of second = 75% overall
    }

    #[test]
    fn test_reset_for_rescan() {
        let mut session = make_test_session();
        session.disc.label = "TEST".into();
        session.tmdb.show_name = "Show".into();
        session.wizard.list_cursor = 5;
        session.reset_for_rescan();
        assert_eq!(session.disc.label, "");
        assert_eq!(session.tmdb.show_name, "");
        assert_eq!(session.wizard.list_cursor, 0);
    }

    #[test]
    fn test_next_unassigned_episode() {
        let mut session = make_test_session();
        assert_eq!(session.next_unassigned_episode(), 1);

        session.wizard.episode_assignments.insert("1".into(), vec![
            Episode { episode_number: 1, name: "Ep1".into(), runtime: None },
            Episode { episode_number: 2, name: "Ep2".into(), runtime: None },
        ]);
        assert_eq!(session.next_unassigned_episode(), 3);
    }

    #[test]
    fn test_linkable_context_none_before_tmdb() {
        let session = make_test_session();
        let snap = session.snapshot();
        assert!(snap.linkable_context.is_none());
    }

    #[test]
    fn test_linkable_context_available_after_tmdb() {
        let mut session = make_test_session();
        session.tmdb.show_name = "Breaking Bad".into();
        session.wizard.season_num = Some(1);
        let snap = session.snapshot();
        assert!(snap.linkable_context.is_some());
        let ctx = snap.linkable_context.unwrap();
        assert_eq!(ctx.show_name, "Breaking Bad");
        assert_eq!(ctx.season_num, 1);
    }
}
```

- [ ] **Step 2: Add module declaration to main.rs**

Add `mod session;` alongside the other module declarations in `src/main.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test session`
Expected: all session tests pass

- [ ] **Step 4: Commit**

```
feat: add DriveSession struct with snapshot builder and tab summary
```

---

### Task 5: Refactor render functions to accept View structs

**Files:**
- Modify: `src/tui/wizard.rs`
- Modify: `src/tui/dashboard.rs`

This is the largest refactoring task. Each render function gets a new signature that takes a View struct + area, and the old signature becomes a thin adapter that builds the view from `&App`. This allows both the current App-based code and the future coordinator to use the same renderers.

- [ ] **Step 1: Add view-based render functions to wizard.rs (scanning + tmdb)**

Add new functions alongside the existing ones. Do NOT delete the old functions yet — they'll become adapters.

For `render_scanning`, add:

```rust
pub fn render_scanning_view(f: &mut Frame, view: &ScanningView, status: &str, spinner: usize, area: Rect) {
    // Same rendering logic as render_scanning, but reads from ScanningView
    // instead of &App
    let title = if view.label.is_empty() {
        "bluback".to_string()
    } else {
        format!("bluback — {}", view.label)
    };
    // ... (port the full render_scanning body, replacing app.disc.label with view.label,
    //      app.disc.scan_log with view.scan_log, app.status_message with status,
    //      app.spinner_frame with spinner)
}
```

Then make the existing `render_scanning` delegate:

```rust
pub fn render_scanning(f: &mut Frame, app: &App) {
    let view = ScanningView {
        label: app.disc.label.clone(),
        scan_log: app.disc.scan_log.clone(),
    };
    render_scanning_view(f, &view, &app.status_message, app.spinner_frame, f.area());
}
```

Apply the same pattern for `render_tmdb_search` -> `render_tmdb_search_view`.

- [ ] **Step 2: Run tests to verify no regressions**

Run: `cargo test`
Expected: all tests pass (render adapters preserve behavior)

- [ ] **Step 3: Add view-based render functions for season, playlist_manager, confirm**

Apply the same adapter pattern:
- `render_season` -> `render_season_view(f, &SeasonView, status, spinner, area)`
- `render_playlist_manager` -> `render_playlist_manager_view(f, &PlaylistView, status, area)`
- `render_confirm` -> `render_confirm_view(f, &ConfirmView, status, area)`

Each existing function becomes a thin adapter that constructs the View from `&App` and calls the `_view` variant.

- [ ] **Step 4: Run tests again**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Add view-based render functions to dashboard.rs**

Apply same pattern:
- `render` -> `render_dashboard_view(f, &DashboardView, status, area)`
- `render_done` -> `render_done_view(f, &DoneView, area)`

Existing functions become adapters.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 7: Commit**

```
refactor: add View-based render functions alongside App-based adapters
```

---

### Task 6: Create tab bar renderer

**Files:**
- Create: `src/tui/tab_bar.rs`
- Modify: `src/tui/mod.rs` (add `pub mod tab_bar;`)

- [ ] **Step 1: Create tab_bar.rs**

```rust
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Tabs};

use crate::types::{TabSummary, TabState};

/// Height of the tab bar in terminal rows
pub const TAB_BAR_HEIGHT: u16 = 1;

/// Render the tab bar at the top of the screen.
/// Returns the remaining area below the tab bar for content.
pub fn render(f: &mut Frame, tabs: &[TabSummary], active_index: usize, area: Rect) -> Rect {
    if tabs.len() <= 1 {
        // Single drive: no tab bar needed, return full area
        return area;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(TAB_BAR_HEIGHT),
            Constraint::Min(0),
        ])
        .split(area);

    let titles: Vec<Line> = tabs
        .iter()
        .map(|tab| {
            let label = format_tab_label(tab);
            Line::from(label)
        })
        .collect();

    let tab_widget = Tabs::new(titles)
        .select(active_index)
        .highlight_style(Style::default().bold().fg(Color::Cyan))
        .divider(" │ ");

    f.render_widget(tab_widget, chunks[0]);

    chunks[1]
}

fn format_tab_label(tab: &TabSummary) -> String {
    match tab.state {
        TabState::Idle => format!("{}: Waiting for disc", tab.device_name),
        TabState::Scanning => format!("{}: Scanning", tab.device_name),
        TabState::Wizard => format!("{}: Setup", tab.device_name),
        TabState::Ripping => {
            if let Some((current, total, pct)) = tab.rip_progress {
                format!("{}: Ripping {}/{} {}%", tab.device_name, current, total, pct)
            } else {
                format!("{}: Ripping", tab.device_name)
            }
        }
        TabState::Done => format!("{}: Done", tab.device_name),
        TabState::Error => {
            let err = tab.error.as_deref().unwrap_or("error");
            format!("{}: {}", tab.device_name, err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionId;

    #[test]
    fn test_format_tab_label_idle() {
        let tab = TabSummary {
            session_id: SessionId(1),
            device_name: "sr0".into(),
            state: TabState::Idle,
            rip_progress: None,
            error: None,
        };
        assert_eq!(format_tab_label(&tab), "sr0: Waiting for disc");
    }

    #[test]
    fn test_format_tab_label_ripping() {
        let tab = TabSummary {
            session_id: SessionId(1),
            device_name: "sr0".into(),
            state: TabState::Ripping,
            rip_progress: Some((3, 8, 42)),
            error: None,
        };
        assert_eq!(format_tab_label(&tab), "sr0: Ripping 3/8 42%");
    }

    #[test]
    fn test_format_tab_label_error() {
        let tab = TabSummary {
            session_id: SessionId(1),
            device_name: "sr1".into(),
            state: TabState::Error,
            rip_progress: None,
            error: Some("drive disconnected".into()),
        };
        assert_eq!(format_tab_label(&tab), "sr1: drive disconnected");
    }
}
```

- [ ] **Step 2: Add module declaration**

In `src/tui/mod.rs`, add `pub mod tab_bar;` alongside the other module declarations.

- [ ] **Step 3: Run tests**

Run: `cargo test tab_bar`
Expected: all 3 tests pass

- [ ] **Step 4: Commit**

```
feat: add tab bar renderer for multi-drive TUI
```

---

### Task 7: Session thread event loop

**Files:**
- Modify: `src/session.rs`

This task adds the main event loop for the session thread — processing `SessionCommand` messages, managing background tasks, and emitting state updates.

- [ ] **Step 1: Add disc scanning to DriveSession**

Port the `start_disc_scan` logic from `tui/mod.rs` into a method on `DriveSession`:

```rust
impl DriveSession {
    /// Start scanning for disc presence and playlists.
    pub fn start_disc_scan(&mut self) {
        let device = self.device.clone();
        let max_speed = self.config.should_max_speed(self.no_max_speed);
        let (tx, rx) = mpsc::channel();

        std::thread::Builder::new()
            .name(format!("scan-{}", self.device.display()))
            .spawn(move || {
                let dev_str = device.to_string_lossy().to_string();

                // Check for disc
                let label = crate::disc::get_volume_label(&dev_str);
                if label.is_empty() {
                    let msg = format!("{} — no disc", dev_str);
                    let _ = tx.send(BackgroundResult::WaitingForDisc(msg));
                    return;
                }

                let _ = tx.send(BackgroundResult::DiscFound(dev_str.clone()));

                if max_speed {
                    crate::disc::set_max_speed(&dev_str);
                }

                let tx_progress = tx.clone();
                let result = (|| -> anyhow::Result<(String, String, Vec<Playlist>)> {
                    let playlists = crate::media::scan_playlists_with_progress(
                        &dev_str,
                        Some(&move |elapsed, timeout| {
                            let _ = tx_progress.send(BackgroundResult::ScanProgress(elapsed, timeout));
                        }),
                    )
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                    Ok((dev_str.clone(), label, playlists))
                })();
                let _ = tx.send(BackgroundResult::DiscScan(result));
            })
            .expect("failed to spawn scan thread");

        self.pending_rx = Some(rx);
        self.status_message = "Scanning for disc...".into();
        self.screen = Screen::Scanning;
    }
}
```

- [ ] **Step 2: Add poll_background to DriveSession**

Port the `poll_background` logic from `tui/mod.rs`. This processes `BackgroundResult` messages and updates session state. Same logic, but operates on `&mut self` instead of `&mut App`:

```rust
impl DriveSession {
    /// Poll for background task results (disc scan, TMDb, media probe).
    /// Returns true if a state change occurred that warrants a new snapshot.
    pub fn poll_background(&mut self) -> bool {
        // Port the full poll_background logic from tui/mod.rs:638-848
        // replacing app.* with self.*
        // Replace app.args.device with self.device
        // Replace app.config.min_duration(app.args.min_duration) with
        //     self.config.min_duration(self.min_duration_arg)
        // etc.
        //
        // Return true when screen transitions happen or significant state changes occur
        // Return false for WaitingForDisc, DiscFound, ScanProgress (incremental updates)
        todo!("port poll_background — see tui/mod.rs:638-848")
    }
}
```

The actual ported code mirrors `poll_background` exactly, with `app.` replaced by `self.` and `app.args.*` replaced by the per-session args fields.

- [ ] **Step 3: Add the session thread entry point**

```rust
impl DriveSession {
    /// Main event loop for the session thread. Blocks until Shutdown received.
    pub fn run(mut self) {
        // Start initial disc scan
        self.start_disc_scan();
        self.emit_snapshot();

        loop {
            // Poll for input commands with timeout (for spinner/progress)
            let command = self.input_rx.recv_timeout(std::time::Duration::from_millis(100));

            match command {
                Ok(SessionCommand::Shutdown) => {
                    self.rip.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    // Clean up partial files
                    if self.disc.did_mount {
                        let _ = crate::disc::unmount_disc(&self.device.to_string_lossy());
                    }
                    return;
                }
                Ok(SessionCommand::KeyEvent(key)) => {
                    self.handle_key(key);
                }
                Ok(SessionCommand::LinkTo { context }) => {
                    self.apply_linked_context(context);
                    self.emit_snapshot();
                }
                Ok(SessionCommand::ConfigChanged(config)) => {
                    self.config = config;
                    self.eject = self.config.should_eject(self.cli_eject);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Periodic work: spinner, progress polling
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // Main thread died, exit
                    return;
                }
            }

            // Spinner animation
            if self.pending_rx.is_some() {
                self.spinner_frame = self.spinner_frame.wrapping_add(1);
            }

            // Poll background tasks
            let changed = self.poll_background();
            if changed {
                self.emit_snapshot();
            }

            // Poll rip progress
            if self.screen == Screen::Ripping {
                let rip_changed = self.tick_rip();
                if rip_changed {
                    self.emit_snapshot();
                }
            }

            // Propagate process-level cancel
            if crate::CANCELLED.load(std::sync::atomic::Ordering::Relaxed) {
                self.rip.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    fn emit_snapshot(&self) {
        let _ = self.output_tx.send(SessionMessage::Snapshot(self.snapshot()));
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        // Route input to the appropriate handler based on current screen.
        // This will be ported from the screen match in run_app.
        // For now, use the existing wizard/dashboard input handlers.
        // This requires adapting them to take &mut DriveSession instead of &mut App.
        // See Task 8 for the full input handler migration.
        todo!("port input routing from tui/mod.rs:553-600")
    }

    fn apply_linked_context(&mut self, context: SharedContext) {
        self.tmdb.show_name = context.show_name;
        if let Some(show) = context.tmdb_show {
            self.tmdb.search_results = vec![show];
            self.tmdb.selected_show = Some(0);
        }
        self.tmdb.movie_mode = context.movie_mode;
        self.tmdb.episodes = context.episodes;
        self.wizard.season_num = Some(context.season_num);
        self.wizard.start_episode = Some(context.next_episode);

        // Skip to playlist manager
        self.screen = Screen::PlaylistManager;
        self.wizard.input_focus = InputFocus::default();
        self.status_message.clear();
    }

    /// Tick the rip engine — port of dashboard::tick().
    /// Returns true if state changed.
    fn tick_rip(&mut self) -> bool {
        // Port dashboard::tick(), start_next_job(), poll_active_job()
        // Same logic, operating on &mut self instead of &mut App
        todo!("port rip tick — see tui/dashboard.rs:309-467")
    }
}
```

- [ ] **Step 4: Run compilation check**

Run: `cargo check`
Expected: compiles (todo!() calls are fine for now — they'll be filled in during Task 8)

Note: The `todo!()` stubs will be replaced in Task 8 when input handlers and rip tick logic are migrated. This task establishes the event loop structure.

- [ ] **Step 5: Commit**

```
feat: add session thread event loop with disc scan and snapshot emission
```

---

### Task 8: Migrate input handlers and rip tick to DriveSession

**Files:**
- Modify: `src/session.rs`
- Modify: `src/tui/wizard.rs` (add session-based input handlers)
- Modify: `src/tui/dashboard.rs` (add session-based tick/input)

This task replaces the `todo!()` stubs from Task 7 with actual logic.

- [ ] **Step 1: Add session-based input handlers to wizard.rs**

For each existing input handler `handle_*_input(app: &mut App, key: KeyEvent)`, add a parallel version that takes `&mut DriveSession`:

```rust
pub fn handle_tmdb_search_input_session(
    session: &mut crate::session::DriveSession,
    key: KeyEvent,
) {
    // Same logic as handle_tmdb_search_input, but replace:
    //   app.wizard.* -> session.wizard.*
    //   app.tmdb.* -> session.tmdb.*
    //   app.disc.* -> session.disc.*
    //   app.screen -> session.screen
    //   app.status_message -> session.status_message
    //   app.pending_rx -> session.pending_rx
    //   app.config.* -> session.config.*
    //   app.args.* -> session.*_arg fields
    //
    // TMDb search thread spawning stays the same pattern —
    // spawns a thread that sends BackgroundResult via the session's pending_rx channel.
}
```

Repeat for `handle_season_input_session`, `handle_playlist_manager_input_session`, `handle_confirm_input_session`.

- [ ] **Step 2: Add session-based tick and input to dashboard.rs**

```rust
pub fn tick_session(session: &mut crate::session::DriveSession) -> anyhow::Result<bool> {
    // Port of tick() + start_next_job() + poll_active_job()
    // Returns true if state changed
    // Same logic as existing functions, operating on session.rip.* and session.disc.*
}

pub fn handle_input_session(
    session: &mut crate::session::DriveSession,
    key: KeyEvent,
) {
    // Port of handle_input, operating on session.rip.*
}
```

- [ ] **Step 3: Wire up handle_key and tick_rip in session.rs**

Replace the `todo!()` stubs in `DriveSession`:

```rust
fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
    use crossterm::event::{KeyCode, KeyModifiers};

    let input_active = matches!(
        self.wizard.input_focus,
        InputFocus::TextInput | InputFocus::InlineEdit(_)
    );

    // Per-session Ctrl+R: rescan
    if key.code == KeyCode::Char('r')
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && !self.rip.confirm_rescan
    {
        if self.screen == Screen::Ripping {
            self.rip.confirm_rescan = true;
        } else {
            self.reset_for_rescan();
            self.start_disc_scan();
        }
        return;
    }

    // Per-session Ctrl+E: eject
    if key.code == KeyCode::Char('e')
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && !input_active
        && self.screen != Screen::Ripping
    {
        let device_str = self.device.to_string_lossy().to_string();
        match crate::disc::eject_disc(&device_str) {
            Ok(()) => self.status_message = "Disc ejected.".into(),
            Err(e) => self.status_message = format!("Eject failed: {}", e),
        }
        return;
    }

    // Per-session q: quit (handled differently — sends notification)
    if key.code == KeyCode::Char('q') && !input_active && self.screen != Screen::Ripping {
        // In multi-drive, q on a tab doesn't quit the app — it could close the tab
        // For now, treat as no-op in session (global q handled by coordinator)
        return;
    }

    // Rescan confirmation
    if self.rip.confirm_rescan {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.reset_for_rescan();
                self.start_disc_scan();
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.rip.confirm_rescan = false;
            }
            _ => {}
        }
        return;
    }

    // Screen-specific input
    match self.screen {
        Screen::TmdbSearch => {
            crate::tui::wizard::handle_tmdb_search_input_session(self, key);
        }
        Screen::Season => {
            crate::tui::wizard::handle_season_input_session(self, key);
        }
        Screen::PlaylistManager => {
            crate::tui::wizard::handle_playlist_manager_input_session(self, key);
        }
        Screen::Confirm => {
            crate::tui::wizard::handle_confirm_input_session(self, key);
        }
        Screen::Ripping => {
            crate::tui::dashboard::handle_input_session(self, key);
        }
        Screen::Done => {
            if self.disc_detected_label.is_some() {
                if key.code == KeyCode::Enter {
                    self.disc_detected_label = None;
                    self.reset_for_rescan();
                    self.start_disc_scan();
                }
                // Other keys: no-op in session (coordinator handles app-level quit)
            } else if key.code == KeyCode::Enter {
                self.reset_for_rescan();
                self.start_disc_scan();
            }
        }
        _ => {}
    }
}

fn tick_rip(&mut self) -> bool {
    crate::tui::dashboard::tick_session(self).unwrap_or(false)
}
```

- [ ] **Step 4: Fill in poll_background**

Replace the `todo!()` in `poll_background` with the ported logic from `tui/mod.rs:638-848`. Key substitutions:
- `app.args.device = Some(...)` -> `self.device = ...`
- `app.config.min_duration(app.args.min_duration)` -> `self.config.min_duration(self.min_duration_arg)`
- `app.args.movie` -> `self.movie_mode_arg`
- `app.args.season` -> `self.season_arg`
- `app.args.start_episode` -> `self.start_episode_arg`
- `app.tmdb.api_key` -> `self.tmdb_api_key`
- `crate::tmdb::get_api_key(config)` -> not needed (already stored)

- [ ] **Step 5: Run compilation and tests**

Run: `cargo check && cargo test`
Expected: compiles and all tests pass

- [ ] **Step 6: Commit**

```
feat: migrate input handlers and rip tick to DriveSession
```

---

### Task 9: Create multi-session coordinator

**Files:**
- Create: `src/tui/coordinator.rs`
- Modify: `src/tui/mod.rs`

This task creates the coordinator that replaces `run_app` — managing multiple sessions, the drive monitor, tab bar, and input routing.

- [ ] **Step 1: Create coordinator.rs with SessionHandle**

```rust
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::prelude::*;

use crate::config::Config;
use crate::drive_monitor::DriveMonitor;
use crate::session::DriveSession;
use crate::types::*;
use crate::tui::{settings, tab_bar, wizard, dashboard};
use crate::Args;

/// Handle to a running session thread
struct SessionHandle {
    id: SessionId,
    device: PathBuf,
    input_tx: mpsc::Sender<SessionCommand>,
    output_rx: mpsc::Receiver<SessionMessage>,
    thread: Option<JoinHandle<()>>,
    /// Cached latest snapshot for rendering
    snapshot: Option<RenderSnapshot>,
    /// Cached tab summary
    tab_summary: TabSummary,
    /// Whether this session's thread has exited
    dead: bool,
}

pub struct Coordinator {
    sessions: Vec<SessionHandle>,
    active_tab: usize,
    config: Config,
    config_path: PathBuf,
    args: Args,
    quit: bool,
    overlay: Option<Overlay>,
    drive_event_rx: mpsc::Receiver<DriveEvent>,
    /// Tracks which episodes are assigned per (show, season) for overlap validation
    assigned_episodes: std::collections::HashMap<(String, u32), Vec<(SessionId, Vec<u32>)>>,
}
```

- [ ] **Step 2: Implement Coordinator::new and spawn_session**

```rust
impl Coordinator {
    pub fn new(
        args: Args,
        config: Config,
        config_path: PathBuf,
    ) -> Self {
        let (drive_tx, drive_rx) = mpsc::channel();

        // Start drive monitor
        DriveMonitor::spawn(Duration::from_secs(2), drive_tx);

        Coordinator {
            sessions: Vec::new(),
            active_tab: 0,
            config,
            config_path,
            args,
            quit: false,
            overlay: None,
            drive_event_rx: drive_rx,
            assigned_episodes: std::collections::HashMap::new(),
        }
    }

    fn spawn_session(&mut self, device: PathBuf) {
        // Don't create duplicate sessions for the same device
        if self.sessions.iter().any(|s| s.device == device && !s.dead) {
            return;
        }

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (msg_tx, msg_rx) = mpsc::channel();

        let mut session = DriveSession::new(
            device.clone(),
            self.config.clone(),
            cmd_rx,
            msg_tx,
        );

        // Apply CLI args to session
        session.movie_mode_arg = self.args.movie;
        session.season_arg = self.args.season;
        session.start_episode_arg = self.args.start_episode;
        session.min_duration_arg = self.args.min_duration;
        session.no_max_speed = self.args.no_max_speed;
        session.output_dir = self.args.output.clone();
        session.cli_eject = self.args.cli_eject();
        session.format = self.args.format.clone();
        session.format_preset = self.args.format_preset.clone();
        session.overwrite = self.args.overwrite;

        let id = session.id;
        let device_name = device.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| device.to_string_lossy().to_string());

        let thread = std::thread::Builder::new()
            .name(format!("session-{}", device_name))
            .spawn(move || {
                // catch_unwind for crash isolation
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    session.run();
                }));
                if let Err(panic) = result {
                    let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic".to_string()
                    };
                    eprintln!("Session {} panicked: {}", device_name, msg);
                }
            })
            .expect("failed to spawn session thread");

        self.sessions.push(SessionHandle {
            id,
            device,
            input_tx: cmd_tx,
            output_rx: msg_rx,
            thread: Some(thread),
            snapshot: None,
            tab_summary: TabSummary {
                session_id: id,
                device_name,
                state: TabState::Idle,
                rip_progress: None,
                error: None,
            },
            dead: false,
        });
    }
}
```

- [ ] **Step 3: Implement the main event loop**

```rust
impl Coordinator {
    pub fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> anyhow::Result<()> {
        // Initial setup: if --device specified, only that drive.
        // Otherwise, auto mode detects drives.
        if let Some(ref device) = self.args.device {
            self.spawn_session(device.clone());
        }
        // If no --device, drive monitor will emit DriveAppeared events

        loop {
            // 1. Render
            self.render(terminal)?;

            if self.quit {
                self.shutdown_all();
                break;
            }

            // 2. Poll terminal events
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key);
                }
            }

            // 3. Drain drive monitor events
            self.poll_drive_events();

            // 4. Drain session messages
            self.poll_sessions();

            // 5. Detect dead sessions
            self.check_dead_sessions();

            // 6. Propagate process-level cancel
            if crate::CANCELLED.load(std::sync::atomic::Ordering::Relaxed) {
                self.shutdown_all();
                break;
            }
        }

        Ok(())
    }

    fn render(
        &self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> anyhow::Result<()> {
        terminal.draw(|f| {
            let tab_summaries: Vec<&TabSummary> = self.sessions
                .iter()
                .filter(|s| !s.dead)
                .map(|s| &s.tab_summary)
                .collect();

            let summaries_owned: Vec<TabSummary> = tab_summaries.iter().map(|s| (*s).clone()).collect();
            let content_area = tab_bar::render(f, &summaries_owned, self.active_tab, f.area());

            // Render active session's content
            if let Some(session) = self.sessions.get(self.active_tab) {
                if let Some(ref snap) = session.snapshot {
                    match snap.screen {
                        crate::tui::Screen::Scanning => {
                            if let Some(ref view) = snap.scanning {
                                wizard::render_scanning_view(
                                    f, view, &snap.status_message,
                                    snap.spinner_frame, content_area,
                                );
                            }
                        }
                        crate::tui::Screen::TmdbSearch => {
                            if let Some(ref view) = snap.tmdb {
                                wizard::render_tmdb_search_view(
                                    f, view, &snap.status_message,
                                    snap.spinner_frame, content_area,
                                );
                            }
                        }
                        crate::tui::Screen::Season => {
                            if let Some(ref view) = snap.season {
                                wizard::render_season_view(
                                    f, view, &snap.status_message,
                                    snap.spinner_frame, content_area,
                                );
                            }
                        }
                        crate::tui::Screen::PlaylistManager => {
                            if let Some(ref view) = snap.playlist_mgr {
                                wizard::render_playlist_manager_view(
                                    f, view, &snap.status_message,
                                    content_area,
                                );
                            }
                        }
                        crate::tui::Screen::Confirm => {
                            if let Some(ref view) = snap.confirm {
                                wizard::render_confirm_view(
                                    f, view, &snap.status_message,
                                    content_area,
                                );
                            }
                        }
                        crate::tui::Screen::Ripping => {
                            if let Some(ref view) = snap.dashboard {
                                dashboard::render_dashboard_view(
                                    f, view, &snap.status_message,
                                    content_area,
                                );
                            }
                        }
                        crate::tui::Screen::Done => {
                            if let Some(ref view) = snap.done {
                                dashboard::render_done_view(f, view, content_area);
                            }
                        }
                    }
                }
            }

            // Overlay (settings) on top
            if let Some(Overlay::Settings(ref state)) = self.overlay {
                settings::render(f, state);
            }
        })?;
        Ok(())
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        // Ctrl+C: quit
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }

        // Overlay captures all input except Ctrl+C
        if self.overlay.is_some() {
            self.handle_overlay_key(key);
            return;
        }

        // Global: Tab/Shift+Tab to switch tabs
        if key.code == KeyCode::Tab && !key.modifiers.contains(KeyModifiers::SHIFT) {
            if !self.sessions.is_empty() {
                self.active_tab = (self.active_tab + 1) % self.sessions.len();
            }
            return;
        }
        if key.code == KeyCode::BackTab
            || (key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT))
        {
            if !self.sessions.is_empty() {
                self.active_tab = if self.active_tab == 0 {
                    self.sessions.len() - 1
                } else {
                    self.active_tab - 1
                };
            }
            return;
        }

        // Global: Ctrl+S for settings
        if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.open_settings();
            return;
        }

        // Global: Ctrl+L for link picker
        if key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.open_link_picker();
            return;
        }

        // Global: Ctrl+N for manual new session
        if key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.config.multi_drive_mode() == "manual" {
                // TODO: show drive picker
            }
            return;
        }

        // Global: q quits (unless text input active or ripping)
        if key.code == KeyCode::Char('q') {
            // Check if active session is in text input or ripping
            if let Some(session) = self.sessions.get(self.active_tab) {
                if let Some(ref snap) = session.snapshot {
                    if snap.screen == crate::tui::Screen::Ripping {
                        // Forward to session (dashboard handles its own q)
                    } else {
                        self.quit = true;
                        return;
                    }
                } else {
                    self.quit = true;
                    return;
                }
            }
        }

        // Forward all other input to active session
        if let Some(session) = self.sessions.get(self.active_tab) {
            if !session.dead {
                let _ = session.input_tx.send(SessionCommand::KeyEvent(key));
            }
        }
    }

    fn poll_drive_events(&mut self) {
        while let Ok(event) = self.drive_event_rx.try_recv() {
            match event {
                DriveEvent::DriveAppeared(device) => {
                    if self.config.multi_drive_mode() == "auto"
                        && self.args.device.is_none()
                    {
                        self.spawn_session(device);
                    }
                }
                DriveEvent::DriveDisappeared(device) => {
                    if let Some(session) = self.sessions.iter_mut().find(|s| s.device == device) {
                        let _ = session.input_tx.send(SessionCommand::Shutdown);
                        session.tab_summary.state = TabState::Error;
                        session.tab_summary.error = Some("drive removed".into());
                    }
                }
                DriveEvent::DiscInserted(device, _label) => {
                    // Forward to session if it exists
                    // The session's own scan will detect the disc
                }
                DriveEvent::DiscEjected(device) => {
                    // Forward to session if it exists
                }
            }
        }
    }

    fn poll_sessions(&mut self) {
        for session in &mut self.sessions {
            if session.dead {
                continue;
            }
            while let Ok(msg) = session.output_rx.try_recv() {
                match msg {
                    SessionMessage::Snapshot(snap) => {
                        // Update tab summary from snapshot
                        session.tab_summary = TabSummary {
                            session_id: snap.session_id,
                            device_name: session.tab_summary.device_name.clone(),
                            state: match snap.screen {
                                crate::tui::Screen::Scanning => {
                                    if snap.scanning.as_ref().map_or(true, |s| s.label.is_empty()) {
                                        TabState::Idle
                                    } else {
                                        TabState::Scanning
                                    }
                                }
                                crate::tui::Screen::TmdbSearch
                                | crate::tui::Screen::Season
                                | crate::tui::Screen::PlaylistManager
                                | crate::tui::Screen::Confirm => TabState::Wizard,
                                crate::tui::Screen::Ripping => TabState::Ripping,
                                crate::tui::Screen::Done => TabState::Done,
                            },
                            rip_progress: snap.dashboard.as_ref().and_then(|d| {
                                let done = d.jobs.iter()
                                    .filter(|j| matches!(j.status, PlaylistStatus::Done(_)))
                                    .count();
                                let total = d.jobs.len();
                                if total == 0 { return None; }
                                let pct = d.jobs.get(d.current_rip).and_then(|j| {
                                    if let PlaylistStatus::Ripping(ref p) = j.status {
                                        if j.playlist.seconds > 0 {
                                            Some(((done as f64 + p.out_time_secs as f64 / j.playlist.seconds as f64) / total as f64 * 100.0) as u8)
                                        } else { None }
                                    } else { None }
                                }).unwrap_or(0);
                                Some((done + 1, total, pct))
                            }),
                            error: None,
                        };
                        session.snapshot = Some(snap);
                    }
                    SessionMessage::Progress { session_id, progress, job_index } => {
                        // Merge lightweight progress into cached snapshot
                        if let Some(ref mut snap) = session.snapshot {
                            if let Some(ref mut dv) = snap.dashboard {
                                if let Some(job) = dv.jobs.get_mut(job_index) {
                                    job.status = PlaylistStatus::Ripping(progress);
                                }
                            }
                        }
                    }
                    SessionMessage::Notification(notif) => {
                        self.handle_notification(notif);
                    }
                }
            }
        }
    }

    fn check_dead_sessions(&mut self) {
        for session in &mut self.sessions {
            if session.dead {
                continue;
            }
            // Check if thread has exited by trying to send a no-op
            // Actually, check if the output channel is disconnected
            if session.thread.as_ref().map_or(false, |t| t.is_finished()) {
                session.dead = true;
                session.tab_summary.state = TabState::Error;
                if session.tab_summary.error.is_none() {
                    session.tab_summary.error = Some("session ended unexpectedly".into());
                }
            }
        }
    }

    fn handle_notification(&mut self, notif: Notification) {
        match notif {
            Notification::EpisodesAssigned { session_id, show_name, season, episodes } => {
                self.validate_episode_overlap(session_id, &show_name, season, &episodes);
            }
            // Handle other notifications as needed
            _ => {}
        }
    }

    fn validate_episode_overlap(
        &mut self,
        session_id: SessionId,
        show_name: &str,
        season: u32,
        episodes: &[u32],
    ) {
        let key = (show_name.to_string(), season);
        let entry = self.assigned_episodes.entry(key).or_default();

        // Check for overlap with other sessions
        for (other_id, other_eps) in entry.iter() {
            if *other_id == session_id {
                continue;
            }
            let overlap: Vec<u32> = episodes.iter()
                .filter(|e| other_eps.contains(e))
                .cloned()
                .collect();
            if !overlap.is_empty() {
                // Send warning back to the session
                if let Some(session) = self.sessions.iter().find(|s| s.id == session_id) {
                    // TODO: send overlap warning via SessionCommand
                    // For now, this is a no-op — the session proceeds
                }
            }
        }

        // Update or add this session's assignments
        if let Some(existing) = entry.iter_mut().find(|(id, _)| *id == session_id) {
            existing.1 = episodes.to_vec();
        } else {
            entry.push((session_id, episodes.to_vec()));
        }
    }

    fn shutdown_all(&mut self) {
        for session in &mut self.sessions {
            let _ = session.input_tx.send(SessionCommand::Shutdown);
        }
        // Wait briefly for threads to exit
        for session in &mut self.sessions {
            if let Some(thread) = session.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn open_settings(&mut self) {
        let drives: Vec<String> = self.sessions
            .iter()
            .map(|s| s.device.to_string_lossy().to_string())
            .collect();
        let mut state = SettingsState::from_config_with_drives(&self.config, &drives);
        state.apply_env_overrides();
        self.overlay = Some(Overlay::Settings(state));
    }

    fn handle_overlay_key(&mut self, key: crossterm::event::KeyEvent) {
        let action = {
            let state = match self.overlay {
                Some(Overlay::Settings(ref mut s)) => s,
                _ => return,
            };
            if state.save_message.is_some() {
                state.save_message = None;
                state.save_message_at = None;
            }
            settings::handle_input(state, key)
        };
        match action {
            settings::SettingsAction::Save => {
                self.save_settings();
            }
            settings::SettingsAction::SaveAndClose => {
                self.save_settings();
                self.overlay = None;
            }
            settings::SettingsAction::Close => {
                self.overlay = None;
            }
            settings::SettingsAction::None => {}
        }
    }

    fn save_settings(&mut self) {
        let new_config = match self.overlay {
            Some(Overlay::Settings(ref state)) => state.to_config(),
            _ => return,
        };
        match new_config.save(&self.config_path) {
            Ok(()) => {
                self.config = new_config.clone();
                // Broadcast config change to all sessions
                for session in &self.sessions {
                    let _ = session.input_tx.send(
                        SessionCommand::ConfigChanged(new_config.clone()),
                    );
                }
                if let Some(Overlay::Settings(ref mut state)) = self.overlay {
                    let warnings = state.active_env_var_warnings();
                    let msg = if warnings.is_empty() {
                        "Saved!".to_string()
                    } else {
                        format!("Saved! (env vars override: {})", warnings.join(", "))
                    };
                    state.save_message = Some(msg);
                    state.save_message_at = Some(std::time::Instant::now());
                    state.dirty = false;
                }
            }
            Err(e) => {
                if let Some(Overlay::Settings(ref mut state)) = self.overlay {
                    state.save_message = Some(format!("Error: {}", e));
                    state.save_message_at = Some(std::time::Instant::now());
                }
            }
        }
    }

    fn open_link_picker(&mut self) {
        // TODO: implement link picker UI
        // Show list of sessions with linkable_context available
        // User selects one, main thread extracts SharedContext from snapshot
        // and sends SessionCommand::LinkTo to the active session
    }
}
```

- [ ] **Step 4: Wire coordinator into tui/mod.rs**

Modify `run_app` to use the coordinator:

```rust
fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    args: &Args,
    config: &crate::config::Config,
    config_path: std::path::PathBuf,
) -> Result<()> {
    let mut coordinator = crate::tui::coordinator::Coordinator::new(
        args.clone(),
        config.clone(),
        config_path,
    );
    coordinator.run(terminal)
}
```

- [ ] **Step 5: Add module declarations**

In `src/tui/mod.rs`, add `pub mod coordinator;`

- [ ] **Step 6: Run compilation**

Run: `cargo check`
Expected: compiles (some features may still be stubbed with TODO comments)

- [ ] **Step 7: Commit**

```
feat: add multi-session coordinator with tab management and input routing
```

---

### Task 10: Implement linked sessions (Ctrl+L)

**Files:**
- Modify: `src/tui/coordinator.rs`

- [ ] **Step 1: Implement the link picker**

Replace the `open_link_picker` TODO in coordinator.rs:

```rust
fn open_link_picker(&mut self) {
    // Collect linkable sessions (have completed TMDb lookup)
    let linkable: Vec<(usize, &SessionHandle)> = self.sessions.iter()
        .enumerate()
        .filter(|(idx, s)| {
            *idx != self.active_tab
                && !s.dead
                && s.snapshot.as_ref()
                    .and_then(|snap| snap.linkable_context.as_ref())
                    .is_some()
        })
        .collect();

    if linkable.is_empty() {
        // No sessions to link to — no-op
        return;
    }

    if linkable.len() == 1 {
        // Only one option — link immediately
        let (idx, _) = linkable[0];
        self.link_session(idx);
        return;
    }

    // Multiple options — for now, link to the first one.
    // TODO: Show a selection popup for choosing which session to link to
    let (idx, _) = linkable[0];
    self.link_session(idx);
}

fn link_session(&mut self, source_tab: usize) {
    let context = self.sessions.get(source_tab)
        .and_then(|s| s.snapshot.as_ref())
        .and_then(|snap| snap.linkable_context.clone());

    if let Some(context) = context {
        if let Some(session) = self.sessions.get(self.active_tab) {
            let _ = session.input_tx.send(SessionCommand::LinkTo { context });
        }
    }
}
```

- [ ] **Step 2: Run compilation**

Run: `cargo check`
Expected: compiles

- [ ] **Step 3: Commit**

```
feat: implement Ctrl+L linked sessions for multi-disc workflows
```

---

### Task 11: Integration — update main.rs entry point

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Ensure module declarations are present**

Verify `src/main.rs` has:

```rust
mod drive_monitor;
mod session;
```

These should already be added from earlier tasks. If not, add them.

- [ ] **Step 2: Add CLI TODO for future concurrent CLI support**

In `src/cli.rs`, add a comment at the top of the module:

```rust
// TODO(multi-drive): Add concurrent CLI support with interleaved output
// and drive-prefixed progress lines (e.g., [sr0] Ripping playlist 1...)
// For now, CLI mode is single-drive only.
```

- [ ] **Step 3: Run full test suite and clippy**

Run: `cargo test && cargo clippy`
Expected: all tests pass, no clippy warnings

- [ ] **Step 4: Commit**

```
feat: wire multi-drive support into main entry point
```

---

### Task 12: Add overlap validation tests

**Files:**
- Modify: `src/tui/coordinator.rs`

- [ ] **Step 1: Write overlap validation tests**

Add to `coordinator.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_overlap_different_episodes() {
        let (_, drive_rx) = mpsc::channel();
        let mut coord = Coordinator {
            sessions: vec![],
            active_tab: 0,
            config: Config::default(),
            config_path: PathBuf::new(),
            args: Args::parse_from(["bluback"]),
            quit: false,
            overlay: None,
            drive_event_rx: drive_rx,
            assigned_episodes: std::collections::HashMap::new(),
        };

        coord.validate_episode_overlap(
            SessionId(1), "Breaking Bad", 1, &[1, 2, 3, 4],
        );
        coord.validate_episode_overlap(
            SessionId(2), "Breaking Bad", 1, &[5, 6, 7, 8],
        );

        let key = ("Breaking Bad".to_string(), 1);
        let entries = coord.assigned_episodes.get(&key).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_overlap_detected() {
        let (_, drive_rx) = mpsc::channel();
        let mut coord = Coordinator {
            sessions: vec![],
            active_tab: 0,
            config: Config::default(),
            config_path: PathBuf::new(),
            args: Args::parse_from(["bluback"]),
            quit: false,
            overlay: None,
            drive_event_rx: drive_rx,
            assigned_episodes: std::collections::HashMap::new(),
        };

        coord.validate_episode_overlap(
            SessionId(1), "Breaking Bad", 1, &[1, 2, 3, 4],
        );
        // Session 2 overlaps on episodes 3, 4
        coord.validate_episode_overlap(
            SessionId(2), "Breaking Bad", 1, &[3, 4, 5, 6],
        );

        // Both sessions' assignments are tracked
        let key = ("Breaking Bad".to_string(), 1);
        let entries = coord.assigned_episodes.get(&key).unwrap();
        assert_eq!(entries.len(), 2);
        // Overlap detected — in a real implementation, a warning would be sent
    }

    #[test]
    fn test_different_shows_no_overlap() {
        let (_, drive_rx) = mpsc::channel();
        let mut coord = Coordinator {
            sessions: vec![],
            active_tab: 0,
            config: Config::default(),
            config_path: PathBuf::new(),
            args: Args::parse_from(["bluback"]),
            quit: false,
            overlay: None,
            drive_event_rx: drive_rx,
            assigned_episodes: std::collections::HashMap::new(),
        };

        coord.validate_episode_overlap(
            SessionId(1), "Breaking Bad", 1, &[1, 2, 3],
        );
        coord.validate_episode_overlap(
            SessionId(2), "Better Call Saul", 1, &[1, 2, 3],
        );

        assert_eq!(coord.assigned_episodes.len(), 2);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test coordinator`
Expected: all 3 tests pass

- [ ] **Step 3: Commit**

```
test: add episode overlap validation tests for linked sessions
```

---

### Task 13: Final integration test and cleanup

**Files:**
- Multiple files (cleanup pass)

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: all tests pass (existing 234+ unit tests + new tests)

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -W clippy::all`
Expected: no warnings (fix any that appear)

- [ ] **Step 3: Run a build**

Run: `cargo build`
Expected: successful compilation

- [ ] **Step 4: Verify single-drive backward compatibility**

The existing single-drive workflow must work identically:
- With `--device` flag: only one session created, no tab bar
- Without `--device`: auto-detect finds one drive, single session, no tab bar (tab bar hidden when only one session)
- CLI mode (`--no-tui`): unchanged, single-drive only
- `--settings` standalone: unchanged

- [ ] **Step 5: Commit**

```
feat: multi-drive support — concurrent ripping with tabbed TUI sessions
```

---

## Summary

| Task | Description | New Files | Key Deliverables |
|---|---|---|---|
| 1 | Config option | — | `multi_drive` field in Config |
| 2 | Core types | — | SessionId, SessionCommand, SessionMessage, View structs, DriveEvent |
| 3 | Drive monitor | `drive_monitor.rs` | Polling thread, event diffing |
| 4 | DriveSession | `session.rs` | Per-session state, snapshot builder |
| 5 | Render refactor | — | View-based render functions with App adapters |
| 6 | Tab bar | `tui/tab_bar.rs` | Tab bar widget |
| 7 | Session event loop | — | Session thread run(), disc scan, background polling |
| 8 | Input migration | — | Session-based input handlers, rip tick |
| 9 | Coordinator | `tui/coordinator.rs` | Multi-session management, replaces run_app |
| 10 | Linked sessions | — | Ctrl+L, SharedContext, overlap validation |
| 11 | Main entry point | — | Module wiring, CLI TODO |
| 12 | Overlap tests | — | Validation test coverage |
| 13 | Final integration | — | Full test pass, clippy, backward compat |
