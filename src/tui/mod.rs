pub mod dashboard;
pub mod wizard;
pub mod settings;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
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
    TmdbSearch,       // merged: search input + inline results
    Season,           // simplified: just season number (was SeasonEpisode)
    PlaylistManager,  // merged: playlist select + episode mapping
    Confirm,
    Ripping,
    Done,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum InputFocus {
    #[default]
    TextInput,
    List,
    InlineEdit(usize),
}

#[derive(Default)]
pub struct DiscState {
    pub label: String,
    pub label_info: Option<LabelInfo>,
    pub playlists: Vec<Playlist>,
    pub episodes_pl: Vec<Playlist>,
    pub scan_log: Vec<String>,
    pub mount_point: Option<String>,
    pub did_mount: bool,
    pub chapter_counts: HashMap<String, usize>,
}

#[derive(Default)]
pub struct TmdbState {
    pub api_key: Option<String>,
    pub search_query: String,
    pub movie_mode: bool,
    pub search_results: Vec<TmdbShow>,
    pub movie_results: Vec<TmdbMovie>,
    pub selected_show: Option<usize>,
    pub selected_movie: Option<usize>,
    pub show_name: String,
    pub episodes: Vec<Episode>,
}

#[derive(Default)]
pub struct WizardState {
    pub list_cursor: usize,
    pub input_buffer: String,
    pub input_focus: InputFocus,
    pub season_num: Option<u32>,
    pub start_episode: Option<u32>,
    pub episode_assignments: EpisodeAssignments,
    pub playlist_selected: Vec<bool>,
    pub specials: std::collections::HashSet<String>,
    pub show_filtered: bool,
    pub filenames: Vec<String>,
    pub media_infos: Vec<Option<MediaInfo>>,
}

#[derive(Default)]
pub struct RipState {
    pub jobs: Vec<RipJob>,
    pub current_rip: usize,
    pub child: Option<std::process::Child>,
    pub progress_rx: Option<mpsc::Receiver<String>>,
    pub progress_state: HashMap<String, String>,
    pub stderr_buffer: Option<Arc<Mutex<String>>>,
    pub confirm_abort: bool,
    pub confirm_rescan: bool,
}

pub struct App {
    pub screen: Screen,
    pub args: Args,
    pub config: crate::config::Config,
    pub quit: bool,
    pub eject: bool,
    pub has_mkvpropedit: bool,
    pub status_message: String,
    pub spinner_frame: usize,

    pub disc: DiscState,
    pub tmdb: TmdbState,
    pub wizard: WizardState,
    pub rip: RipState,

    pub pending_rx: Option<mpsc::Receiver<BackgroundResult>>,
    pub disc_detected_label: Option<String>,
    pub overlay: Option<crate::types::Overlay>,
    pub config_path: std::path::PathBuf,
}

impl App {
    pub fn new(args: Args) -> Self {
        Self {
            screen: Screen::Scanning,
            args,
            quit: false,
            config: crate::config::Config::default(),
            eject: false,
            has_mkvpropedit: false,
            status_message: String::new(),
            spinner_frame: 0,
            disc: DiscState::default(),
            tmdb: TmdbState::default(),
            wizard: WizardState::default(),
            rip: RipState::default(),
            pending_rx: None,
            disc_detected_label: None,
            overlay: None,
            config_path: std::path::PathBuf::new(),
        }
    }
}

pub(crate) fn start_disc_scan(app: &mut App) {
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
    pub fn open_settings(&mut self) {
        let state = crate::types::SettingsState::from_config(&self.config);
        self.overlay = Some(crate::types::Overlay::Settings(state));
    }

    pub fn reset_for_rescan(&mut self) {
        // Kill any active rip
        if let Some(ref mut child) = self.rip.child {
            let _ = child.kill();
            let _ = child.wait();
        }

        if self.disc.did_mount {
            let _ = crate::disc::unmount_disc(&self.args.device().to_string_lossy());
        }

        self.disc = DiscState::default();

        // Reset tmdb state but keep api_key
        self.tmdb.search_query = String::new();
        self.tmdb.movie_mode = false;
        self.tmdb.search_results = Vec::new();
        self.tmdb.movie_results = Vec::new();
        self.tmdb.selected_show = None;
        self.tmdb.selected_movie = None;
        self.tmdb.show_name = String::new();
        self.tmdb.episodes = Vec::new();
        // Keep tmdb.api_key

        self.wizard = WizardState::default();
        self.rip = RipState::default();
        self.status_message = String::new();
        self.spinner_frame = 0;
        self.pending_rx = None;
        self.disc_detected_label = None;
    }
}

fn terminal_title(app: &App) -> String {
    match app.screen {
        Screen::Scanning => {
            if app.disc.label.is_empty() {
                "bluback — Scanning...".into()
            } else {
                format!("bluback — Scanning {}", app.disc.label)
            }
        }
        Screen::TmdbSearch | Screen::Season | Screen::PlaylistManager | Screen::Confirm => {
            if app.disc.label.is_empty() {
                "bluback".into()
            } else {
                format!("bluback — {}", app.disc.label)
            }
        }
        Screen::Ripping => {
            let done = app.rip.jobs.iter().filter(|j| matches!(j.status, crate::types::PlaylistStatus::Done(_))).count();
            let total = app.rip.jobs.len();
            if let Some(job) = app.rip.jobs.get(app.rip.current_rip) {
                if let crate::types::PlaylistStatus::Ripping(ref prog) = job.status {
                    let pct = if job.playlist.seconds > 0 {
                        (prog.out_time_secs as f64 / job.playlist.seconds as f64 * 100.0).min(100.0) as u32
                    } else {
                        0
                    };
                    return format!("bluback — Ripping {}/{} ({}%)", done + 1, total, pct);
                }
            }
            format!("bluback — Ripping {}/{}", done, total)
        }
        Screen::Done => "bluback — Done".into(),
    }
}

pub fn run(args: &Args, config: &crate::config::Config, config_path: std::path::PathBuf) -> Result<()> {
    enable_raw_mode()?;
    // Save terminal title, enter alternate screen
    io::stdout().execute(crossterm::terminal::SetTitle("bluback"))?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, args, config, config_path);

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    // Restore terminal title (empty resets to terminal default)
    io::stdout().execute(SetTitle(""))?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    args: &Args,
    config: &crate::config::Config,
    config_path: std::path::PathBuf,
) -> Result<()> {
    let mut app = App::new(args.clone());
    app.config = config.clone();
    app.config_path = config_path;
    app.eject = config.should_eject(args.cli_eject());
    app.has_mkvpropedit = crate::disc::has_mkvpropedit();

    // Spawn disc scan in background thread
    app.tmdb.api_key = crate::tmdb::get_api_key(config);
    start_disc_scan(&mut app);

    // Event loop
    loop {
        terminal.draw(|f| {
            match app.screen {
                Screen::Scanning => wizard::render_scanning(f, &app),
                Screen::TmdbSearch => wizard::render_tmdb_search(f, &app),
                Screen::Season => wizard::render_season(f, &app),
                Screen::PlaylistManager => wizard::render_playlist_manager(f, &app),
                Screen::Confirm => wizard::render_confirm(f, &app),
                Screen::Ripping => dashboard::render(f, &app),
                Screen::Done => dashboard::render_done(f, &app),
            }
            if let Some(crate::types::Overlay::Settings(ref state)) = app.overlay {
                settings::render(f, state);
            }
        })?;

        // Update terminal title with current status
        let _ = io::stdout().execute(SetTitle(terminal_title(&app)));

        if app.quit {
            if let Some(ref mut child) = app.rip.child {
                let _ = child.kill();
                let _ = child.wait();
            }
            break;
        }

        // Poll for events
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let input_active = matches!(
                    app.wizard.input_focus,
                    InputFocus::TextInput | InputFocus::InlineEdit(_)
                );

                // Ctrl+C: always quit immediately
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.quit = true;
                    continue;
                }

                // Route ALL input to overlay when active (blocks q, Ctrl+E, Ctrl+R, Ctrl+S)
                if app.overlay.is_some() {
                    let action = {
                        let state = match app.overlay {
                            Some(crate::types::Overlay::Settings(ref mut s)) => s,
                            _ => unreachable!(),
                        };
                        if state.save_message.is_some() {
                            state.save_message = None;
                            state.save_message_at = None;
                        }
                        settings::handle_input(state, key)
                    };
                    match action {
                        settings::SettingsAction::Save => {
                            handle_settings_save(&mut app, config);
                        }
                        settings::SettingsAction::SaveAndClose => {
                            handle_settings_save(&mut app, config);
                            app.overlay = None;
                        }
                        settings::SettingsAction::Close => {
                            app.overlay = None;
                        }
                        settings::SettingsAction::None => {
                            apply_settings_to_session(&mut app);
                        }
                    }
                    continue;
                }

                // Global quit (not during ripping -- dashboard handles its own q)
                if key.code == KeyCode::Char('q')
                    && !input_active
                    && app.screen != Screen::Ripping
                {
                    app.quit = true;
                    continue;
                }

                // Global Ctrl+E: eject disc (not during ripping or text input)
                if key.code == KeyCode::Char('e')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !input_active
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
                    && !app.rip.confirm_rescan
                {
                    if app.screen == Screen::Ripping {
                        app.rip.confirm_rescan = true;
                    } else {
                        app.reset_for_rescan();
                        app.tmdb.api_key = crate::tmdb::get_api_key(config);
                        start_disc_scan(&mut app);
                    }
                    continue;
                }

                // Global Ctrl+S: open settings (not during text input or rip confirmations)
                if key.code == KeyCode::Char('s')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !input_active
                    && !app.rip.confirm_abort
                    && !app.rip.confirm_rescan
                {
                    app.open_settings();
                    continue;
                }

                // Handle rescan confirmation (during ripping)
                if app.rip.confirm_rescan {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            app.reset_for_rescan();
                            app.tmdb.api_key = crate::tmdb::get_api_key(config);
                            start_disc_scan(&mut app);
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            app.rip.confirm_rescan = false;
                        }
                        _ => {}
                    }
                    continue;
                }

                match app.screen {
                    Screen::TmdbSearch => wizard::handle_tmdb_search_input(&mut app, key),
                    Screen::Season => wizard::handle_season_input(&mut app, key),
                    Screen::PlaylistManager => wizard::handle_playlist_manager_input(&mut app, key),
                    Screen::Confirm => wizard::handle_confirm_input(&mut app, key),
                    Screen::Ripping => dashboard::handle_input(&mut app, key),
                    Screen::Done => {
                        if app.disc_detected_label.is_some() {
                            if key.code == KeyCode::Enter {
                                app.disc_detected_label = None;
                                app.reset_for_rescan();
                                app.tmdb.api_key = crate::tmdb::get_api_key(config);
                                start_disc_scan(&mut app);
                            } else {
                                let all_succeeded = app.rip.jobs.iter().all(|j| {
                                    matches!(j.status, crate::types::PlaylistStatus::Done(_))
                                });
                                if app.eject && !app.rip.jobs.is_empty() && all_succeeded {
                                    let device = app.args.device().to_string_lossy().to_string();
                                    let _ = crate::disc::eject_disc(&device);
                                }
                                app.quit = true;
                            }
                        } else if key.code == KeyCode::Enter {
                            app.reset_for_rescan();
                            app.tmdb.api_key = crate::tmdb::get_api_key(config);
                            start_disc_scan(&mut app);
                        } else {
                            let all_succeeded = app
                                .rip
                                .jobs
                                .iter()
                                .all(|j| matches!(j.status, crate::types::PlaylistStatus::Done(_)));
                            if app.eject && !app.rip.jobs.is_empty() && all_succeeded {
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

        // Increment spinner frame when background task is active
        if app.pending_rx.is_some() {
            app.spinner_frame = app.spinner_frame.wrapping_add(1);
        }

        // Clear save message after 2 seconds
        if let Some(crate::types::Overlay::Settings(ref mut state)) = app.overlay {
            if let Some(at) = state.save_message_at {
                if at.elapsed() > Duration::from_secs(2) {
                    state.save_message = None;
                    state.save_message_at = None;
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
            if !app.disc.scan_log.iter().any(|l| l.starts_with(device_prefix)) {
                app.disc.scan_log.push(msg.clone());
            }
            app.status_message = "Waiting for disc...".into();
            return; // Keep pending_rx alive
        }
        BackgroundResult::DiscFound(ref device) => {
            if app.screen == Screen::Done {
                let label = crate::disc::get_volume_label(device);
                app.disc_detected_label = Some(if label.is_empty() {
                    device.clone()
                } else {
                    label
                });
                return; // Keep polling alive for potential full scan later
            }
            app.disc.scan_log.clear();
            app.status_message = format!("Scanning {}...", device);
            return; // Keep pending_rx alive
        }
        _ => {}
    }

    app.pending_rx = None;

    match result {
        BackgroundResult::WaitingForDisc(_) | BackgroundResult::DiscFound(_) => unreachable!(),
        BackgroundResult::DiscScan(Ok(_)) | BackgroundResult::DiscScan(Err(_))
            if app.screen == Screen::Done =>
        {
            // Ignore full scan results on Done screen — we only wanted disc detection
        }
        BackgroundResult::DiscScan(Ok((device, label, playlists))) => {
            // Update device to the one that had the disc
            app.args.device = Some(std::path::PathBuf::from(device));
            app.disc.label_info = crate::disc::parse_volume_label(&label);
            app.disc.label = label;
            let min_dur = app.config.min_duration(app.args.min_duration);
            app.disc.episodes_pl = crate::disc::filter_episodes(&playlists, min_dur)
                .into_iter()
                .cloned()
                .collect();
            app.disc.playlists = playlists;

            app.tmdb.movie_mode =
                app.args.movie || (app.disc.episodes_pl.len() == 1 && app.args.season.is_none());

            if let Some(ref info) = app.disc.label_info {
                app.tmdb.search_query = info.show.clone();
                if !app.tmdb.movie_mode {
                    app.wizard.season_num = Some(info.season);
                }
            }
            if let Some(s) = app.args.season {
                app.wizard.season_num = Some(s);
            }
            app.wizard.start_episode = app.args.start_episode;
            app.wizard.show_filtered = app.config.show_filtered();
            app.wizard.playlist_selected = app.disc.playlists.iter().map(|pl| {
                app.disc.episodes_pl.iter().any(|ep| ep.num == pl.num)
            }).collect();

            // Extract chapter counts from MPLS files
            let device_str = app.args.device().to_string_lossy().to_string();
            match crate::disc::ensure_mounted(&device_str) {
                Ok((mount, did_mount)) => {
                    let nums: Vec<&str> =
                        app.disc.playlists.iter().map(|pl| pl.num.as_str()).collect();
                    app.disc.chapter_counts = crate::chapters::count_chapters_for_playlists(
                        std::path::Path::new(&mount),
                        &nums,
                    );
                    if did_mount {
                        let _ = crate::disc::unmount_disc(&device_str);
                    }
                }
                Err(_) => {
                    app.disc.chapter_counts.clear();
                }
            }

            app.status_message.clear();

            if app.disc.episodes_pl.is_empty() {
                app.status_message = "No episode-length playlists found.".into();
                app.screen = Screen::Done;
            } else {
                app.screen = Screen::TmdbSearch;
                app.wizard.input_focus = InputFocus::TextInput;
                app.wizard.input_buffer = if app.tmdb.api_key.is_none() {
                    String::new()
                } else {
                    app.tmdb.search_query.clone()
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
                app.tmdb.search_results = results;
                app.wizard.list_cursor = 0;
                app.wizard.input_focus = InputFocus::List;
                app.status_message.clear();
            }
        }
        BackgroundResult::ShowSearch(Err(e)) => {
            app.status_message = format!("TMDb search failed: {}", e);
        }
        BackgroundResult::MovieSearch(Ok(results)) => {
            if results.is_empty() {
                app.status_message = "No results found.".into();
            } else {
                app.tmdb.movie_results = results;
                app.wizard.list_cursor = 0;
                app.wizard.input_focus = InputFocus::List;
                app.status_message.clear();
            }
        }
        BackgroundResult::MovieSearch(Err(e)) => {
            app.status_message = format!("TMDb search failed: {}", e);
        }
        BackgroundResult::SeasonFetch(Ok(eps)) => {
            app.tmdb.episodes = eps;
            app.wizard.list_cursor = 0;
            app.status_message.clear();
        }
        BackgroundResult::SeasonFetch(Err(e)) => {
            app.status_message = format!("Failed to fetch season: {}", e);
            app.tmdb.episodes.clear();
        }
        BackgroundResult::MediaProbe(infos) => {
            let selected_indices: Vec<usize> = app
                .disc
                .playlists
                .iter()
                .enumerate()
                .filter(|(i, _)| app.wizard.playlist_selected.get(*i).copied().unwrap_or(false))
                .map(|(i, _)| i)
                .collect();

            let filenames: Vec<String> = infos
                .iter()
                .zip(selected_indices.iter())
                .map(|(info, &idx)| wizard::playlist_filename(app, idx, info.as_ref()))
                .collect();

            app.wizard.filenames = filenames;
            app.wizard.media_infos = infos;

            if app.wizard.filenames.is_empty() {
                app.status_message = "No playlists selected.".into();
            } else {
                app.wizard.list_cursor = 0;
                app.status_message.clear();
                app.screen = Screen::Confirm;
            }
        }
    }
}

fn handle_settings_save(app: &mut App, config: &crate::config::Config) {
    let new_config = match app.overlay {
        Some(crate::types::Overlay::Settings(ref state)) => state.to_config(),
        _ => return,
    };
    match new_config.save(&app.config_path) {
        Ok(()) => {
            app.config = new_config;
            app.eject = app.config.should_eject(app.args.cli_eject());
            if let Some(ref dir) = app.config.output_dir {
                app.args.output = std::path::PathBuf::from(dir);
            }
            if let Some(ref dev) = app.config.device {
                if dev != crate::config::DEFAULT_DEVICE {
                    app.args.device = Some(std::path::PathBuf::from(dev));
                }
            }
            if let Some(crate::types::Overlay::Settings(ref mut state)) = app.overlay {
                state.save_message = Some("Saved!".into());
                state.save_message_at = Some(std::time::Instant::now());
                state.dirty = false;
            }
            // Reset workflow if not ripping
            if app.screen != Screen::Ripping && app.screen != Screen::Done {
                app.reset_for_rescan();
                app.tmdb.api_key = crate::tmdb::get_api_key(config);
                start_disc_scan(app);
            }
        }
        Err(e) => {
            if let Some(crate::types::Overlay::Settings(ref mut state)) = app.overlay {
                state.save_message = Some(format!("Error: {}", e));
                state.save_message_at = Some(std::time::Instant::now());
            }
        }
    }
}

fn apply_settings_to_session(app: &mut App) {
    let new_config = match app.overlay {
        Some(crate::types::Overlay::Settings(ref state)) => state.to_config(),
        _ => return,
    };
    app.config = new_config;
    app.eject = app.config.should_eject(app.args.cli_eject());
}
