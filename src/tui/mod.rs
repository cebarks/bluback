pub mod dashboard;
pub mod wizard;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
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
    pub list_cursor: usize,

    // Text input
    pub input_buffer: String,
    pub input_active: bool,
    /// 0 = editing season, 1 = editing start episode
    pub season_field: u8,

    // Rip state
    pub rip_jobs: Vec<RipJob>,
    pub current_rip: usize,
    pub rip_child: Option<std::process::Child>,
    pub progress_rx: Option<mpsc::Receiver<String>>,
    pub progress_state: HashMap<String, String>,
    pub stderr_buffer: Option<Arc<Mutex<String>>>,
    pub confirm_abort: bool,

    // Status/error messages
    pub status_message: String,
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
            list_cursor: 0,
            input_buffer: String::new(),
            input_active: false,
            season_field: 0,
            rip_jobs: Vec::new(),
            current_rip: 0,
            rip_child: None,
            progress_rx: None,
            progress_state: HashMap::new(),
            stderr_buffer: None,
            confirm_abort: false,
            status_message: String::new(),
        }
    }
}

pub fn run(args: &Args) -> Result<()> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, args);

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, args: &Args) -> Result<()> {
    let mut app = App::new(args.clone());

    // Initial scan (blocking -- runs before event loop)
    app.status_message = format!("Scanning disc at {}...", args.device.display());
    terminal.draw(|f| wizard::render_scanning(f, &app))?;

    // Perform scan
    let device = args.device.to_string_lossy().to_string();
    app.label = crate::disc::get_volume_label(&device);
    app.label_info = crate::disc::parse_volume_label(&app.label);
    app.playlists = crate::disc::scan_playlists(&device)?;
    app.episodes_pl = crate::disc::filter_episodes(&app.playlists, args.min_duration)
        .into_iter().cloned().collect();
    app.api_key = crate::tmdb::get_api_key();
    app.status_message.clear();

    // Set movie mode from flag or auto-detect (single playlist)
    app.movie_mode = args.movie || (app.episodes_pl.len() == 1 && args.season.is_none());

    // Pre-fill from label/args
    if let Some(ref info) = app.label_info {
        app.search_query = info.show.clone();
        if !app.movie_mode {
            app.season_num = Some(info.season);
        }
    }
    if let Some(s) = args.season {
        app.season_num = Some(s);
    }
    app.start_episode = args.start_episode;

    // Initialize playlist selection (all selected)
    app.playlist_selected = vec![true; app.episodes_pl.len()];

    if app.episodes_pl.is_empty() {
        app.status_message = "No episode-length playlists found.".into();
        app.screen = Screen::Done;
    } else {
        app.screen = Screen::TmdbSearch;
        app.input_active = true;
        app.input_buffer = if app.api_key.is_none() {
            String::new() // Will prompt for API key first
        } else {
            app.search_query.clone()
        };
    }

    // Event loop
    loop {
        terminal.draw(|f| {
            match app.screen {
                Screen::Scanning => wizard::render_scanning(f, &app),
                Screen::TmdbSearch => wizard::render_tmdb_search(f, &app),
                Screen::ShowSelect => wizard::render_show_select(f, &app),
                Screen::SeasonEpisode => wizard::render_season_episode(f, &app),
                Screen::PlaylistSelect => wizard::render_playlist_select(f, &app),
                Screen::Confirm => wizard::render_confirm(f, &app),
                Screen::Ripping => dashboard::render(f, &app),
                Screen::Done => dashboard::render_done(f, &app),
            }
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

                match app.screen {
                    Screen::TmdbSearch => wizard::handle_tmdb_search_input(&mut app, key),
                    Screen::ShowSelect => wizard::handle_show_select_input(&mut app, key),
                    Screen::SeasonEpisode => wizard::handle_season_episode_input(&mut app, key),
                    Screen::PlaylistSelect => wizard::handle_playlist_select_input(&mut app, key),
                    Screen::Confirm => wizard::handle_confirm_input(&mut app, key),
                    Screen::Ripping => dashboard::handle_input(&mut app, key),
                    Screen::Done => {
                        app.quit = true;
                    }
                    _ => {}
                }
            }
        }

        // If ripping, check for progress updates
        if app.screen == Screen::Ripping {
            dashboard::tick(&mut app)?;
        }
    }

    Ok(())
}
