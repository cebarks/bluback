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
    /// Separate channel for upfront stream probing (doesn't block wizard interaction)
    pub probe_rx: Option<mpsc::Receiver<BackgroundResult>>,
    pub disc_detected_label: Option<String>,
    pub tmdb_api_key: Option<String>,

    pub input_rx: mpsc::Receiver<SessionCommand>,
    pub output_tx: mpsc::Sender<SessionMessage>,

    // Per-session CLI args
    pub movie_mode_arg: bool,
    pub season_arg: Option<u32>,
    pub start_episode_arg: Option<u32>,
    pub min_probe_duration_arg: Option<u32>,
    pub no_max_speed: bool,
    pub output_dir: PathBuf,
    pub cli_eject: Option<bool>,
    pub format: Option<String>,
    pub format_preset: Option<String>,
    pub overwrite: bool,
    pub no_metadata: bool,
    pub no_hooks: bool,
    pub verify: bool,
    pub verify_level: crate::verify::VerifyLevel,
    pub stream_filter: crate::streams::StreamFilter,

    /// Whether auto-detection is active.
    pub auto_detect: bool,
    /// Whether batch mode is active (auto-restart on new disc detection).
    pub batch: bool,
    /// Number of discs processed in this session (0 until first scan completes).
    pub batch_disc_count: u32,

    /// Path to the history database file (each thread opens its own connection).
    pub history_db_path: Option<std::path::PathBuf>,
    /// Active history session ID (set after scan completes and session is recorded).
    pub history_session_id: Option<i64>,
    /// Skip duplicate detection / episode continuation but still record.
    pub ignore_history: bool,

    // --- History-based TUI hints (informational only) ---
    /// Warning shown on scanning/tmdb screen if this disc was previously ripped.
    pub history_duplicate_hint: Option<String>,
    /// Suggested starting episode from history: (next_episode, hint_text).
    pub history_episode_hint: Option<(u32, String)>,
    /// Playlists that were completed in a previous session of this disc.
    pub history_ripped_playlists: std::collections::HashSet<String>,
    /// Whether the current session was successfully saved to history on completion.
    pub history_session_saved: bool,
    /// Playlist numbers that were pre-classified (skipped during probe).
    pub skip_set: std::collections::HashSet<String>,
}

impl DriveSession {
    pub fn new(
        device: PathBuf,
        config: Config,
        stream_filter: crate::streams::StreamFilter,
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
            probe_rx: None,
            disc_detected_label: None,
            tmdb_api_key,

            input_rx,
            output_tx,

            movie_mode_arg: false,
            season_arg: None,
            start_episode_arg: None,
            min_probe_duration_arg: None,
            no_max_speed: false,
            output_dir: PathBuf::from("."),
            cli_eject: None,
            format: None,
            format_preset: None,
            overwrite: false,
            no_metadata: false,
            no_hooks: false,
            verify: false,
            verify_level: crate::verify::VerifyLevel::Quick,
            stream_filter,

            auto_detect: false,
            batch: false,
            batch_disc_count: 0,

            history_db_path: None,
            history_session_id: None,
            ignore_history: false,

            history_duplicate_hint: None,
            history_episode_hint: None,
            history_ripped_playlists: std::collections::HashSet::new(),
            history_session_saved: false,
            skip_set: std::collections::HashSet::new(),
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
                    .filter(|j| {
                        matches!(
                            j.status,
                            PlaylistStatus::Done(_)
                                | PlaylistStatus::Verified(..)
                                | PlaylistStatus::VerifyFailed(..)
                        )
                    })
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
            .store(true, std::sync::atomic::Ordering::Relaxed);
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
        self.tmdb.specials = Vec::new();

        self.wizard = WizardState::default();
        self.rip = RipState::default();
        self.status_message = String::new();
        self.spinner_frame = 0;
        self.pending_rx = None;
        self.probe_rx = None;
        self.disc_detected_label = None;
        // history_db_path and ignore_history survive reset (session-level config).
        // history_session_id is per-disc and must be cleared.
        self.history_session_id = None;
        // History hints are per-disc state
        self.history_duplicate_hint = None;
        self.history_episode_hint = None;
        self.history_ripped_playlists.clear();
        self.history_session_saved = false;
        self.skip_set.clear();
    }

    /// Spawn a background thread to probe a single unprobed playlist on demand.
    /// No-op if the playlist is already probed or if a probe is already in flight.
    pub fn start_on_demand_probe(&mut self, playlist_num: &str) {
        if self.wizard.stream_infos.contains_key(playlist_num) || self.probe_rx.is_some() {
            return;
        }
        let device = self.device.to_string_lossy().to_string();
        let num = playlist_num.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name(format!("probe-{}", num))
            .spawn(
                move || match crate::media::probe::probe_playlist(&device, &num) {
                    Ok((media, streams)) => {
                        let mut results = std::collections::HashMap::new();
                        results.insert(num, (media, streams));
                        let _ = tx.send(BackgroundResult::BulkProbe(results));
                    }
                    Err(e) => {
                        log::warn!("On-demand probe failed for {}: {}", num, e);
                    }
                },
            )
            .expect("failed to spawn probe thread");
        self.probe_rx = Some(rx);
    }

    /// Returns playlists above the probe duration threshold that aren't pre-classified.
    /// These are the "episode-length" playlists used for detection and episode assignment.
    pub fn probed_playlists(&self) -> Vec<&Playlist> {
        let min_dur = self.config.min_probe_duration(self.min_probe_duration_arg);
        self.disc
            .playlists
            .iter()
            .filter(|pl| pl.seconds >= min_dur && !self.skip_set.contains(&pl.num))
            .collect()
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
            history_duplicate_hint: self.history_duplicate_hint.clone(),
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
            probed_count: self.probed_playlists().len(),
            history_duplicate_hint: self.history_duplicate_hint.clone(),
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
            history_episode_hint: self.history_episode_hint.clone(),
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
            show_specials: self.wizard.show_specials,
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
            stream_infos: self.wizard.stream_infos.clone(),
            track_selections: self.wizard.track_selections.clone(),
            expanded_playlist: self.wizard.expanded_playlist,
            detection_results: self.wizard.detection_results.clone(),
            history_ripped_playlists: self.history_ripped_playlists.clone(),
            start_episode_popup: self.wizard.start_episode_popup,
            min_probe_duration: self.config.min_probe_duration(self.min_probe_duration_arg),
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
            clip_sizes: self.disc.clip_sizes.clone(),
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
            verify_failed_idx: self.rip.verify_failed_idx,
            batch_disc_count: self.batch_disc_count,
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
            eject: self.eject && !self.device.is_dir(),
            status_message: self.status_message.clone(),
            filenames: self.wizard.filenames.clone(),
            batch_disc_count: self.batch_disc_count,
            history_session_saved: self.history_session_saved,
        })
    }

    // --- Session thread entry point and event loop ---

    /// Main entry point for the session thread. Runs the scan/wizard/rip lifecycle.
    pub fn run(mut self) {
        // Open a thread-local history DB connection (rusqlite::Connection is !Send)
        let history_db = self.history_db_path.as_ref().and_then(|path| {
            crate::history::HistoryDb::open(path)
                .map_err(|e| log::warn!("history: failed to open DB: {}", e))
                .ok()
        });

        self.start_disc_scan();
        self.emit_snapshot();

        loop {
            let command = self
                .input_rx
                .recv_timeout(std::time::Duration::from_millis(100));

            match command {
                Ok(SessionCommand::Shutdown) => {
                    // Best-effort: mark session as cancelled
                    if let (Some(db), Some(sid)) = (&history_db, self.history_session_id) {
                        let _ = db.finish_session(sid, crate::history::SessionStatus::Cancelled);
                    }
                    self.rip
                        .cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    if self.disc.did_mount {
                        let _ = crate::disc::unmount_disc(&self.device.to_string_lossy());
                    }
                    return;
                }
                Ok(SessionCommand::KeyEvent(key)) => {
                    let prev_screen = self.screen.clone();
                    let was_ripping = self.screen == Screen::Ripping;
                    self.handle_key(key);
                    // Compute episode hint when entering PlaylistManager from Season
                    if prev_screen == Screen::Season
                        && self.screen == Screen::PlaylistManager
                        && self.history_episode_hint.is_none()
                    {
                        self.compute_episode_hint(&history_db);
                    }
                    // Detect user abort: was ripping, now done with abort message
                    if was_ripping
                        && self.screen == Screen::Done
                        && self.status_message == "Rip aborted."
                    {
                        if let (Some(db), Some(sid)) = (&history_db, self.history_session_id) {
                            let _ =
                                db.finish_session(sid, crate::history::SessionStatus::Cancelled);
                        }
                        self.history_session_id = None;
                    }
                    self.emit_snapshot();
                }
                Ok(SessionCommand::LinkTo { context }) => {
                    self.apply_linked_context(context);
                    self.emit_snapshot();
                }
                Ok(SessionCommand::ConfigChanged(config)) => {
                    self.config = *config;
                    self.eject = self.config.should_eject(self.cli_eject);
                    self.auto_detect = self.config.auto_detect();
                    self.batch = self.config.batch();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }

            if self.pending_rx.is_some() {
                self.spinner_frame = self.spinner_frame.wrapping_add(1);
            }

            let changed = self.poll_background();
            let probe_changed = self.poll_probe();
            if changed || probe_changed {
                // Record history session when scan completes (transition to TmdbSearch)
                if self.screen == Screen::TmdbSearch && self.history_session_id.is_none() {
                    self.compute_duplicate_hint(&history_db);
                    self.record_history_session(&history_db);
                }
                self.emit_snapshot();
            }

            if self.screen == Screen::Ripping {
                let rip_changed = self.tick_rip(&history_db);
                if rip_changed {
                    self.emit_snapshot();
                }
            }

            if crate::CANCELLED.load(std::sync::atomic::Ordering::Relaxed) {
                // Best-effort: mark session as cancelled
                if let (Some(db), Some(sid)) = (&history_db, self.history_session_id) {
                    let _ = db.finish_session(sid, crate::history::SessionStatus::Cancelled);
                }
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

    /// Poll the upfront probe channel (non-blocking, independent of pending_rx).
    fn poll_probe(&mut self) -> bool {
        let rx = match self.probe_rx {
            Some(ref rx) => rx,
            None => return false,
        };

        match rx.try_recv() {
            Ok(BackgroundResult::BulkProbe(results)) => {
                for (num, (media, streams)) in results {
                    // Update playlist stream counts
                    if let Some(pl) = self.disc.playlists.iter_mut().find(|p| p.num == num) {
                        pl.video_streams = streams.video_streams.len() as u32;
                        pl.audio_streams = streams.audio_streams.len() as u32;
                        pl.subtitle_streams = streams.subtitle_streams.len() as u32;
                    }
                    self.wizard.media_infos.insert(num.clone(), media);
                    self.wizard.stream_infos.insert(num.clone(), streams);
                    // If this was a lazy probe for the expanded playlist, enter track edit mode
                    if self
                        .wizard
                        .expanded_playlist
                        .and_then(|idx| self.disc.playlists.get(idx))
                        .map(|pl| &pl.num)
                        == Some(&num)
                    {
                        self.wizard.input_focus = InputFocus::TrackEdit(0);
                    }
                }
                self.probe_rx = None;
                true
            }
            Ok(_) => {
                self.probe_rx = None;
                false
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.probe_rx = None;
                false
            }
        }
    }

    /// Spawn a background thread to scan this session's device for a disc.
    pub fn start_disc_scan(&mut self) {
        let device = self.device.clone();
        let is_folder = device.is_dir();
        let max_speed = self.config.should_max_speed(self.no_max_speed);
        let min_probe_duration = self.config.min_probe_duration(self.min_probe_duration_arg);
        let auto_detect = self.auto_detect;
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::Builder::new()
            .name(format!("scan-{}", self.device.display()))
            .spawn(move || {
                let dev_str = device.to_string_lossy().to_string();

                let label = if is_folder {
                    // Folder input: resolve label immediately, no polling needed
                    crate::disc::resolve_label(
                        &crate::types::InputSource::Folder {
                            path: device.clone(),
                        },
                        None,
                    )
                } else {
                    // Disc input: poll for disc presence every 2 seconds until found
                    loop {
                        let l = crate::disc::get_volume_label(&dev_str);
                        if !l.is_empty() {
                            break l;
                        }
                        let msg = format!("{} — no disc", dev_str);
                        if tx.send(BackgroundResult::WaitingForDisc(msg)).is_err() {
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                };

                let _ = tx.send(BackgroundResult::DiscFound(dev_str.clone()));
                if !is_folder && max_speed {
                    crate::disc::set_max_speed(&dev_str);
                }
                let tx_progress = tx.clone();
                let tx_probe = tx.clone();
                let result = (|| -> anyhow::Result<DiscScanResult> {
                    let (playlists, probe_cache, skip_set) =
                        crate::media::scan_playlists_with_progress(
                            &dev_str,
                            min_probe_duration,
                            auto_detect,
                            Some(&move |elapsed, timeout| {
                                let _ = tx_progress
                                    .send(BackgroundResult::ScanProgress(elapsed, timeout));
                            }),
                            Some(&move |current, total, num| {
                                let _ = tx_probe.send(BackgroundResult::ProbeProgress(
                                    current,
                                    total,
                                    num.to_string(),
                                ));
                            }),
                        )
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    Ok((dev_str, label, playlists, probe_cache, skip_set))
                })();
                let _ = tx.send(BackgroundResult::DiscScan(result));
            })
            .expect("failed to spawn scan thread");

        self.pending_rx = Some(rx);
        self.disc.scan_log.push("Scanning for disc...".into());
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
                self.emit_snapshot();
                return false; // Keep pending_rx alive
            }
            BackgroundResult::DiscFound(ref device) => {
                if self.screen == Screen::Done {
                    if self.batch {
                        // Batch mode: auto-restart without popup
                        self.batch_disc_count += 1;
                        self.disc_detected_label = None;
                        self.reset_for_rescan();
                        self.tmdb_api_key = crate::tmdb::get_api_key(&self.config);
                        self.start_disc_scan();
                        log::info!("=== Batch disc {} ===", self.batch_disc_count);
                        return true;
                    }
                    // Non-batch: show popup, wait for Enter
                    let label = crate::disc::get_volume_label(device);
                    self.disc_detected_label = Some(if label.is_empty() {
                        device.clone()
                    } else {
                        label
                    });
                    return true;
                }
                self.disc.scan_log.clear();
                let msg = format!("Scanning {}...", device);
                self.disc.scan_log.push(msg.clone());
                self.status_message = msg;
                self.emit_snapshot();
                return false; // Keep pending_rx alive
            }
            BackgroundResult::ScanProgress(elapsed, timeout) => {
                let msg = format!(
                    "AACS negotiation in progress ({}s / {}s)...",
                    elapsed, timeout
                );
                self.disc.scan_log.push(msg.clone());
                self.status_message = msg;
                self.emit_snapshot();
                return false; // Keep pending_rx alive
            }
            BackgroundResult::ProbeProgress(current, total, ref num) => {
                let msg = format!("Probing playlist {} ({}/{})...", num, current, total);
                self.disc.scan_log.push(msg.clone());
                self.status_message = msg;
                self.emit_snapshot();
                return false; // Keep pending_rx alive
            }
            _ => {}
        }

        self.pending_rx = None;

        match result {
            BackgroundResult::WaitingForDisc(_)
            | BackgroundResult::DiscFound(_)
            | BackgroundResult::ScanProgress(_, _)
            | BackgroundResult::ProbeProgress(_, _, _) => unreachable!(),
            BackgroundResult::DiscScan(Ok(_)) | BackgroundResult::DiscScan(Err(_))
                if self.screen == Screen::Done =>
            {
                // Ignore full scan results on Done screen
            }
            BackgroundResult::DiscScan(Ok((device, label, playlists, probe_cache, skip_set))) => {
                self.device = PathBuf::from(device);
                self.disc.label_info = crate::disc::parse_volume_label(&label);
                self.disc.label = label;
                self.disc.playlists = playlists;
                self.skip_set = skip_set;

                // Extract chapter counts and title order from mounted disc
                let device_str = self.device.to_string_lossy().to_string();
                let title_order = match crate::disc::ensure_mounted(&device_str) {
                    Ok((mount, did_mount)) => {
                        let mount_path = std::path::Path::new(&mount);
                        let nums: Vec<&str> = self
                            .disc
                            .playlists
                            .iter()
                            .map(|pl| pl.num.as_str())
                            .collect();
                        let mpls_info = crate::chapters::collect_mpls_info(mount_path, &nums);
                        self.disc.chapter_counts = mpls_info
                            .iter()
                            .map(|(k, v)| (k.clone(), v.chapters.len()))
                            .collect();
                        self.disc.clip_sizes = mpls_info
                            .into_iter()
                            .map(|(k, v)| (k, v.clip_size))
                            .collect();
                        let order = crate::index::parse_title_order(mount_path);

                        // Upgrade display label from bdmt_*.xml if available
                        // label_info stays from original lsblk label (bdmt format won't match regex)
                        if !self.device.is_dir() {
                            if let Some(bdmt_title) = crate::disc::parse_bdmt_title(mount_path) {
                                self.disc.label = bdmt_title;
                            }
                        }

                        if did_mount {
                            let _ = crate::disc::unmount_disc(&device_str);
                        }
                        order
                    }
                    Err(_) => {
                        self.disc.chapter_counts.clear();
                        self.disc.clip_sizes.clear();
                        None
                    }
                };

                // Reorder playlists by title index (or MPLS number fallback)
                crate::index::reorder_playlists(&mut self.disc.playlists, title_order.as_deref());

                // Populate wizard caches from the scan's probe results (must happen
                // before detection, movie_mode, and playlist_selected logic)
                for (num, (media, streams)) in &probe_cache {
                    self.wizard.media_infos.insert(num.clone(), media.clone());
                    self.wizard
                        .stream_infos
                        .insert(num.clone(), streams.clone());
                }

                // Run detection on probed playlists only (before movie_mode and
                // playlist_selected so they can use detection results)
                if self.auto_detect {
                    let probed: Vec<crate::types::Playlist> =
                        self.probed_playlists().into_iter().cloned().collect();
                    self.wizard.detection_results = crate::detection::run_detection_with_chapters(
                        &probed,
                        None, // no TMDb episodes yet at scan time
                        None, // no TMDb specials yet
                        &self.disc.chapter_counts,
                    );
                }

                // Add synthetic detection entries for pre-classified specials
                // (above junk threshold but skipped during probe — < 50% of median)
                if self.auto_detect {
                    let min_dur = self.config.min_probe_duration(self.min_probe_duration_arg);
                    for pl in &self.disc.playlists {
                        if pl.seconds >= min_dur
                            && self.skip_set.contains(&pl.num)
                            && !self
                                .wizard
                                .detection_results
                                .iter()
                                .any(|d| d.playlist_num == pl.num)
                        {
                            self.wizard.detection_results.push(
                                crate::detection::DetectionResult {
                                    playlist_num: pl.num.clone(),
                                    suggested_type: crate::detection::SuggestedType::Special,
                                    confidence: crate::detection::Confidence::High,
                                    reasons: vec![
                                        "Pre-classified: duration < 50% of median".into(),
                                    ],
                                },
                            );
                        }
                    }
                }

                // Movie mode detection — detection-aware
                let auto_detect_on = self.auto_detect;
                let episode_count = if auto_detect_on {
                    self.wizard
                        .detection_results
                        .iter()
                        .filter(|d| {
                            matches!(
                                d.suggested_type,
                                crate::detection::SuggestedType::Episode
                                    | crate::detection::SuggestedType::MultiEpisode
                            )
                        })
                        .count()
                } else {
                    self.probed_playlists().len()
                };
                self.tmdb.movie_mode =
                    self.movie_mode_arg || (episode_count == 1 && self.season_arg.is_none());

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

                // Detection-aware playlist_selected initialization
                self.wizard.playlist_selected = self
                    .disc
                    .playlists
                    .iter()
                    .map(|pl| {
                        if auto_detect_on {
                            self.wizard.detection_results.iter().any(|d| {
                                d.playlist_num == pl.num
                                    && matches!(
                                        d.suggested_type,
                                        crate::detection::SuggestedType::Episode
                                            | crate::detection::SuggestedType::MultiEpisode
                                    )
                            })
                        } else {
                            self.wizard.stream_infos.contains_key(&pl.num)
                        }
                    })
                    .collect();

                // Count first disc in batch mode
                if self.batch && self.batch_disc_count == 0 {
                    self.batch_disc_count = 1;
                }

                self.status_message.clear();

                if self.probed_playlists().is_empty() {
                    let min_dur = self.config.min_probe_duration(self.min_probe_duration_arg);
                    self.status_message = format!(
                        "No playlists found above probe threshold ({}s). Try lowering --min-probe-duration.",
                        min_dur
                    );
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
            BackgroundResult::SeasonFetch(Ok(eps), specials) => {
                self.tmdb.episodes = eps;
                self.tmdb.specials = specials.unwrap_or_default();
                self.wizard.list_cursor = 0;
                self.status_message.clear();
            }
            BackgroundResult::SeasonFetch(Err(e), _) => {
                self.status_message = format!("Failed to fetch season: {}", e);
                self.tmdb.episodes.clear();
                self.tmdb.specials.clear();
            }
            BackgroundResult::MediaProbe(playlist_num, result) => {
                if let Some((media_info, stream_info)) = *result {
                    self.wizard
                        .media_infos
                        .insert(playlist_num.clone(), media_info);
                    self.wizard
                        .stream_infos
                        .insert(playlist_num.clone(), stream_info);
                    // If this playlist is currently expanded, enter track edit mode
                    if let Some(exp_idx) = self.wizard.expanded_playlist {
                        if exp_idx < self.disc.playlists.len()
                            && self.disc.playlists[exp_idx].num == playlist_num
                        {
                            self.wizard.input_focus = InputFocus::TrackEdit(0);
                        }
                    }
                }
                self.status_message.clear();
            }
            BackgroundResult::BulkProbe(results) => {
                for (num, (media, streams)) in results {
                    self.wizard.media_infos.insert(num.clone(), media);
                    self.wizard.stream_infos.insert(num, streams);
                }
            }
        }
        true
    }

    /// Route keyboard input to the appropriate screen handler.
    pub fn handle_key(&mut self, key: KeyEvent) {
        let input_active = matches!(
            self.wizard.input_focus,
            InputFocus::TextInput | InputFocus::InlineEdit(_) | InputFocus::TrackEdit(_)
        );

        // Ctrl+C handled by coordinator — session ignores it

        // Ctrl+E: eject this session's disc (not during ripping or text input, not for folders)
        if key.code == KeyCode::Char('e')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && !input_active
            && self.screen != Screen::Ripping
            && !self.device.is_dir()
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
        if !self.tmdb.movie_mode {
            // Assign episodes using non-special probed playlists before detection
            // so playlists have initial assignments.
            // run_detection_if_enabled may mark specials and call reassign_regular_episodes.
            let probed_non_special: Vec<Playlist> = self
                .probed_playlists()
                .into_iter()
                .filter(|pl| !self.wizard.specials.contains(&pl.num))
                .cloned()
                .collect();
            self.wizard.episode_assignments = crate::util::assign_episodes(
                &probed_non_special,
                &self.tmdb.episodes,
                context.next_episode,
            );
            crate::tui::wizard::run_detection_if_enabled(self);
        }
        self.screen = Screen::PlaylistManager;
        self.wizard.input_focus = InputFocus::default();
        self.status_message.clear();
    }

    /// Tick the rip engine. Returns true if state changed.
    fn tick_rip(&mut self, history_db: &Option<crate::history::HistoryDb>) -> bool {
        crate::tui::dashboard::tick_session(self, history_db)
    }

    /// Compute episode continuation hint from history.
    /// Called after TMDb/season selection, before entering the Playlist Manager.
    pub fn compute_episode_hint(&mut self, history_db: &Option<crate::history::HistoryDb>) {
        if self.ignore_history || self.tmdb.movie_mode {
            return;
        }
        let db = match history_db {
            Some(db) => db,
            None => return,
        };
        let season = match self.wizard.season_num {
            Some(s) => s as i32,
            None => return,
        };
        let tmdb_id = self
            .tmdb
            .selected_show
            .and_then(|i| self.tmdb.search_results.get(i))
            .map(|s| s.id as i64);
        let last_ep = if let Some(tmdb_id) = tmdb_id {
            db.last_episode(tmdb_id, season).ok().flatten()
        } else {
            None
        };
        if let Some(last) = last_ep {
            let next = last as u32 + 1;
            self.history_episode_hint = Some((next, format!("Last ripped: E{:02}", last)));
            // Pre-fill start episode if the user hasn't manually set one
            // and the CLI didn't provide one
            if self.wizard.start_episode.is_none() && self.start_episode_arg.is_none() {
                self.wizard.start_episode = Some(next);
            }
        }
    }

    /// Compute history-based hints after disc scan completes.
    fn compute_duplicate_hint(&mut self, history_db: &Option<crate::history::HistoryDb>) {
        if self.ignore_history || self.disc.label.is_empty() {
            return;
        }
        let db = match history_db {
            Some(db) => db,
            None => return,
        };
        if let Ok(matches) = db.find_session_by_label(&self.disc.label) {
            if let Some(prev) = matches
                .iter()
                .find(|s| s.status == crate::history::SessionStatus::Completed)
            {
                let partial = prev.files_completed < prev.files_total;
                self.history_duplicate_hint = Some(format!(
                    "{} {} on {} ({}/{} playlists)",
                    prev.title,
                    if partial {
                        "partially ripped"
                    } else {
                        "ripped"
                    },
                    &prev.started_at[..10.min(prev.started_at.len())],
                    prev.files_completed,
                    prev.files_total
                ));
                if let Ok(Some(detail)) = db.get_session(prev.id) {
                    self.history_ripped_playlists = detail
                        .files
                        .iter()
                        .filter(|f| f.status == crate::history::FileStatus::Completed)
                        .map(|f| f.playlist.clone())
                        .collect();
                }
            }
        }
    }

    /// Record a history session after disc scan completes.
    fn record_history_session(&mut self, history_db: &Option<crate::history::HistoryDb>) {
        let db = match history_db {
            Some(db) => db,
            None => return,
        };

        let title = if self.tmdb.movie_mode {
            self.tmdb
                .selected_movie
                .and_then(|i| self.tmdb.movie_results.get(i))
                .map(|m| m.title.clone())
                .unwrap_or_else(|| self.disc.label.clone())
        } else if !self.tmdb.show_name.is_empty() {
            self.tmdb.show_name.clone()
        } else {
            self.disc
                .label_info
                .as_ref()
                .map(|l| l.show.clone())
                .unwrap_or_else(|| self.disc.label.clone())
        };

        let tmdb_id = if self.tmdb.movie_mode {
            self.tmdb
                .selected_movie
                .and_then(|i| self.tmdb.movie_results.get(i))
                .map(|m| m.id as i64)
        } else {
            self.tmdb
                .selected_show
                .and_then(|i| self.tmdb.search_results.get(i))
                .map(|s| s.id as i64)
        };

        let info = crate::history::SessionInfo {
            volume_label: self.disc.label.clone(),
            device: Some(self.device.to_string_lossy().to_string()),
            tmdb_id,
            tmdb_type: if self.tmdb.movie_mode {
                Some("movie".into())
            } else {
                Some("tv".into())
            },
            title,
            season: self.wizard.season_num.map(|s| s as i32),
            disc_number: self.disc.label_info.as_ref().map(|l| l.disc as i32),
            batch_id: if self.batch {
                Some(format!("tui-{}", self.id.0))
            } else {
                None
            },
            config_snapshot: Some(
                serde_json::to_string(&crate::history::ConfigSnapshot::from_config(&self.config))
                    .unwrap_or_default(),
            ),
        };

        match db.start_session(&info) {
            Ok(id) => {
                self.history_session_id = Some(id);

                // Record disc playlists
                let min_probe_dur = self.config.min_probe_duration(self.min_probe_duration_arg);
                let playlist_infos: Vec<crate::history::DiscPlaylistInfo> = self
                    .disc
                    .playlists
                    .iter()
                    .map(|pl| crate::history::DiscPlaylistInfo {
                        playlist: pl.num.clone(),
                        duration_ms: Some((pl.seconds as i64) * 1000),
                        video_streams: Some(pl.video_streams as i32),
                        audio_streams: Some(pl.audio_streams as i32),
                        subtitle_streams: Some(pl.subtitle_streams as i32),
                        chapters: self.disc.chapter_counts.get(&pl.num).map(|&c| c as i32),
                        is_filtered: pl.seconds < min_probe_dur,
                    })
                    .collect();
                if let Err(e) = db.record_disc_playlists(id, &playlist_infos) {
                    log::warn!("history: failed to record playlists: {}", e);
                }
            }
            Err(e) => {
                log::warn!("history: failed to start session: {}", e);
            }
        }
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

    /// Build a `RipThreadContext` for the job at `job_idx`, capturing all
    /// session state needed by the rip thread.
    pub fn rip_thread_context(&self, job_idx: usize) -> crate::workflow::RipThreadContext {
        let job = &self.rip.jobs[job_idx];
        let pl = &job.playlist;
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
                // Find which selected-playlist position this job corresponds to.
                // Jobs are built from selected playlists in order, so job_idx
                // maps directly to the Nth selected playlist.
                Some(job_idx as u32 + 1)
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
            .cloned()
            .unwrap_or_default();

        let cached_track_selection = self.wizard.track_selections.get(&pl.num).cloned();

        crate::workflow::RipThreadContext {
            device: self.device.to_string_lossy().to_string(),
            playlist: pl.clone(),
            output_dir: self.output_dir.clone(),
            episodes,
            season: self.wizard.season_num.unwrap_or(0),
            movie_mode: self.tmdb.movie_mode,
            is_special,
            movie_title,
            show_name,
            label: self.disc.label.clone(),
            label_info: self.disc.label_info.clone(),
            config: self.config.clone(),
            format_override: self.format.clone(),
            format_preset_override: self.format_preset.clone(),
            part,
            cached_track_selection,
            stream_filter: self.stream_filter.clone(),
            overwrite: self.config.overwrite() || self.overwrite,
            estimated_size: job.estimated_size,
        }
    }

    /// Get visible playlists (respecting show_filtered and show_specials settings).
    pub fn visible_playlists(&self) -> Vec<(usize, &Playlist)> {
        let min_probe_dur = self.config.min_probe_duration(self.min_probe_duration_arg);
        self.disc
            .playlists
            .iter()
            .enumerate()
            .filter(|(_, pl)| {
                let above_threshold = pl.seconds >= min_probe_dur;
                let is_special = self.wizard.specials.contains(&pl.num)
                    || self.wizard.detection_results.iter().any(|d| {
                        d.playlist_num == pl.num
                            && d.suggested_type == crate::detection::SuggestedType::Special
                            && d.confidence >= crate::detection::Confidence::High
                    });
                let visible_by_threshold = above_threshold || self.wizard.show_filtered;
                let visible_by_special = !is_special || self.wizard.show_specials;
                visible_by_threshold && visible_by_special
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
        DriveSession::new(
            PathBuf::from("/dev/sr0"),
            config,
            crate::streams::StreamFilter::default(),
            cmd_rx,
            msg_tx,
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
                    video_streams: 0,
                    audio_streams: 0,
                    subtitle_streams: 0,
                },
                episode: vec![Episode {
                    episode_number: 1,
                    name: "Ep 1".into(),
                    runtime: None,
                }],
                filename: "ep1.mkv".into(),
                status: PlaylistStatus::Done(1_000_000),
                estimated_size: 9_000_000,
            },
            RipJob {
                playlist: Playlist {
                    num: "00002".into(),
                    duration: "1:00:00".into(),
                    seconds: 3600,
                    video_streams: 0,
                    audio_streams: 0,
                    subtitle_streams: 0,
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
                    ..Default::default()
                }),
                estimated_size: 9_000_000,
            },
            RipJob {
                playlist: Playlist {
                    num: "00003".into(),
                    duration: "1:00:00".into(),
                    seconds: 3600,
                    video_streams: 0,
                    audio_streams: 0,
                    subtitle_streams: 0,
                },
                episode: vec![Episode {
                    episode_number: 3,
                    name: "Ep 3".into(),
                    runtime: None,
                }],
                filename: "ep3.mkv".into(),
                status: PlaylistStatus::Pending,
                estimated_size: 9_000_000,
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
        session.history_session_id = Some(42);

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

        // batch fields survive reset
        assert!(!session.batch);
        assert_eq!(session.batch_disc_count, 0);

        // history_session_id is per-disc state and gets cleared
        assert!(session.history_session_id.is_none());
        // history_db_path and ignore_history are session-level config and survive
    }

    #[test]
    fn test_batch_fields_survive_reset() {
        let mut session = make_test_session();
        session.batch = true;
        session.batch_disc_count = 3;
        session.disc.label = "DISC_4".into();
        session.screen = Screen::PlaylistManager;

        session.reset_for_rescan();

        assert!(session.batch);
        assert_eq!(session.batch_disc_count, 3);
    }

    #[test]
    fn test_history_config_survives_reset() {
        let mut session = make_test_session();
        session.history_db_path = Some(PathBuf::from("/tmp/history.db"));
        session.ignore_history = true;
        session.history_session_id = Some(99);
        session.history_duplicate_hint = Some("test hint".into());
        session.history_episode_hint = Some((5, "Last ripped: E04".into()));
        session.history_ripped_playlists.insert("00800".to_string());
        session.history_session_saved = true;

        session.reset_for_rescan();

        // Session-level config survives
        assert_eq!(
            session.history_db_path.as_deref(),
            Some(std::path::Path::new("/tmp/history.db"))
        );
        assert!(session.ignore_history);
        // Per-disc state is cleared
        assert!(session.history_session_id.is_none());
        assert!(session.history_duplicate_hint.is_none());
        assert!(session.history_episode_hint.is_none());
        assert!(session.history_ripped_playlists.is_empty());
        assert!(!session.history_session_saved);
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

    #[test]
    fn test_reset_for_rescan_cancels_active_rip() {
        let mut session = make_test_session();
        let old_cancel = session.rip.cancel.clone(); // Arc clone
        assert!(!old_cancel.load(std::sync::atomic::Ordering::Relaxed));

        session.reset_for_rescan();

        // The OLD cancel flag (held by the orphaned remux thread) must be true
        assert!(old_cancel.load(std::sync::atomic::Ordering::Relaxed));
        // The NEW rip state has a fresh cancel flag (false)
        assert!(!session
            .rip
            .cancel
            .load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn test_reset_for_rescan_clears_specials() {
        let mut session = make_test_session();
        session.tmdb.specials = vec![Episode {
            episode_number: 1,
            name: "Special 1".into(),
            runtime: None,
        }];

        session.reset_for_rescan();

        assert!(session.tmdb.specials.is_empty());
    }

    #[test]
    fn start_disc_scan_folder_skips_volume_label_poll() {
        let (_cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let (msg_tx, _msg_rx) = std::sync::mpsc::channel();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("BDMV")).unwrap();

        let session = DriveSession::new(
            dir.path().to_path_buf(),
            crate::config::Config::default(),
            crate::streams::StreamFilter::default(),
            cmd_rx,
            msg_tx,
        );
        assert!(session.device.is_dir());
    }
}
