use std::path::PathBuf;
use std::sync::mpsc;

use crate::config::Config;
use crate::tui::{DiscState, RipState, Screen, TmdbState, WizardState};
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

                // Overall progress: each done job = 100%, current job = current_pct
                let overall = if total > 0 {
                    ((done_count as f64 * 100.0 + current_pct as f64) / total as f64) as u8
                } else {
                    0
                };

                (
                    TabState::Ripping,
                    Some((done_count + 1, total, overall)),
                )
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
        })
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
