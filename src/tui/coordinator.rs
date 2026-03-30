use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::prelude::*;

use crate::config::Config;
use crate::drive_monitor::DriveMonitor;
use crate::session::DriveSession;
use crate::tui::{dashboard, settings, tab_bar, wizard, InputFocus, Screen};
use crate::types::*;
use crate::Args;

/// Maps (show_name, season) to a list of (session_id, episode_numbers) assignments
type EpisodeAssignmentMap = HashMap<(String, u32), Vec<(SessionId, Vec<u32>)>>;

struct SessionHandle {
    id: SessionId,
    device: PathBuf,
    input_tx: mpsc::Sender<SessionCommand>,
    output_rx: mpsc::Receiver<SessionMessage>,
    thread: Option<JoinHandle<()>>,
    snapshot: Option<RenderSnapshot>,
    tab_summary: TabSummary,
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
    /// Track assigned episodes per (show_name, season) across sessions for overlap detection.
    assigned_episodes: EpisodeAssignmentMap,
}

impl Coordinator {
    pub fn new(args: Args, config: Config, config_path: PathBuf) -> Self {
        let (drive_tx, drive_rx) = mpsc::channel();
        DriveMonitor::spawn(Duration::from_secs(2), drive_tx);

        Self {
            sessions: Vec::new(),
            active_tab: 0,
            config,
            config_path,
            args,
            quit: false,
            overlay: None,
            drive_event_rx: drive_rx,
            assigned_episodes: HashMap::new(),
        }
    }

    fn spawn_session(&mut self, device: PathBuf) {
        // Skip if a live session already exists for this device
        if self.sessions.iter().any(|s| s.device == device && !s.dead) {
            return;
        }

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (msg_tx, msg_rx) = mpsc::channel();

        let mut session = DriveSession::new(device.clone(), self.config.clone(), cmd_rx, msg_tx);

        // Copy CLI args to session
        session.movie_mode_arg = self.args.movie;
        session.season_arg = self.args.season;
        session.start_episode_arg = self.args.start_episode;
        session.min_duration_arg = Some(self.args.min_duration);
        session.no_max_speed = self.args.no_max_speed;
        session.output_dir = self.args.output.clone();
        session.cli_eject = self.args.cli_eject();
        session.format = self.args.format.clone();
        session.format_preset = self.args.format_preset.clone();
        session.overwrite = self.args.overwrite;
        session.no_metadata = self.args.no_metadata;
        session.no_hooks = self.args.no_hooks;
        session.verify = self.args.verify || (!self.args.no_verify && self.config.verify());
        session.verify_level = match self
            .args
            .verify_level
            .as_deref()
            .unwrap_or(self.config.verify_level())
        {
            "full" => crate::verify::VerifyLevel::Full,
            _ => crate::verify::VerifyLevel::Quick,
        };

        let session_id = session.id;
        let device_name = device
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| device.to_string_lossy().to_string());

        let thread = std::thread::Builder::new()
            .name(format!("session-{}", device.display()))
            .spawn(move || {
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
                    log::error!("Session thread panicked: {}", msg);
                }
            })
            .expect("failed to spawn session thread");

        let handle = SessionHandle {
            id: session_id,
            device: device.clone(),
            input_tx: cmd_tx,
            output_rx: msg_rx,
            thread: Some(thread),
            snapshot: None,
            tab_summary: TabSummary {
                session_id,
                device_name,
                state: TabState::Idle,
                rip_progress: None,
                error: None,
            },
            dead: false,
        };

        self.sessions.push(handle);
    }

    pub fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        // If --device specified, spawn session for that device only
        if let Some(ref device) = self.args.device {
            self.spawn_session(device.clone());
        }

        loop {
            self.render(terminal)?;

            if self.quit {
                self.shutdown_all();
                break;
            }

            // Poll terminal events
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key);
                }
            }

            // Poll drive monitor events
            self.poll_drive_events();

            // Poll session messages
            self.poll_sessions();

            // Check for dead sessions
            self.check_dead_sessions();

            // Propagate process-level cancel
            if crate::CANCELLED.load(std::sync::atomic::Ordering::Relaxed) {
                self.shutdown_all();
                break;
            }
        }

        Ok(())
    }

    fn render(&self, terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
        terminal.draw(|f| {
            let area = f.area();

            // Collect tab summaries from non-dead sessions
            let tab_summaries: Vec<TabSummary> = self
                .sessions
                .iter()
                .filter(|s| !s.dead)
                .map(|s| s.tab_summary.clone())
                .collect();

            // Render tab bar (returns content area below it)
            let content_area = tab_bar::render(f, &tab_summaries, self.active_tab, area);

            // Render active session's content
            if let Some(session) = self.active_session() {
                if let Some(ref snap) = session.snapshot {
                    let status = &snap.status_message;
                    let spinner = snap.spinner_frame;

                    match snap.screen {
                        Screen::Scanning => {
                            if let Some(ref view) = snap.scanning {
                                wizard::render_scanning_view(
                                    f,
                                    view,
                                    status,
                                    spinner,
                                    content_area,
                                );
                            }
                        }
                        Screen::TmdbSearch => {
                            if let Some(ref view) = snap.tmdb {
                                wizard::render_tmdb_search_view(
                                    f,
                                    view,
                                    status,
                                    spinner,
                                    content_area,
                                );
                            }
                        }
                        Screen::Season => {
                            if let Some(ref view) = snap.season {
                                wizard::render_season_view(f, view, status, spinner, content_area);
                            }
                        }
                        Screen::PlaylistManager => {
                            if let Some(ref view) = snap.playlist_mgr {
                                wizard::render_playlist_manager_view(f, view, status, content_area);
                            }
                        }
                        Screen::Confirm => {
                            if let Some(ref view) = snap.confirm {
                                wizard::render_confirm_view(f, view, status, content_area);
                            }
                        }
                        Screen::Ripping => {
                            if let Some(ref view) = snap.dashboard {
                                dashboard::render_dashboard_view(f, view, status, content_area);
                            }
                        }
                        Screen::Done => {
                            if let Some(ref view) = snap.done {
                                dashboard::render_done_view(f, view, content_area);
                            }
                        }
                    }
                }
            }

            // Render settings overlay on top if present
            if let Some(Overlay::Settings(ref state)) = self.overlay {
                settings::render(f, state);
            }
        })?;

        Ok(())
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        // Ctrl+C: always quit immediately
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }

        // Route ALL input to overlay when active
        if self.overlay.is_some() {
            self.handle_overlay_key(key);
            return;
        }

        // Tab/Shift+Tab: switch active tab
        if key.code == KeyCode::Tab && !key.modifiers.contains(KeyModifiers::SHIFT) {
            if !self.sessions.is_empty() {
                let live_count = self.live_session_count();
                if live_count > 0 {
                    self.active_tab = (self.active_tab + 1) % live_count;
                }
            }
            return;
        }
        if key.code == KeyCode::BackTab
            || (key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT))
        {
            if !self.sessions.is_empty() {
                let live_count = self.live_session_count();
                if live_count > 0 {
                    self.active_tab = if self.active_tab == 0 {
                        live_count - 1
                    } else {
                        self.active_tab - 1
                    };
                }
            }
            return;
        }

        // Ctrl+S: open settings
        if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.open_settings();
            return;
        }

        // Ctrl+L: open link picker
        if key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.link_session();
            return;
        }

        // Ctrl+N: new manual session (if no --device, spawn on first available drive
        // without an existing session)
        if key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::CONTROL) {
            // Find drives that don't have active sessions
            let active_devices: Vec<PathBuf> = self
                .sessions
                .iter()
                .filter(|s| !s.dead)
                .map(|s| s.device.clone())
                .collect();
            let all_drives = crate::disc::detect_optical_drives();
            for drive in all_drives {
                if !active_devices.contains(&drive) {
                    self.spawn_session(drive);
                    self.active_tab = self.live_session_count().saturating_sub(1);
                    break;
                }
            }
            return;
        }

        // q: quit (unless active session is in a state that uses 'q')
        if key.code == KeyCode::Char('q') {
            if let Some(session) = self.active_session() {
                if let Some(ref snap) = session.snapshot {
                    // Check if text input is active or session is ripping
                    let input_active = self.is_text_input_active(snap);
                    if snap.screen == Screen::Ripping || input_active {
                        // Forward to session instead of quitting
                        self.forward_key_to_active(key);
                        return;
                    }
                }
            }
            self.quit = true;
            return;
        }

        // Everything else: forward to active session
        self.forward_key_to_active(key);
    }

    fn is_text_input_active(&self, snap: &RenderSnapshot) -> bool {
        match snap.screen {
            Screen::TmdbSearch => {
                if let Some(ref view) = snap.tmdb {
                    matches!(
                        view.input_focus,
                        InputFocus::TextInput
                            | InputFocus::InlineEdit(_)
                            | InputFocus::TrackEdit(_)
                    )
                } else {
                    false
                }
            }
            Screen::Season => {
                if let Some(ref view) = snap.season {
                    matches!(view.input_focus, InputFocus::TextInput)
                } else {
                    false
                }
            }
            Screen::PlaylistManager => {
                if let Some(ref view) = snap.playlist_mgr {
                    matches!(
                        view.input_focus,
                        InputFocus::InlineEdit(_) | InputFocus::TrackEdit(_)
                    )
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn forward_key_to_active(&self, key: crossterm::event::KeyEvent) {
        if let Some(session) = self.active_session() {
            let _ = session.input_tx.send(SessionCommand::KeyEvent(key));
        }
    }

    fn active_session(&self) -> Option<&SessionHandle> {
        self.sessions
            .iter()
            .filter(|s| !s.dead)
            .nth(self.active_tab)
    }

    #[allow(dead_code)] // Will be used for session-specific operations (e.g., direct state mutation)
    fn active_session_mut(&mut self) -> Option<&mut SessionHandle> {
        let tab = self.active_tab;
        self.sessions.iter_mut().filter(|s| !s.dead).nth(tab)
    }

    fn live_session_count(&self) -> usize {
        self.sessions.iter().filter(|s| !s.dead).count()
    }

    fn poll_drive_events(&mut self) {
        while let Ok(event) = self.drive_event_rx.try_recv() {
            match event {
                DriveEvent::DriveAppeared(device) | DriveEvent::DiscInserted(device, _) => {
                    // Auto-spawn session if no --device flag was given
                    if self.args.device.is_none() {
                        self.spawn_session(device);
                    }
                }
                DriveEvent::DriveDisappeared(ref device) => {
                    // Find the session for this device and shut it down
                    for session in &mut self.sessions {
                        if &session.device == device && !session.dead {
                            let _ = session.input_tx.send(SessionCommand::Shutdown);
                            session.tab_summary.state = TabState::Error;
                            session.tab_summary.error = Some("drive disconnected".to_string());
                        }
                    }
                }
                DriveEvent::DiscEjected(ref device) => {
                    // Notify the session but don't kill it — the user might
                    // insert a new disc
                    for session in &mut self.sessions {
                        if &session.device == device && !session.dead {
                            session.tab_summary.state = TabState::Idle;
                        }
                    }
                }
            }
        }
    }

    fn poll_sessions(&mut self) {
        let mut notifications = Vec::new();

        for session in &mut self.sessions {
            if session.dead {
                continue;
            }

            // Drain all available messages
            while let Ok(msg) = session.output_rx.try_recv() {
                match msg {
                    SessionMessage::Snapshot(boxed_snap) => {
                        let snap = *boxed_snap;
                        session.tab_summary = TabSummary {
                            session_id: snap.session_id,
                            device_name: session.tab_summary.device_name.clone(),
                            state: match snap.screen {
                                Screen::Scanning => {
                                    if snap
                                        .scanning
                                        .as_ref()
                                        .map(|s| s.label.is_empty())
                                        .unwrap_or(true)
                                    {
                                        TabState::Idle
                                    } else {
                                        TabState::Scanning
                                    }
                                }
                                Screen::TmdbSearch
                                | Screen::Season
                                | Screen::PlaylistManager
                                | Screen::Confirm => TabState::Wizard,
                                Screen::Ripping => TabState::Ripping,
                                Screen::Done => TabState::Done,
                            },
                            rip_progress: if snap.screen == Screen::Ripping {
                                snap.dashboard.as_ref().map(|d| {
                                    let total = d.jobs.len();
                                    let done_count = d
                                        .jobs
                                        .iter()
                                        .filter(|j| {
                                            matches!(
                                                j.status,
                                                PlaylistStatus::Done(_)
                                                    | PlaylistStatus::Verified(..)
                                                    | PlaylistStatus::VerifyFailed(..)
                                            )
                                        })
                                        .count();
                                    let current_pct = d
                                        .jobs
                                        .get(d.current_rip)
                                        .and_then(|job| {
                                            if let PlaylistStatus::Ripping(ref prog) = job.status {
                                                if job.playlist.seconds > 0 {
                                                    Some(
                                                        (prog.out_time_secs as f64
                                                            / job.playlist.seconds as f64
                                                            * 100.0)
                                                            .min(100.0)
                                                            as u8,
                                                    )
                                                } else {
                                                    Some(0)
                                                }
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or(0);
                                    let overall = if total > 0 {
                                        ((done_count as f64 * 100.0 + current_pct as f64)
                                            / total as f64)
                                            as u8
                                    } else {
                                        0
                                    };
                                    (done_count + 1, total, overall)
                                })
                            } else {
                                None
                            },
                            error: session.tab_summary.error.clone(),
                        };
                        session.snapshot = Some(snap);
                    }
                    SessionMessage::Progress {
                        session_id: _,
                        progress,
                        job_index,
                    } => {
                        // Merge progress into the cached snapshot's dashboard view
                        if let Some(ref mut snap) = session.snapshot {
                            if let Some(ref mut dashboard) = snap.dashboard {
                                if let Some(job) = dashboard.jobs.get_mut(job_index) {
                                    job.status = PlaylistStatus::Ripping(progress);
                                }
                            }
                        }
                    }
                    SessionMessage::Notification(notification) => {
                        notifications.push(notification);
                    }
                }
            }
        }

        // Handle notifications outside the session borrow
        for notification in notifications {
            self.handle_notification(notification);
        }
    }

    fn handle_notification(&mut self, notification: Notification) {
        match notification {
            Notification::EpisodesAssigned {
                session_id,
                show_name,
                season,
                episodes,
            } => {
                self.validate_episode_overlap(session_id, &show_name, season, &episodes);
            }
            Notification::ScreenChanged {
                session_id,
                tab_summary,
            } => {
                if let Some(session) = self.sessions.iter_mut().find(|s| s.id == session_id) {
                    session.tab_summary = tab_summary;
                }
            }
            // Other notifications are informational; we can log or ignore
            Notification::RipComplete { .. }
            | Notification::RipFailed { .. }
            | Notification::AllDone { .. }
            | Notification::DiscDetected { .. }
            | Notification::SessionCrashed { .. } => {}
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

        // Remove any previous assignments from this session for this show/season
        if let Some(entries) = self.assigned_episodes.get_mut(&key) {
            entries.retain(|(sid, _)| *sid != session_id);
        }

        // Add new assignments
        self.assigned_episodes
            .entry(key.clone())
            .or_default()
            .push((session_id, episodes.to_vec()));

        // Check for overlapping episodes across different sessions
        if let Some(entries) = self.assigned_episodes.get(&key) {
            let mut all_eps: Vec<(SessionId, u32)> = Vec::new();
            for (sid, eps) in entries {
                for &ep in eps {
                    all_eps.push((*sid, ep));
                }
            }

            // Find duplicates (same episode from different sessions)
            let mut seen: HashMap<u32, SessionId> = HashMap::new();
            for (sid, ep) in &all_eps {
                if let Some(other_sid) = seen.get(ep) {
                    if other_sid != sid {
                        // Overlap detected — log for now
                        log::warn!(
                            "Episode {} of {} S{:02} is assigned in multiple sessions",
                            ep,
                            show_name,
                            season
                        );
                    }
                } else {
                    seen.insert(*ep, *sid);
                }
            }
        }
    }

    fn check_dead_sessions(&mut self) {
        for session in &mut self.sessions {
            if session.dead {
                continue;
            }
            if let Some(ref thread) = session.thread {
                if thread.is_finished() {
                    session.dead = true;
                    session.tab_summary.state = TabState::Error;
                    if session.tab_summary.error.is_none() {
                        session.tab_summary.error = Some("session ended".to_string());
                    }
                }
            }
        }

        // Clamp active_tab if sessions died
        let live = self.live_session_count();
        if live > 0 && self.active_tab >= live {
            self.active_tab = live - 1;
        }
    }

    fn shutdown_all(&mut self) {
        for session in &mut self.sessions {
            if !session.dead {
                let _ = session.input_tx.send(SessionCommand::Shutdown);
            }
        }

        // Join all threads with a brief timeout
        for session in &mut self.sessions {
            if let Some(thread) = session.thread.take() {
                // Give each thread a moment to clean up
                let _ = thread.join();
            }
            session.dead = true;
        }
    }

    // --- Settings overlay ---

    fn open_settings(&mut self) {
        let drives: Vec<String> = crate::disc::detect_optical_drives()
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
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
            settings::SettingsAction::None => {
                // Apply live preview of settings changes (no save to disk)
                let new_config = match self.overlay {
                    Some(Overlay::Settings(ref state)) => state.to_config(),
                    _ => return,
                };
                self.config = new_config;
            }
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

                // Update args from new config
                if let Some(ref dir) = new_config.output_dir {
                    self.args.output = PathBuf::from(dir);
                }
                if let Some(ref dev) = new_config.device {
                    if dev != crate::config::DEFAULT_DEVICE {
                        self.args.device = Some(PathBuf::from(dev));
                    }
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

                // Broadcast config change to all live sessions
                for session in &self.sessions {
                    if !session.dead {
                        let _ = session
                            .input_tx
                            .send(SessionCommand::ConfigChanged(Box::new(new_config.clone())));
                    }
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

    // --- Link picker ---

    fn link_session(&self) {
        // Find sessions with linkable context
        let linkable: Vec<(SessionId, SharedContext)> = self
            .sessions
            .iter()
            .filter(|s| !s.dead)
            .filter_map(|s| {
                s.snapshot
                    .as_ref()
                    .and_then(|snap| snap.linkable_context.clone())
                    .map(|ctx| (s.id, ctx))
            })
            .collect();

        if linkable.is_empty() {
            return;
        }

        // Get active session
        let active_id = match self.active_session() {
            Some(s) => s.id,
            None => return,
        };

        // Find a linkable context from another session (prefer the first non-active one)
        let context = linkable
            .iter()
            .find(|(id, _)| *id != active_id)
            .or_else(|| linkable.first())
            .map(|(_, ctx)| ctx.clone());

        if let Some(ctx) = context {
            if let Some(session) = self.active_session() {
                let _ = session
                    .input_tx
                    .send(SessionCommand::LinkTo { context: ctx });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn make_test_coordinator() -> Coordinator {
        let (_, drive_rx) = mpsc::channel();
        let args = Args::parse_from(["bluback"]);
        let config = Config::default();
        let config_path = PathBuf::from("/tmp/test_config.toml");

        Coordinator {
            sessions: Vec::new(),
            active_tab: 0,
            config,
            config_path,
            args,
            quit: false,
            overlay: None,
            drive_event_rx: drive_rx,
            assigned_episodes: HashMap::new(),
        }
    }

    #[test]
    fn test_no_overlap_different_episodes() {
        let mut coord = make_test_coordinator();
        let session1 = SessionId(1);
        let session2 = SessionId(2);

        coord.validate_episode_overlap(session1, "Breaking Bad", 1, &[1, 2, 3]);
        coord.validate_episode_overlap(session2, "Breaking Bad", 1, &[4, 5, 6]);

        let key = ("Breaking Bad".to_string(), 1);
        let entries = coord.assigned_episodes.get(&key).unwrap();

        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&(session1, vec![1, 2, 3])));
        assert!(entries.contains(&(session2, vec![4, 5, 6])));
    }

    #[test]
    fn test_overlap_detected() {
        let mut coord = make_test_coordinator();
        let session1 = SessionId(1);
        let session2 = SessionId(2);

        coord.validate_episode_overlap(session1, "The Wire", 2, &[1, 2, 3]);
        coord.validate_episode_overlap(session2, "The Wire", 2, &[3, 4, 5]);

        let key = ("The Wire".to_string(), 2);
        let entries = coord.assigned_episodes.get(&key).unwrap();

        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&(session1, vec![1, 2, 3])));
        assert!(entries.contains(&(session2, vec![3, 4, 5])));
    }

    #[test]
    fn test_different_shows_no_overlap() {
        let mut coord = make_test_coordinator();
        let session1 = SessionId(1);
        let session2 = SessionId(2);

        coord.validate_episode_overlap(session1, "Sopranos", 1, &[1, 2, 3]);
        coord.validate_episode_overlap(session2, "The Wire", 1, &[1, 2, 3]);

        let key1 = ("Sopranos".to_string(), 1);
        let key2 = ("The Wire".to_string(), 1);

        let entries1 = coord.assigned_episodes.get(&key1).unwrap();
        let entries2 = coord.assigned_episodes.get(&key2).unwrap();

        assert_eq!(entries1.len(), 1);
        assert_eq!(entries2.len(), 1);
        assert!(entries1.contains(&(session1, vec![1, 2, 3])));
        assert!(entries2.contains(&(session2, vec![1, 2, 3])));
    }
}
