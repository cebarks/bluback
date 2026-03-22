pub mod dashboard;
pub mod wizard;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use std::collections::HashMap;
use std::io;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::types::*;
use crate::Args;

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Scanning,
    TmdbSearch,
    ShowSelect,
    SeasonEpisode,
    EpisodeMapping,
    PlaylistSelect,
    Confirm,
    Ripping,
    Done,
}

pub struct App {
    pub screen: Screen,
    pub args: Args,
    pub quit: bool,

    // Disc data
    pub label: String,
    pub label_info: Option<LabelInfo>,
    pub playlists: Vec<Playlist>,
    pub episodes_pl: Vec<Playlist>,

    // TMDb data
    pub api_key: Option<String>,
    pub search_query: String,
    pub movie_mode: bool,
    pub search_results: Vec<TmdbShow>,
    pub selected_show: Option<usize>,
    pub movie_results: Vec<TmdbMovie>,
    pub selected_movie: Option<usize>,
    pub season_num: Option<u32>,
    pub episodes: Vec<Episode>,
    pub start_episode: Option<u32>,
    pub episode_assignments: EpisodeAssignments,

    // Selection state
    pub playlist_selected: Vec<bool>,
    pub filenames: Vec<String>,
    pub media_infos: Vec<Option<MediaInfo>>,
    pub list_cursor: usize,

    // Text input
    pub input_buffer: String,
    pub input_active: bool,
    /// 0 = editing season, 1 = editing start episode
    pub season_field: u8,
    /// Which row is being edited in EpisodeMapping screen (None = navigation mode)
    pub mapping_edit_row: Option<usize>,

    // Rip state
    pub rip_jobs: Vec<RipJob>,
    pub current_rip: usize,
    pub rip_child: Option<std::process::Child>,
    pub progress_rx: Option<mpsc::Receiver<String>>,
    pub progress_state: HashMap<String, String>,
    pub stderr_buffer: Option<Arc<Mutex<String>>>,
    pub confirm_abort: bool,
    pub confirm_rescan: bool,

    // Config
    pub config: crate::config::Config,
    pub show_name: String,

    // Status/error messages
    pub status_message: String,
    pub scan_log: Vec<String>,

    // Eject
    pub eject: bool,

    // Chapter extraction
    pub has_mkvpropedit: bool,
    pub mount_point: Option<String>,
    pub did_mount: bool,
    pub chapter_counts: HashMap<String, usize>,

    // Background task channel (disc scan, TMDb, media probes)
    pub pending_rx: Option<mpsc::Receiver<BackgroundResult>>,
}

impl App {
    pub fn new(args: Args) -> Self {
        Self {
            screen: Screen::Scanning,
            args,
            quit: false,
            label: String::new(),
            label_info: None,
            playlists: Vec::new(),
            episodes_pl: Vec::new(),
            api_key: None,
            search_query: String::new(),
            movie_mode: false,
            search_results: Vec::new(),
            selected_show: None,
            movie_results: Vec::new(),
            selected_movie: None,
            season_num: None,
            episodes: Vec::new(),
            start_episode: None,
            episode_assignments: HashMap::new(),
            playlist_selected: Vec::new(),
            filenames: Vec::new(),
            media_infos: Vec::new(),
            list_cursor: 0,
            input_buffer: String::new(),
            input_active: false,
            season_field: 0,
            mapping_edit_row: None,
            rip_jobs: Vec::new(),
            current_rip: 0,
            rip_child: None,
            progress_rx: None,
            progress_state: HashMap::new(),
            stderr_buffer: None,
            confirm_abort: false,
            confirm_rescan: false,
            config: crate::config::Config::default(),
            show_name: String::new(),
            status_message: String::new(),
            scan_log: Vec::new(),
            eject: false,
            has_mkvpropedit: false,
            mount_point: None,
            did_mount: false,
            chapter_counts: HashMap::new(),
            pending_rx: None,
        }
    }
}

fn start_disc_scan(app: &mut App) {
    let explicit_device = app.args.device.clone();
    let max_speed = app.config.should_max_speed(app.args.no_max_speed);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // Build device list: explicit device only, or all detected drives
        let devices: Vec<String> = if let Some(ref dev) = explicit_device {
            vec![dev.to_string_lossy().to_string()]
        } else {
            crate::disc::detect_optical_drives()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        };

        // Poll devices for disc presence
        let found = 'poll: loop {
            for dev in &devices {
                let label = crate::disc::get_volume_label(dev);
                if !label.is_empty() {
                    break 'poll dev.clone();
                }
                let msg = format!("{} — no disc", dev);
                if tx.send(BackgroundResult::WaitingForDisc(msg)).is_err() {
                    return;
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        };

        if tx.send(BackgroundResult::DiscFound(found.clone())).is_err() {
            return;
        }

        if max_speed {
            crate::disc::set_max_speed(&found);
        }
        let result = (|| -> anyhow::Result<(String, String, Vec<Playlist>)> {
            let label = crate::disc::get_volume_label(&found);
            let playlists = crate::disc::scan_playlists(&found)?;
            Ok((found, label, playlists))
        })();
        let _ = tx.send(BackgroundResult::DiscScan(result));
    });
    app.pending_rx = Some(rx);
    app.status_message = "Scanning for disc...".into();
    app.screen = Screen::Scanning;
}

impl App {
    pub fn reset_for_rescan(&mut self) {
        // Kill any active rip
        if let Some(ref mut child) = self.rip_child {
            let _ = child.kill();
            let _ = child.wait();
        }

        // Clear all disc/TMDb/rip state, preserving args, config, api_key, eject
        self.label = String::new();
        self.label_info = None;
        self.playlists = Vec::new();
        self.episodes_pl = Vec::new();
        self.search_query = String::new();
        self.movie_mode = false;
        self.search_results = Vec::new();
        self.selected_show = None;
        self.movie_results = Vec::new();
        self.selected_movie = None;
        self.season_num = None;
        self.episodes = Vec::new();
        self.start_episode = None;
        self.episode_assignments = HashMap::new();
        self.playlist_selected = Vec::new();
        self.filenames = Vec::new();
        self.list_cursor = 0;
        self.input_buffer = String::new();
        self.input_active = false;
        self.season_field = 0;
        self.mapping_edit_row = None;
        self.rip_jobs = Vec::new();
        self.current_rip = 0;
        self.rip_child = None;
        self.progress_rx = None;
        self.progress_state = HashMap::new();
        self.stderr_buffer = None;
        self.confirm_abort = false;
        self.confirm_rescan = false;
        self.show_name = String::new();
        self.status_message = String::new();
        self.scan_log = Vec::new();
        self.pending_rx = None;
        if self.did_mount {
            let _ = crate::disc::unmount_disc(&self.args.device().to_string_lossy());
        }
        self.mount_point = None;
        self.did_mount = false;
        self.chapter_counts.clear();
    }
}

pub fn run(args: &Args, config: &crate::config::Config) -> Result<()> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, args, config);

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    args: &Args,
    config: &crate::config::Config,
) -> Result<()> {
    let mut app = App::new(args.clone());
    app.config = config.clone();
    app.eject = config.should_eject(args.cli_eject());
    app.has_mkvpropedit = crate::disc::has_mkvpropedit();

    // Spawn disc scan in background thread
    app.api_key = crate::tmdb::get_api_key(config);
    start_disc_scan(&mut app);

    // Event loop
    loop {
        terminal.draw(|f| match app.screen {
            Screen::Scanning => wizard::render_scanning(f, &app),
            Screen::TmdbSearch => wizard::render_tmdb_search(f, &app),
            Screen::ShowSelect => wizard::render_show_select(f, &app),
            Screen::SeasonEpisode => wizard::render_season_episode(f, &app),
            Screen::EpisodeMapping => wizard::render_episode_mapping(f, &app),
            Screen::PlaylistSelect => wizard::render_playlist_select(f, &app),
            Screen::Confirm => wizard::render_confirm(f, &app),
            Screen::Ripping => dashboard::render(f, &app),
            Screen::Done => dashboard::render_done(f, &app),
        })?;

        if app.quit {
            if let Some(ref mut child) = app.rip_child {
                let _ = child.kill();
                let _ = child.wait();
            }
            break;
        }

        // Poll for events
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Global quit (not during ripping -- dashboard handles its own q)
                if key.code == KeyCode::Char('q')
                    && !app.input_active
                    && app.screen != Screen::Ripping
                {
                    app.quit = true;
                    continue;
                }
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.quit = true;
                    continue;
                }

                // Global Ctrl+E: eject disc (not during ripping or text input)
                if key.code == KeyCode::Char('e')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !app.input_active
                    && app.screen != Screen::Ripping
                {
                    if let Some(ref device) = app.args.device {
                        let device_str = device.to_string_lossy().to_string();
                        match crate::disc::eject_disc(&device_str) {
                            Ok(()) => app.status_message = "Disc ejected.".into(),
                            Err(e) => {
                                app.status_message = format!("Eject failed: {}", e);
                            }
                        }
                    } else {
                        app.status_message = "No disc device detected yet.".into();
                    }
                    continue;
                }

                // Global Ctrl+R: rescan disc
                if key.code == KeyCode::Char('r')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !app.confirm_rescan
                {
                    if app.screen == Screen::Ripping {
                        app.confirm_rescan = true;
                    } else {
                        app.reset_for_rescan();
                        app.api_key = crate::tmdb::get_api_key(config);
                        start_disc_scan(&mut app);
                    }
                    continue;
                }

                // Handle rescan confirmation (during ripping)
                if app.confirm_rescan {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            app.reset_for_rescan();
                            app.api_key = crate::tmdb::get_api_key(config);
                            start_disc_scan(&mut app);
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            app.confirm_rescan = false;
                        }
                        _ => {}
                    }
                    continue;
                }

                match app.screen {
                    Screen::TmdbSearch => wizard::handle_tmdb_search_input(&mut app, key),
                    Screen::ShowSelect => wizard::handle_show_select_input(&mut app, key),
                    Screen::SeasonEpisode => wizard::handle_season_episode_input(&mut app, key),
                    Screen::EpisodeMapping => wizard::handle_episode_mapping_input(&mut app, key),
                    Screen::PlaylistSelect => wizard::handle_playlist_select_input(&mut app, key),
                    Screen::Confirm => wizard::handle_confirm_input(&mut app, key),
                    Screen::Ripping => dashboard::handle_input(&mut app, key),
                    Screen::Done => {
                        if key.code == KeyCode::Enter {
                            app.reset_for_rescan();
                            app.api_key = crate::tmdb::get_api_key(config);
                            start_disc_scan(&mut app);
                        } else {
                            // Eject on exit if enabled and all rips succeeded
                            let all_succeeded = app
                                .rip_jobs
                                .iter()
                                .all(|j| matches!(j.status, crate::types::PlaylistStatus::Done(_)));
                            if app.eject && !app.rip_jobs.is_empty() && all_succeeded {
                                let device = app.args.device().to_string_lossy().to_string();
                                let _ = crate::disc::eject_disc(&device);
                            }
                            app.quit = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Poll background tasks
        poll_background(&mut app);

        // If ripping, check for progress updates
        if app.screen == Screen::Ripping {
            dashboard::tick(&mut app)?;
        }
    }

    Ok(())
}

fn poll_background(app: &mut App) {
    let rx = match app.pending_rx {
        Some(ref rx) => rx,
        None => return,
    };

    let result = match rx.try_recv() {
        Ok(r) => r,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(mpsc::TryRecvError::Disconnected) => {
            app.pending_rx = None;
            app.status_message = "Background task failed unexpectedly".into();
            return;
        }
    };

    match result {
        BackgroundResult::WaitingForDisc(ref msg) => {
            // Append log entry for each new device tried
            let device_prefix = msg.split(" — ").next().unwrap_or("");
            if !app.scan_log.iter().any(|l| l.starts_with(device_prefix)) {
                app.scan_log.push(msg.clone());
            }
            app.status_message = "Waiting for disc...".into();
            return; // Keep pending_rx alive
        }
        BackgroundResult::DiscFound(ref device) => {
            app.status_message = format!("Scanning {}...", device);
            return; // Keep pending_rx alive
        }
        _ => {}
    }

    app.pending_rx = None;

    match result {
        BackgroundResult::WaitingForDisc(_) | BackgroundResult::DiscFound(_) => unreachable!(),
        BackgroundResult::DiscScan(Ok((device, label, playlists))) => {
            // Update device to the one that had the disc
            app.args.device = Some(std::path::PathBuf::from(device));
            app.label_info = crate::disc::parse_volume_label(&label);
            app.label = label;
            app.episodes_pl = crate::disc::filter_episodes(&playlists, app.args.min_duration)
                .into_iter()
                .cloned()
                .collect();
            app.playlists = playlists;

            app.movie_mode =
                app.args.movie || (app.episodes_pl.len() == 1 && app.args.season.is_none());

            if let Some(ref info) = app.label_info {
                app.search_query = info.show.clone();
                if !app.movie_mode {
                    app.season_num = Some(info.season);
                }
            }
            if let Some(s) = app.args.season {
                app.season_num = Some(s);
            }
            app.start_episode = app.args.start_episode;
            app.playlist_selected = vec![true; app.episodes_pl.len()];

            // Extract chapter counts from MPLS files
            let device_str = app.args.device().to_string_lossy().to_string();
            match crate::disc::ensure_mounted(&device_str) {
                Ok((mount, did_mount)) => {
                    let nums: Vec<&str> =
                        app.episodes_pl.iter().map(|pl| pl.num.as_str()).collect();
                    app.chapter_counts = crate::chapters::count_chapters_for_playlists(
                        std::path::Path::new(&mount),
                        &nums,
                    );
                    if did_mount {
                        let _ = crate::disc::unmount_disc(&device_str);
                    }
                }
                Err(_) => {
                    app.chapter_counts.clear();
                }
            }

            app.status_message.clear();

            if app.episodes_pl.is_empty() {
                app.status_message = "No episode-length playlists found.".into();
                app.screen = Screen::Done;
            } else {
                app.screen = Screen::TmdbSearch;
                app.input_active = true;
                app.input_buffer = if app.api_key.is_none() {
                    String::new()
                } else {
                    app.search_query.clone()
                };
            }
        }
        BackgroundResult::DiscScan(Err(e)) => {
            app.status_message = format!("Scan failed: {}", e);
            app.screen = Screen::Done;
        }
        BackgroundResult::ShowSearch(Ok(results)) => {
            if results.is_empty() {
                app.status_message = "No results found.".into();
            } else {
                app.search_results = results;
                app.list_cursor = 0;
                app.input_active = false;
                app.status_message.clear();
                app.screen = Screen::ShowSelect;
            }
        }
        BackgroundResult::ShowSearch(Err(e)) => {
            app.status_message = format!("TMDb search failed: {}", e);
        }
        BackgroundResult::MovieSearch(Ok(results)) => {
            if results.is_empty() {
                app.status_message = "No results found.".into();
            } else {
                app.movie_results = results;
                app.list_cursor = 0;
                app.input_active = false;
                app.status_message.clear();
                app.screen = Screen::ShowSelect;
            }
        }
        BackgroundResult::MovieSearch(Err(e)) => {
            app.status_message = format!("TMDb search failed: {}", e);
        }
        BackgroundResult::SeasonFetch(Ok(eps)) => {
            app.episodes = eps;
            app.status_message.clear();

            if !app.episodes.is_empty() {
                app.season_field = 1;
                let disc_num = app.label_info.as_ref().map(|l| l.disc);
                let guessed = crate::util::guess_start_episode(disc_num, app.episodes_pl.len());
                app.input_buffer = app.start_episode.unwrap_or(guessed).to_string();
            }
        }
        BackgroundResult::SeasonFetch(Err(e)) => {
            app.status_message = format!("Failed to fetch season: {}", e);
            app.episodes.clear();
        }
        BackgroundResult::MediaProbe(infos) => {
            let selected_indices: Vec<usize> = app
                .episodes_pl
                .iter()
                .enumerate()
                .filter(|(i, _)| app.playlist_selected.get(*i).copied().unwrap_or(false))
                .map(|(i, _)| i)
                .collect();

            let filenames: Vec<String> = infos
                .iter()
                .zip(selected_indices.iter())
                .map(|(info, &idx)| wizard::playlist_filename(app, idx, info.as_ref()))
                .collect();

            app.filenames = filenames;
            app.media_infos = infos;

            if app.filenames.is_empty() {
                app.status_message = "No playlists selected.".into();
            } else {
                app.list_cursor = 0;
                app.status_message.clear();
                app.screen = Screen::Confirm;
            }
        }
    }
}
