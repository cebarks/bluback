pub mod coordinator;
pub mod dashboard;
pub mod settings;
pub mod tab_bar;
pub mod wizard;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use std::collections::HashMap;
use std::io;
use std::sync::mpsc;
use std::time::Duration;

use crate::types::*;
use crate::Args;

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Scanning,
    TmdbSearch,      // merged: search input + inline results
    Season,          // simplified: just season number (was SeasonEpisode)
    PlaylistManager, // merged: playlist select + episode mapping
    Confirm,
    Ripping,
    Done,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum InputFocus {
    #[default]
    TextInput,
    List,
    /// Episode assignment editing (visible row index)
    InlineEdit(usize),
    /// Track selection editing (sub-row index within expanded tracks)
    TrackEdit(usize),
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
    pub clip_sizes: HashMap<String, u64>,
}

#[derive(Default)]
pub struct TmdbState {
    #[allow(dead_code)]
    // Legacy field from single-session App; sessions use DriveSession.tmdb_api_key
    pub api_key: Option<String>,
    pub search_query: String,
    pub movie_mode: bool,
    pub search_results: Vec<TmdbShow>,
    pub movie_results: Vec<TmdbMovie>,
    pub selected_show: Option<usize>,
    pub selected_movie: Option<usize>,
    pub show_name: String,
    pub episodes: Vec<Episode>,
    pub specials: Vec<Episode>,
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
    pub media_infos: std::collections::HashMap<String, MediaInfo>,
    pub stream_infos: std::collections::HashMap<String, StreamInfo>,
    pub track_selections: std::collections::HashMap<String, Vec<usize>>,
    pub expanded_playlist: Option<usize>,
    pub detection_results: Vec<crate::detection::DetectionResult>,
}

pub struct RipState {
    pub jobs: Vec<RipJob>,
    pub current_rip: usize,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub progress_rx:
        Option<mpsc::Receiver<Result<crate::types::RipProgress, crate::media::MediaError>>>,
    pub confirm_abort: bool,
    pub confirm_rescan: bool,
    pub chapters_added: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub verify_failed_idx: Option<usize>,
}

impl Default for RipState {
    fn default() -> Self {
        Self {
            jobs: Vec::new(),
            current_rip: 0,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            progress_rx: None,
            confirm_abort: false,
            confirm_rescan: false,
            chapters_added: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            verify_failed_idx: None,
        }
    }
}

#[allow(dead_code)] // Used by run_settings; most fields only needed for the old single-session run_app
pub struct App {
    pub screen: Screen,
    pub args: Args,
    pub config: crate::config::Config,
    pub quit: bool,
    pub eject: bool,
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

pub fn run(
    args: &Args,
    config: &crate::config::Config,
    config_path: std::path::PathBuf,
    stream_filter: &crate::streams::StreamFilter,
    history_db_path: Option<std::path::PathBuf>,
) -> Result<()> {
    enable_raw_mode()?;
    // Save terminal title, enter alternate screen
    io::stdout().execute(crossterm::terminal::SetTitle("bluback"))?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(
        &mut terminal,
        args,
        config,
        config_path,
        stream_filter,
        history_db_path,
    );

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    // Restore terminal title (empty resets to terminal default)
    io::stdout().execute(SetTitle(""))?;

    result
}

pub fn run_settings(config: &crate::config::Config, config_path: std::path::PathBuf) -> Result<()> {
    use clap::Parser;

    enable_raw_mode()?;
    io::stdout().execute(SetTitle("bluback — Settings"))?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let default_args = Args::parse_from(["bluback"]);
    let mut app = App::new(default_args);
    app.config = config.clone();
    app.config_path = config_path;
    let drives: Vec<String> = crate::disc::detect_optical_drives()
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let mut state = crate::types::SettingsState::from_config_with_drives(config, &drives);
    state.standalone = true;
    state.apply_env_overrides();
    app.overlay = Some(crate::types::Overlay::Settings(state));

    loop {
        terminal.draw(|f| {
            let block = ratatui::widgets::Block::default()
                .title("bluback")
                .borders(ratatui::widgets::Borders::ALL);
            f.render_widget(block, f.area());
            if let Some(crate::types::Overlay::Settings(ref state)) = app.overlay {
                settings::render(f, state);
            }
        })?;

        if app.quit || app.overlay.is_none() {
            break;
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                if let Some(crate::types::Overlay::Settings(ref mut state)) = app.overlay {
                    if state.save_message.is_some() {
                        state.save_message = None;
                        state.save_message_at = None;
                    }
                    match settings::handle_input(state, key) {
                        settings::SettingsAction::Save => {
                            let new_config = state.to_config();
                            match new_config.save(&app.config_path) {
                                Ok(()) => {
                                    let warnings = state.active_env_var_warnings();
                                    let msg = if warnings.is_empty() {
                                        "Saved!".to_string()
                                    } else {
                                        format!(
                                            "Saved! (env vars override: {})",
                                            warnings.join(", ")
                                        )
                                    };
                                    state.save_message = Some(msg);
                                    state.save_message_at = Some(std::time::Instant::now());
                                    state.dirty = false;
                                }
                                Err(e) => {
                                    state.save_message = Some(format!("Error: {}", e));
                                    state.save_message_at = Some(std::time::Instant::now());
                                }
                            }
                        }
                        settings::SettingsAction::SaveAndClose => {
                            let new_config = state.to_config();
                            let _ = new_config.save(&app.config_path);
                            app.overlay = None;
                        }
                        settings::SettingsAction::Close => {
                            app.overlay = None;
                        }
                        settings::SettingsAction::None => {}
                    }
                }
            }
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
    }

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    io::stdout().execute(SetTitle(""))?;
    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    args: &Args,
    config: &crate::config::Config,
    config_path: std::path::PathBuf,
    stream_filter: &crate::streams::StreamFilter,
    history_db_path: Option<std::path::PathBuf>,
) -> Result<()> {
    let mut coord = coordinator::Coordinator::new(
        args.clone(),
        config.clone(),
        config_path,
        stream_filter.clone(),
    );
    coord.history_db_path = history_db_path;
    coord.run(terminal)
}
