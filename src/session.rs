use std::path::PathBuf;
use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::Config;
use crate::tui::{DiscState, InputFocus, RipState, Screen, TmdbState, WizardState};
use crate::types::*;

static NEXT_SESSION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn next_session_id() -> SessionId {
    SessionId(NEXT_SESSION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

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

    pub input_rx: mpsc::Receiver<SessionCommand>,
    pub output_tx: mpsc::Sender<SessionMessage>,

    // Per-session CLI args
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
    pub no_metadata: bool,
}

impl DriveSession {
    pub fn new(
        device: PathBuf,
        config: Config,
        input_rx: mpsc::Receiver<SessionCommand>,
        output_tx: mpsc::Sender<SessionMessage>,
    ) -> Self {
        let eject = config.should_eject(None);
        let tmdb_api_key = crate::tmdb::get_api_key(&config);

        Self {
            id: next_session_id(),
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
            no_metadata: false,
        }
    }

    /// Build a full render snapshot for the main thread to display.
    pub fn snapshot(&self) -> RenderSnapshot {
        let linkable_context = if !self.tmdb.show_name.is_empty() {
            Some(SharedContext {
                show_name: self.tmdb.show_name.clone(),
                tmdb_show: self
                    .tmdb
                    .selected_show
                    .and_then(|i| self.tmdb.search_results.get(i).cloned()),
                season_num: self.wizard.season_num.unwrap_or(1),
                next_episode: self.next_unassigned_episode(),
                movie_mode: self.tmdb.movie_mode,
                episodes: self.tmdb.episodes.clone(),
            })
        } else if self.tmdb.selected_movie.is_some() {
            Some(SharedContext {
                show_name: self
                    .tmdb
                    .movie_results
                    .get(self.tmdb.selected_movie.unwrap())
                    .map(|m| m.title.clone())
                    .unwrap_or_default(),
                tmdb_show: None,
                season_num: 0,
                next_episode: 1,
                movie_mode: true,
                episodes: vec![],
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

    /// Build a compact tab bar summary for this session.
    #[allow(dead_code)] // Used indirectly by coordinator via snapshot-based tab summary updates
    pub fn tab_summary(&self) -> TabSummary {
        let device_name = self
            .device
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.device.to_string_lossy().to_string());

        let (state, rip_progress) = match &self.screen {
            Screen::Scanning => {
                if self.disc.label.is_empty() {
                    (TabState::Idle, None)
                } else {
                    (TabState::Scanning, None)
                }
            }
            Screen::TmdbSearch | Screen::Season | Screen::PlaylistManager | Screen::Confirm => {
                (TabState::Wizard, None)
            }
            Screen::Ripping => {
                let total = self.rip.jobs.len();
                let done_count = self
                    .rip
                    .jobs
                    .iter()
                    .filter(|j| matches!(j.status, PlaylistStatus::Done(_)))
                    .count();

                let current_pct = self
                    .rip
                    .jobs
                    .get(self.rip.current_rip)
                    .and_then(|job| {
                        if let PlaylistStatus::Ripping(ref prog) = job.status {
                            if job.playlist.seconds > 0 {
                                Some(
                                    (prog.out_time_secs as f64 / job.playlist.seconds as f64
                                        * 100.0)
                                        .min(100.0) as u8,
                                )
                            } else {
                                Some(0)
                            }
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);

                // Overall progress: each done job = 100%, current job = current_pct
                let overall = if total > 0 {
                    ((done_count as f64 * 100.0 + current_pct as f64) / total as f64) as u8
                } else {
                    0
                };

                (TabState::Ripping, Some((done_count + 1, total, overall)))
            }
            Screen::Done => (TabState::Done, None),
        };

        TabSummary {
            session_id: self.id,
            device_name,
            state,
            rip_progress,
            error: None,
        }
    }

    /// Reset all per-disc state for a new scan. Preserves config, CLI args, and channels.
    pub fn reset_for_rescan(&mut self) {
        // Cancel any active rip
        self.rip
            .cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.rip.progress_rx = None;

        if self.disc.did_mount {
            let _ = crate::disc::unmount_disc(&self.device.to_string_lossy());
        }

        self.disc = DiscState::default();

        // Reset tmdb state but keep api_key at session level
        self.tmdb.search_query = String::new();
        self.tmdb.movie_mode = false;
        self.tmdb.search_results = Vec::new();
        self.tmdb.movie_results = Vec::new();
        self.tmdb.selected_show = None;
        self.tmdb.selected_movie = None;
        self.tmdb.show_name = String::new();
        self.tmdb.episodes = Vec::new();

        self.wizard = WizardState::default();
        self.rip = RipState::default();
        self.status_message = String::new();
        self.spinner_frame = 0;
        self.pending_rx = None;
        self.disc_detected_label = None;
    }

    /// Returns the next unassigned episode number (max assigned + 1, or 1 if none).
    pub fn next_unassigned_episode(&self) -> u32 {
        let max = self
            .wizard
            .episode_assignments
            .values()
            .flat_map(|eps| eps.iter().map(|e| e.episode_number))
            .max();
        match max {
            Some(n) => n + 1,
            None => 1,
        }
    }

    fn build_scanning_view(&self) -> Option<ScanningView> {
        if self.screen != Screen::Scanning {
            return None;
        }
        Some(ScanningView {
            label: self.disc.label.clone(),
            scan_log: self.disc.scan_log.clone(),
        })
    }

    fn build_tmdb_view(&self) -> Option<TmdbView> {
        if self.screen != Screen::TmdbSearch {
            return None;
        }
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
            episodes_pl_count: self.disc.episodes_pl.len(),
        })
    }

    fn build_season_view(&self) -> Option<SeasonView> {
        if self.screen != Screen::Season {
            return None;
        }
        Some(SeasonView {
            show_name: self.tmdb.show_name.clone(),
            season_num: self.wizard.season_num,
            input_buffer: self.wizard.input_buffer.clone(),
            input_focus: self.wizard.input_focus.clone(),
            episodes: self.tmdb.episodes.clone(),
            list_cursor: self.wizard.list_cursor,
            label: self.disc.label.clone(),
        })
    }

    fn build_playlist_view(&self) -> Option<PlaylistView> {
        if self.screen != Screen::PlaylistManager {
            return None;
        }
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
            label: self.disc.label.clone(),
            filenames: std::collections::HashMap::new(), // TODO: compute filenames
        })
    }

    fn build_confirm_view(&self) -> Option<ConfirmView> {
        if self.screen != Screen::Confirm {
            return None;
        }
        // Build the list of selected playlists matching filenames
        let selected_playlists: Vec<Playlist> = self
            .disc
            .playlists
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                self.wizard
                    .playlist_selected
                    .get(*i)
                    .copied()
                    .unwrap_or(false)
            })
            .map(|(_, pl)| pl.clone())
            .collect();

        Some(ConfirmView {
            filenames: self.wizard.filenames.clone(),
            playlists: selected_playlists,
            episode_assignments: self.wizard.episode_assignments.clone(),
            list_cursor: self.wizard.list_cursor,
            movie_mode: self.tmdb.movie_mode,
            label: self.disc.label.clone(),
            output_dir: self.output_dir.display().to_string(),
            dry_run: false, // DriveSession doesn't support dry_run yet
            media_infos: self.wizard.media_infos.clone(),
        })
    }

    fn build_dashboard_view(&self) -> Option<DashboardView> {
        if self.screen != Screen::Ripping {
            return None;
        }
        Some(DashboardView {
            jobs: self.rip.jobs.clone(),
            current_rip: self.rip.current_rip,
            confirm_abort: self.rip.confirm_abort,
            confirm_rescan: self.rip.confirm_rescan,
            label: self.disc.label.clone(),
        })
    }

    fn build_done_view(&self) -> Option<DoneView> {
        if self.screen != Screen::Done {
            return None;
        }
        Some(DoneView {
            jobs: self.rip.jobs.clone(),
            label: self.disc.label.clone(),
            disc_detected_label: self.disc_detected_label.clone(),
            eject: self.eject,
            status_message: self.status_message.clone(),
            filenames: self.wizard.filenames.clone(),
        })
    }

    // --- Session thread entry point and event loop ---

    /// Main entry point for the session thread. Runs the scan/wizard/rip lifecycle.
    pub fn run(mut self) {
        self.start_disc_scan();
        self.emit_snapshot();

        loop {
            let command = self
                .input_rx
                .recv_timeout(std::time::Duration::from_millis(100));

            match command {
                Ok(SessionCommand::Shutdown) => {
                    self.rip
                        .cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    if self.disc.did_mount {
                        let _ = crate::disc::unmount_disc(&self.device.to_string_lossy());
                    }
                    return;
                }
                Ok(SessionCommand::KeyEvent(key)) => {
                    self.handle_key(key);
                    self.emit_snapshot();
                }
                Ok(SessionCommand::LinkTo { context }) => {
                    self.apply_linked_context(context);
                    self.emit_snapshot();
                }
                Ok(SessionCommand::ConfigChanged(config)) => {
                    self.config = *config;
                    self.eject = self.config.should_eject(self.cli_eject);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }

            if self.pending_rx.is_some() {
                self.spinner_frame = self.spinner_frame.wrapping_add(1);
            }

            let changed = self.poll_background();
            if changed {
                self.emit_snapshot();
            }

            if self.screen == Screen::Ripping {
                let rip_changed = self.tick_rip();
                if rip_changed {
                    self.emit_snapshot();
                }
            }

            if crate::CANCELLED.load(std::sync::atomic::Ordering::Relaxed) {
                self.rip
                    .cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    fn emit_snapshot(&self) {
        let _ = self
            .output_tx
            .send(SessionMessage::Snapshot(Box::new(self.snapshot())));
    }

    /// Spawn a background thread to scan this session's device for a disc.
    pub fn start_disc_scan(&mut self) {
        let device = self.device.clone();
        let max_speed = self.config.should_max_speed(self.no_max_speed);
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::Builder::new()
            .name(format!("scan-{}", self.device.display()))
            .spawn(move || {
                let dev_str = device.to_string_lossy().to_string();

                // Poll for disc presence every 2 seconds until found
                let label = loop {
                    let l = crate::disc::get_volume_label(&dev_str);
                    if !l.is_empty() {
                        break l;
                    }
                    let msg = format!("{} — no disc", dev_str);
                    if tx.send(BackgroundResult::WaitingForDisc(msg)).is_err() {
                        return; // Receiver dropped, session shutting down
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                };

                let _ = tx.send(BackgroundResult::DiscFound(dev_str.clone()));
                if max_speed {
                    crate::disc::set_max_speed(&dev_str);
                }
                let tx_progress = tx.clone();
                let result = (|| -> anyhow::Result<(String, String, Vec<Playlist>)> {
                    let playlists = crate::media::scan_playlists_with_progress(
                        &dev_str,
                        Some(&move |elapsed, timeout| {
                            let _ =
                                tx_progress.send(BackgroundResult::ScanProgress(elapsed, timeout));
                        }),
                    )
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                    Ok((dev_str, label, playlists))
                })();
                let _ = tx.send(BackgroundResult::DiscScan(result));
            })
            .expect("failed to spawn scan thread");

        self.pending_rx = Some(rx);
        self.status_message = "Scanning for disc...".into();
        self.screen = Screen::Scanning;
    }

    /// Poll for background task results. Returns true on meaningful state changes
    /// (screen transitions, TMDb results), false for incremental updates.
    pub fn poll_background(&mut self) -> bool {
        let rx = match self.pending_rx {
            Some(ref rx) => rx,
            None => return false,
        };

        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending_rx = None;
                self.status_message = "Background task failed unexpectedly".into();
                return true;
            }
        };

        match result {
            BackgroundResult::WaitingForDisc(ref msg) => {
                let device_prefix = msg.split(" — ").next().unwrap_or("");
                if !self
                    .disc
                    .scan_log
                    .iter()
                    .any(|l| l.starts_with(device_prefix))
                {
                    self.disc.scan_log.push(msg.clone());
                }
                self.status_message = "Waiting for disc...".into();
                return false; // Keep pending_rx alive
            }
            BackgroundResult::DiscFound(ref device) => {
                if self.screen == Screen::Done {
                    let label = crate::disc::get_volume_label(device);
                    self.disc_detected_label = Some(if label.is_empty() {
                        device.clone()
                    } else {
                        label
                    });
                    return true;
                }
                self.disc.scan_log.clear();
                self.status_message = format!("Scanning {}...", device);
                return false; // Keep pending_rx alive
            }
            BackgroundResult::ScanProgress(elapsed, timeout) => {
                self.status_message = format!(
                    "AACS negotiation in progress ({}s / {}s)...",
                    elapsed, timeout
                );
                return false; // Keep pending_rx alive
            }
            _ => {}
        }

        self.pending_rx = None;

        match result {
            BackgroundResult::WaitingForDisc(_)
            | BackgroundResult::DiscFound(_)
            | BackgroundResult::ScanProgress(_, _) => unreachable!(),
            BackgroundResult::DiscScan(Ok(_)) | BackgroundResult::DiscScan(Err(_))
                if self.screen == Screen::Done =>
            {
                // Ignore full scan results on Done screen
            }
            BackgroundResult::DiscScan(Ok((device, label, playlists))) => {
                self.device = PathBuf::from(device);
                self.disc.label_info = crate::disc::parse_volume_label(&label);
                self.disc.label = label;
                let min_dur = self
                    .config
                    .min_duration(self.min_duration_arg.unwrap_or(900));
                self.disc.episodes_pl = crate::disc::filter_episodes(&playlists, min_dur)
                    .into_iter()
                    .cloned()
                    .collect();
                self.disc.playlists = playlists;

                self.tmdb.movie_mode = self.movie_mode_arg
                    || (self.disc.episodes_pl.len() == 1 && self.season_arg.is_none());

                if let Some(ref info) = self.disc.label_info {
                    self.tmdb.search_query = info.show.clone();
                    if !self.tmdb.movie_mode {
                        self.wizard.season_num = Some(info.season);
                    }
                }
                if let Some(s) = self.season_arg {
                    self.wizard.season_num = Some(s);
                }
                self.wizard.start_episode = self.start_episode_arg;
                self.wizard.show_filtered = self.config.show_filtered();
                self.wizard.playlist_selected = self
                    .disc
                    .playlists
                    .iter()
                    .map(|pl| self.disc.episodes_pl.iter().any(|ep| ep.num == pl.num))
                    .collect();

                // Extract chapter counts from MPLS files
                let device_str = self.device.to_string_lossy().to_string();
                match crate::disc::ensure_mounted(&device_str) {
                    Ok((mount, did_mount)) => {
                        let nums: Vec<&str> = self
                            .disc
                            .playlists
                            .iter()
                            .map(|pl| pl.num.as_str())
                            .collect();
                        self.disc.chapter_counts = crate::chapters::count_chapters_for_playlists(
                            std::path::Path::new(&mount),
                            &nums,
                        );
                        if did_mount {
                            let _ = crate::disc::unmount_disc(&device_str);
                        }
                    }
                    Err(_) => {
                        self.disc.chapter_counts.clear();
                    }
                }

                self.status_message.clear();

                if self.disc.episodes_pl.is_empty() {
                    self.status_message = "No episode-length playlists found.".into();
                    self.screen = Screen::Done;
                } else {
                    self.screen = Screen::TmdbSearch;
                    self.wizard.input_focus = InputFocus::TextInput;
                    self.wizard.input_buffer = if self.tmdb_api_key.is_none() {
                        String::new()
                    } else {
                        self.tmdb.search_query.clone()
                    };
                }
            }
            BackgroundResult::DiscScan(Err(e)) => {
                self.status_message = format!("Scan failed: {}", e);
                self.screen = Screen::Done;
            }
            BackgroundResult::ShowSearch(Ok(results)) => {
                if results.is_empty() {
                    self.status_message = "No results found.".into();
                } else {
                    self.tmdb.search_results = results;
                    self.wizard.list_cursor = 0;
                    self.wizard.input_focus = InputFocus::List;
                    self.status_message.clear();
                }
            }
            BackgroundResult::ShowSearch(Err(e)) => {
                self.status_message = format!("TMDb search failed: {}", e);
            }
            BackgroundResult::MovieSearch(Ok(results)) => {
                if results.is_empty() {
                    self.status_message = "No results found.".into();
                } else {
                    self.tmdb.movie_results = results;
                    self.wizard.list_cursor = 0;
                    self.wizard.input_focus = InputFocus::List;
                    self.status_message.clear();
                }
            }
            BackgroundResult::MovieSearch(Err(e)) => {
                self.status_message = format!("TMDb search failed: {}", e);
            }
            BackgroundResult::SeasonFetch(Ok(eps)) => {
                self.tmdb.episodes = eps;
                self.wizard.list_cursor = 0;
                self.status_message.clear();
            }
            BackgroundResult::SeasonFetch(Err(e)) => {
                self.status_message = format!("Failed to fetch season: {}", e);
                self.tmdb.episodes.clear();
            }
            BackgroundResult::MediaProbe(infos) => {
                let selected_indices: Vec<usize> = self
                    .disc
                    .playlists
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        self.wizard
                            .playlist_selected
                            .get(*i)
                            .copied()
                            .unwrap_or(false)
                    })
                    .map(|(i, _)| i)
                    .collect();

                let filenames: Vec<String> = infos
                    .iter()
                    .zip(selected_indices.iter())
                    .map(|(info, &idx)| self.playlist_filename(idx, info.as_ref()))
                    .collect();

                self.wizard.filenames = filenames;
                self.wizard.media_infos = infos;

                if self.wizard.filenames.is_empty() {
                    self.status_message = "No playlists selected.".into();
                } else {
                    self.wizard.list_cursor = 0;
                    self.status_message.clear();
                    self.screen = Screen::Confirm;
                }
            }
        }
        true
    }

    /// Route keyboard input to the appropriate screen handler.
    pub fn handle_key(&mut self, key: KeyEvent) {
        let input_active = matches!(
            self.wizard.input_focus,
            InputFocus::TextInput | InputFocus::InlineEdit(_)
        );

        // Ctrl+C handled by coordinator — session ignores it

        // Ctrl+E: eject this session's disc (not during ripping or text input)
        if key.code == KeyCode::Char('e')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && !input_active
            && self.screen != Screen::Ripping
        {
            let device_str = self.device.to_string_lossy().to_string();
            match crate::disc::eject_disc(&device_str) {
                Ok(()) => self.status_message = "Disc ejected.".into(),
                Err(e) => {
                    self.status_message = format!("Eject failed: {}", e);
                }
            }
            return;
        }

        // Ctrl+R: rescan this session's disc
        if key.code == KeyCode::Char('r')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && !self.rip.confirm_rescan
        {
            if self.screen == Screen::Ripping {
                self.rip.confirm_rescan = true;
            } else {
                self.reset_for_rescan();
                self.tmdb_api_key = crate::tmdb::get_api_key(&self.config);
                self.start_disc_scan();
            }
            return;
        }

        // Handle rescan confirmation (during ripping)
        if self.rip.confirm_rescan {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.reset_for_rescan();
                    self.tmdb_api_key = crate::tmdb::get_api_key(&self.config);
                    self.start_disc_scan();
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.rip.confirm_rescan = false;
                }
                _ => {}
            }
            return;
        }

        // q is a no-op in session (global quit handled by coordinator)
        // except during ripping where dashboard handles it for abort confirmation

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
                        self.tmdb_api_key = crate::tmdb::get_api_key(&self.config);
                        self.start_disc_scan();
                    }
                    // Other keys in Done with detected disc: no-op for session
                    // (the coordinator decides whether to quit)
                } else if key.code == KeyCode::Enter {
                    self.reset_for_rescan();
                    self.tmdb_api_key = crate::tmdb::get_api_key(&self.config);
                    self.start_disc_scan();
                }
                // Other keys on Done: no-op for session
            }
            _ => {}
        }
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
        self.screen = Screen::PlaylistManager;
        self.wizard.input_focus = InputFocus::default();
        self.status_message.clear();
    }

    /// Tick the rip engine. Returns true if state changed.
    fn tick_rip(&mut self) -> bool {
        crate::tui::dashboard::tick_session(self)
    }

    /// Compute the output filename for a playlist at the given index.
    pub fn playlist_filename(
        &self,
        playlist_index: usize,
        media_info: Option<&MediaInfo>,
    ) -> String {
        let pl = &self.disc.playlists[playlist_index];
        let is_special = self.wizard.specials.contains(&pl.num);

        let show_name = if !self.tmdb.show_name.is_empty() {
            self.tmdb.show_name.clone()
        } else {
            self.disc
                .label_info
                .as_ref()
                .map(|l| l.show.clone())
                .unwrap_or_else(|| "Unknown".to_string())
        };

        let movie_title = if self.tmdb.movie_mode {
            let movie = self
                .tmdb
                .selected_movie
                .and_then(|i| self.tmdb.movie_results.get(i));
            let title = movie.map(|m| m.title.as_str()).unwrap_or("movie");
            let year = movie
                .and_then(|m| m.release_date.as_deref())
                .and_then(|d| d.get(..4))
                .unwrap_or("");
            Some((title.to_string(), year.to_string()))
        } else {
            None
        };

        let part = if self.tmdb.movie_mode {
            let selected_count = self.wizard.playlist_selected.iter().filter(|&&s| s).count();
            if selected_count > 1 {
                self.disc
                    .playlists
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        self.wizard
                            .playlist_selected
                            .get(*i)
                            .copied()
                            .unwrap_or(false)
                    })
                    .position(|(i, _)| i == playlist_index)
                    .map(|p| p as u32 + 1)
            } else {
                None
            }
        } else {
            None
        };

        let episodes = self
            .wizard
            .episode_assignments
            .get(&pl.num)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        crate::workflow::build_output_filename(
            pl,
            episodes,
            self.wizard.season_num.unwrap_or(0),
            self.tmdb.movie_mode,
            is_special,
            movie_title.as_ref().map(|(t, y)| (t.as_str(), y.as_str())),
            &show_name,
            &self.disc.label,
            self.disc.label_info.as_ref(),
            &self.config,
            self.format.as_deref(),
            self.format_preset.as_deref(),
            media_info,
            part,
        )
    }

    /// Get visible playlists (respecting show_filtered setting).
    pub fn visible_playlists(&self) -> Vec<(usize, &Playlist)> {
        self.disc
            .playlists
            .iter()
            .enumerate()
            .filter(|(_, pl)| {
                self.wizard.show_filtered || self.disc.episodes_pl.iter().any(|ep| ep.num == pl.num)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_test_session() -> DriveSession {
        let config = Config::default();
        let (_cmd_tx, cmd_rx) = mpsc::channel();
        let (msg_tx, _msg_rx) = mpsc::channel();
        DriveSession::new(PathBuf::from("/dev/sr0"), config, cmd_rx, msg_tx)
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
        assert_eq!(session.screen, Screen::Scanning);

        let snap = session.snapshot();
        assert!(snap.scanning.is_some());
        assert!(snap.tmdb.is_none());
        assert!(snap.season.is_none());
        assert!(snap.playlist_mgr.is_none());
        assert!(snap.confirm.is_none());
        assert!(snap.dashboard.is_none());
        assert!(snap.done.is_none());
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
                playlist: Playlist {
                    num: "00001".into(),
                    duration: "1:00:00".into(),
                    seconds: 3600,
                },
                episode: vec![Episode {
                    episode_number: 1,
                    name: "Ep 1".into(),
                    runtime: None,
                }],
                filename: "ep1.mkv".into(),
                status: PlaylistStatus::Done(1_000_000),
            },
            RipJob {
                playlist: Playlist {
                    num: "00002".into(),
                    duration: "1:00:00".into(),
                    seconds: 3600,
                },
                episode: vec![Episode {
                    episode_number: 2,
                    name: "Ep 2".into(),
                    runtime: None,
                }],
                filename: "ep2.mkv".into(),
                status: PlaylistStatus::Ripping(RipProgress {
                    frame: 1000,
                    fps: 30.0,
                    total_size: 500_000,
                    out_time_secs: 1800,
                    bitrate: "20Mbps".into(),
                    speed: 2.0,
                }),
            },
            RipJob {
                playlist: Playlist {
                    num: "00003".into(),
                    duration: "1:00:00".into(),
                    seconds: 3600,
                },
                episode: vec![Episode {
                    episode_number: 3,
                    name: "Ep 3".into(),
                    runtime: None,
                }],
                filename: "ep3.mkv".into(),
                status: PlaylistStatus::Pending,
            },
        ];
        session.rip.current_rip = 1;

        let summary = session.tab_summary();
        assert_eq!(summary.state, TabState::Ripping);
        let (current, total, overall) = summary.rip_progress.unwrap();
        assert_eq!(current, 2); // done_count(1) + 1
        assert_eq!(total, 3);
        // 1 done (100%) + 1 at 50% + 1 pending (0%) = 150/300 = 50%
        assert_eq!(overall, 50);
    }

    #[test]
    fn test_reset_for_rescan() {
        let mut session = make_test_session();
        session.disc.label = "MY_DISC".into();
        session.tmdb.show_name = "Test Show".into();
        session.tmdb.search_query = "test".into();
        session.wizard.season_num = Some(2);
        session.screen = Screen::PlaylistManager;
        session.status_message = "Something".into();

        session.reset_for_rescan();

        assert!(session.disc.label.is_empty());
        assert!(session.tmdb.show_name.is_empty());
        assert!(session.tmdb.search_query.is_empty());
        assert!(session.wizard.season_num.is_none());
        assert!(session.status_message.is_empty());
        assert!(session.pending_rx.is_none());
        assert!(session.disc_detected_label.is_none());
        // tmdb_api_key at session level is preserved
        // (it's not part of TmdbState reset)
    }

    #[test]
    fn test_next_unassigned_episode() {
        let session = make_test_session();
        assert_eq!(session.next_unassigned_episode(), 1);

        let mut session2 = make_test_session();
        let mut assignments = HashMap::new();
        assignments.insert(
            "00001".to_string(),
            vec![Episode {
                episode_number: 3,
                name: "Ep 3".into(),
                runtime: None,
            }],
        );
        assignments.insert(
            "00002".to_string(),
            vec![
                Episode {
                    episode_number: 5,
                    name: "Ep 5".into(),
                    runtime: None,
                },
                Episode {
                    episode_number: 6,
                    name: "Ep 6".into(),
                    runtime: None,
                },
            ],
        );
        session2.wizard.episode_assignments = assignments;
        assert_eq!(session2.next_unassigned_episode(), 7);
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
        session.wizard.season_num = Some(2);

        let snap = session.snapshot();
        assert!(snap.linkable_context.is_some());
        let ctx = snap.linkable_context.unwrap();
        assert_eq!(ctx.show_name, "Breaking Bad");
        assert_eq!(ctx.season_num, 2);
        assert_eq!(ctx.next_episode, 1);
        assert!(!ctx.movie_mode);
    }
}
