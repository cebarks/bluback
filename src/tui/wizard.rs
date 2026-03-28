use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table};
use std::sync::mpsc;

use super::{App, InputFocus, Screen};
use crate::tmdb;
use crate::types::{
    BackgroundResult, ConfirmView, PlaylistView, ScanningView, SeasonView, TmdbView,
};
use crate::util::{assign_episodes, guess_start_episode, parse_episode_input};

const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn spinner_char(frame: usize) -> char {
    SPINNER_CHARS[frame % SPINNER_CHARS.len()]
}

pub fn playlist_filename(
    app: &App,
    playlist_index: usize,
    media_info: Option<&crate::types::MediaInfo>,
) -> String {
    let pl = &app.disc.playlists[playlist_index];
    let is_special = app.wizard.specials.contains(&pl.num);

    let show_name = if !app.tmdb.show_name.is_empty() {
        app.tmdb.show_name.clone()
    } else {
        app.disc
            .label_info
            .as_ref()
            .map(|l| l.show.clone())
            .unwrap_or_else(|| "Unknown".to_string())
    };

    let movie_title = if app.tmdb.movie_mode {
        let movie = app
            .tmdb
            .selected_movie
            .and_then(|i| app.tmdb.movie_results.get(i));
        let title = movie.map(|m| m.title.as_str()).unwrap_or("movie");
        let year = movie
            .and_then(|m| m.release_date.as_deref())
            .and_then(|d| d.get(..4))
            .unwrap_or("");
        Some((title.to_string(), year.to_string()))
    } else {
        None
    };

    let part = if app.tmdb.movie_mode {
        let selected_count = app.wizard.playlist_selected.iter().filter(|&&s| s).count();
        if selected_count > 1 {
            app.disc
                .playlists
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    app.wizard
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

    let episodes = app
        .wizard
        .episode_assignments
        .get(&pl.num)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);

    crate::workflow::build_output_filename(
        pl,
        episodes,
        app.wizard.season_num.unwrap_or(0),
        app.tmdb.movie_mode,
        is_special,
        movie_title.as_ref().map(|(t, y)| (t.as_str(), y.as_str())),
        &show_name,
        &app.disc.label,
        app.disc.label_info.as_ref(),
        &app.config,
        app.args.format.as_deref(),
        app.args.format_preset.as_deref(),
        media_info,
        part,
    )
}

fn standard_layout(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area)
}

fn label_text(label: &str) -> String {
    if label.is_empty() {
        String::new()
    } else {
        format!("Disc: {}", label)
    }
}

fn format_status(status_message: &str, pending: bool, spinner_frame: usize) -> String {
    if status_message.is_empty() {
        return String::new();
    }
    if pending {
        format!("{} {}", spinner_char(spinner_frame), status_message)
    } else {
        status_message.to_string()
    }
}

pub fn render_scanning_view(
    f: &mut Frame,
    view: &ScanningView,
    status: &str,
    _spinner: usize,
    area: Rect,
) {
    let chunks = standard_layout(area);

    let header_text = label_text(&view.label);
    let title = Paragraph::new(header_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Blu-ray Backup"),
    );
    f.render_widget(title, chunks[0]);

    let mut lines: Vec<Line> = view
        .scan_log
        .iter()
        .map(|s| Line::from(s.as_str()).style(Style::default().fg(Color::DarkGray)))
        .collect();
    if !status.is_empty() {
        lines.push(Line::from(status.to_string()));
    }
    let body =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Scanning"));
    f.render_widget(body, chunks[1]);

    let hints = Paragraph::new("q: Quit | Ctrl+E: Eject | Ctrl+R: Rescan | Ctrl+S: Settings")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn render_scanning(f: &mut Frame, app: &App) {
    let view = ScanningView {
        label: app.disc.label.clone(),
        scan_log: app.disc.scan_log.clone(),
    };
    let status = format_status(
        &app.status_message,
        app.pending_rx.is_some(),
        app.spinner_frame,
    );
    render_scanning_view(f, &view, &status, app.spinner_frame, f.area());
}

pub fn render_tmdb_search_view(
    f: &mut Frame,
    view: &TmdbView,
    status: &str,
    _spinner: usize,
    area: Rect,
) {
    let chunks = standard_layout(area);

    let mode_label = if view.movie_mode { "Movie" } else { "TV Show" };
    let step_title = format!("Step 1: TMDb Search ({})", mode_label);
    let disc_text = label_text(&view.label);
    let header_text = if disc_text.is_empty() {
        format!("{} playlists", view.episodes_pl_count)
    } else {
        format!("{}  |  {} playlists", disc_text, view.episodes_pl_count)
    };
    let title =
        Paragraph::new(header_text).block(Block::default().borders(Borders::ALL).title(step_title));
    f.render_widget(title, chunks[0]);

    if !view.has_api_key {
        // API key input mode
        let content_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(chunks[1]);

        let msg = Paragraph::new("No TMDb API key found. Enter your key to enable episode naming:");
        f.render_widget(msg, content_chunks[0]);

        let input = Paragraph::new(format!("{}|", view.input_buffer))
            .block(Block::default().borders(Borders::ALL).title("TMDb API Key"));
        f.render_widget(input, content_chunks[1]);

        let hints = Paragraph::new(
            "Enter: Save key | Esc: Skip TMDb | Ctrl+E: Eject | Ctrl+R: Rescan | Ctrl+S: Settings",
        )
        .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hints, chunks[2]);
    } else {
        // Search input + inline results
        let has_results = if view.movie_mode {
            !view.movie_results.is_empty()
        } else {
            !view.show_results.is_empty()
        };

        let content_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(chunks[1]);

        // Search input field
        let input_style = if view.input_focus == InputFocus::TextInput {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let cursor = if view.input_focus == InputFocus::TextInput {
            "|"
        } else {
            ""
        };
        let mut lines = vec![Line::from(format!("{}{}", view.input_buffer, cursor))];
        if !status.is_empty() {
            lines.push(Line::from(status.to_string()).style(Style::default().fg(Color::Yellow)));
        }
        let input = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Search query")
                .border_style(input_style),
        );
        f.render_widget(input, content_chunks[0]);

        // Results list (when results exist)
        if has_results {
            let items: Vec<ListItem> = if view.movie_mode {
                view.movie_results
                    .iter()
                    .enumerate()
                    .map(|(i, movie)| {
                        let year = movie
                            .release_date
                            .as_deref()
                            .unwrap_or("")
                            .get(..4)
                            .unwrap_or("");
                        let marker = if view.input_focus == InputFocus::List
                            && i == view.list_cursor
                        {
                            "> "
                        } else {
                            "  "
                        };
                        ListItem::new(format!("{}{} ({})", marker, movie.title, year))
                    })
                    .collect()
            } else {
                view.show_results
                    .iter()
                    .enumerate()
                    .map(|(i, show)| {
                        let year = show
                            .first_air_date
                            .as_deref()
                            .unwrap_or("")
                            .get(..4)
                            .unwrap_or("");
                        let marker = if view.input_focus == InputFocus::List
                            && i == view.list_cursor
                        {
                            "> "
                        } else {
                            "  "
                        };
                        ListItem::new(format!("{}{} ({})", marker, show.name, year))
                    })
                    .collect()
            };

            let list_style = if view.input_focus == InputFocus::List {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Results")
                        .border_style(list_style),
                )
                .highlight_style(Style::default().fg(Color::Yellow));
            f.render_widget(list, content_chunks[1]);
        }

        let toggle = if view.movie_mode {
            "TV Show"
        } else {
            "Movie"
        };
        let hints_text = if view.input_focus == InputFocus::List {
            "Up/Down: Navigate | Enter: Select | Esc: Back to search | Ctrl+R: Rescan | Ctrl+S: Settings".to_string()
        } else {
            format!(
                "Enter: Search | Down: Results | Tab: Switch to {} | Esc: Skip TMDb | Ctrl+R: Rescan | Ctrl+S: Settings",
                toggle
            )
        };
        let hints = Paragraph::new(hints_text).style(Style::default().fg(Color::DarkGray));
        f.render_widget(hints, chunks[2]);
    }
}

pub fn render_tmdb_search(f: &mut Frame, app: &App) {
    let view = TmdbView {
        has_api_key: app.tmdb.api_key.is_some(),
        movie_mode: app.tmdb.movie_mode,
        search_query: app.tmdb.search_query.clone(),
        input_buffer: app.wizard.input_buffer.clone(),
        input_focus: app.wizard.input_focus.clone(),
        show_results: app.tmdb.search_results.clone(),
        movie_results: app.tmdb.movie_results.clone(),
        list_cursor: app.wizard.list_cursor,
        show_name: app.tmdb.show_name.clone(),
        label: app.disc.label.clone(),
        episodes_pl_count: app.disc.episodes_pl.len(),
    };
    let status = format_status(
        &app.status_message,
        app.pending_rx.is_some(),
        app.spinner_frame,
    );
    render_tmdb_search_view(f, &view, &status, app.spinner_frame, f.area());
}

pub fn handle_tmdb_search_input(app: &mut App, key: KeyEvent) {
    match app.wizard.input_focus {
        InputFocus::TextInput => {
            match key.code {
                KeyCode::Char(c) => app.wizard.input_buffer.push(c),
                KeyCode::Backspace => {
                    app.wizard.input_buffer.pop();
                }
                KeyCode::Enter => {
                    let input = app.wizard.input_buffer.trim().to_string();
                    if input.is_empty() {
                        return;
                    }

                    // If no API key yet, treat input as the API key
                    if app.tmdb.api_key.is_none() {
                        if let Err(e) = tmdb::save_api_key(&input) {
                            app.status_message = format!("Failed to save API key: {}", e);
                            return;
                        }
                        app.tmdb.api_key = Some(input);
                        app.wizard.input_buffer = app.tmdb.search_query.clone();
                        app.status_message.clear();
                        return;
                    }

                    // Spawn TMDb search in background thread
                    if let Some(ref api_key) = app.tmdb.api_key.clone() {
                        if app.pending_rx.is_some() {
                            return; // Already waiting for a result
                        }
                        let api_key = api_key.clone();
                        let query = input;
                        let (tx, rx) = mpsc::channel();
                        if app.tmdb.movie_mode {
                            std::thread::spawn(move || {
                                let _ = tx.send(BackgroundResult::MovieSearch(tmdb::search_movie(
                                    &query, &api_key,
                                )));
                            });
                        } else {
                            std::thread::spawn(move || {
                                let _ = tx.send(BackgroundResult::ShowSearch(tmdb::search_show(
                                    &query, &api_key,
                                )));
                            });
                        }
                        app.pending_rx = Some(rx);
                        app.status_message = "Searching TMDb...".into();
                    }
                }
                KeyCode::Down => {
                    let has_results = if app.tmdb.movie_mode {
                        !app.tmdb.movie_results.is_empty()
                    } else {
                        !app.tmdb.search_results.is_empty()
                    };
                    if has_results {
                        app.wizard.input_focus = InputFocus::List;
                        app.wizard.list_cursor = 0;
                    }
                }
                KeyCode::Tab if app.tmdb.api_key.is_some() => {
                    app.tmdb.movie_mode = !app.tmdb.movie_mode;
                }
                KeyCode::Esc => {
                    app.wizard.input_focus = InputFocus::List;
                    app.wizard.list_cursor = 0;
                    app.screen = Screen::PlaylistManager;
                }
                _ => {}
            }
        }
        InputFocus::List => {
            let result_count = if app.tmdb.movie_mode {
                app.tmdb.movie_results.len()
            } else {
                app.tmdb.search_results.len()
            };

            match key.code {
                KeyCode::Up => {
                    if app.wizard.list_cursor == 0 {
                        app.wizard.input_focus = InputFocus::TextInput;
                    } else {
                        app.wizard.list_cursor -= 1;
                    }
                }
                KeyCode::Down if app.wizard.list_cursor + 1 < result_count => {
                    app.wizard.list_cursor += 1;
                }
                KeyCode::Enter => {
                    if result_count == 0 {
                        return;
                    }

                    if app.tmdb.movie_mode {
                        app.tmdb.selected_movie = Some(app.wizard.list_cursor);
                        app.tmdb.show_name =
                            app.tmdb.movie_results[app.wizard.list_cursor].title.clone();
                        app.wizard.input_focus = InputFocus::List;
                        app.wizard.list_cursor = 0;
                        app.screen = Screen::PlaylistManager;
                    } else {
                        app.tmdb.selected_show = Some(app.wizard.list_cursor);
                        let show = &app.tmdb.search_results[app.wizard.list_cursor];
                        app.tmdb.show_name = show.name.clone();

                        // If we already have a season number, fetch episodes in background
                        if let Some(season) = app.wizard.season_num {
                            if let Some(ref api_key) = app.tmdb.api_key.clone() {
                                let api_key = api_key.clone();
                                let show_id = show.id;
                                let (tx, rx) = mpsc::channel();
                                std::thread::spawn(move || {
                                    let _ = tx.send(BackgroundResult::SeasonFetch(
                                        tmdb::get_season(show_id, season, &api_key),
                                    ));
                                });
                                app.pending_rx = Some(rx);
                                app.status_message = "Fetching season...".into();
                            }
                        }

                        app.wizard.input_buffer = app
                            .wizard
                            .season_num
                            .map(|s| s.to_string())
                            .unwrap_or_default();
                        app.wizard.input_focus = InputFocus::TextInput;
                        app.wizard.list_cursor = 0;
                        app.screen = Screen::Season;
                    }
                }
                KeyCode::Esc => {
                    app.tmdb.search_results.clear();
                    app.tmdb.movie_results.clear();
                    app.wizard.input_focus = InputFocus::TextInput;
                }
                _ => {}
            }
        }
        InputFocus::InlineEdit(_) => {}
    }
}

pub fn render_season_view(
    f: &mut Frame,
    view: &SeasonView,
    status: &str,
    _spinner: usize,
    area: Rect,
) {
    let chunks = standard_layout(area);

    let disc_text = label_text(&view.label);
    let header_text = if disc_text.is_empty() {
        format!("Show: {}", view.show_name)
    } else {
        format!("{}  |  Show: {}", disc_text, view.show_name)
    };

    let title = Paragraph::new(header_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Step 2: Season"),
    );
    f.render_widget(title, chunks[0]);

    let content_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(chunks[1]);

    // Season number input
    let input_active = matches!(view.input_focus, InputFocus::TextInput);
    let season_display = if input_active {
        format!("{}|", view.input_buffer)
    } else {
        view.season_num
            .map(|s| s.to_string())
            .unwrap_or_default()
    };
    let season_style = Style::default().fg(Color::Yellow);
    let season_input = Paragraph::new(season_display).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Season number")
            .border_style(season_style),
    );
    f.render_widget(season_input, content_chunks[0]);

    // Episode list preview (when fetched from TMDb)
    if !view.episodes.is_empty() {
        let ep_lines: Vec<Line> = view
            .episodes
            .iter()
            .map(|ep| {
                let runtime = ep.runtime.unwrap_or(0);
                Line::from(format!(
                    "  E{:02} - {} ({} min)",
                    ep.episode_number, ep.name, runtime
                ))
            })
            .collect();

        let available_height = content_chunks[1].height.saturating_sub(2) as usize; // minus borders
        let total = ep_lines.len();
        let max_scroll = total.saturating_sub(available_height);
        let scroll_offset = view.list_cursor.min(max_scroll);

        let title = if max_scroll > 0 {
            format!(
                "Season {}: {} episodes (↑/↓ to scroll)",
                view.season_num.unwrap_or(0),
                total
            )
        } else {
            format!(
                "Season {}: {} episodes",
                view.season_num.unwrap_or(0),
                total
            )
        };

        let list = Paragraph::new(ep_lines)
            .block(Block::default().borders(Borders::ALL).title(title))
            .scroll((scroll_offset as u16, 0));
        f.render_widget(list, content_chunks[1]);
    } else if !status.is_empty() {
        let msg = Paragraph::new(status.to_string())
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("Episodes"));
        f.render_widget(msg, content_chunks[1]);
    } else {
        let empty = Paragraph::new("Enter season number and press Enter to fetch episodes")
            .block(Block::default().borders(Borders::ALL).title("Episodes"));
        f.render_widget(empty, content_chunks[1]);
    }

    let hints = Paragraph::new(
        "Enter: Confirm/Fetch | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan | Ctrl+S: Settings",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn render_season(f: &mut Frame, app: &App) {
    let show_name = app
        .tmdb
        .selected_show
        .and_then(|i| app.tmdb.search_results.get(i))
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    let view = SeasonView {
        show_name,
        season_num: app.wizard.season_num,
        input_buffer: app.wizard.input_buffer.clone(),
        input_focus: app.wizard.input_focus.clone(),
        episodes: app.tmdb.episodes.clone(),
        list_cursor: app.wizard.list_cursor,
        label: app.disc.label.clone(),
    };
    let status = format_status(
        &app.status_message,
        app.pending_rx.is_some(),
        app.spinner_frame,
    );
    render_season_view(f, &view, &status, app.spinner_frame, f.area());
}

pub fn handle_season_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up if app.wizard.list_cursor > 0 => {
            app.wizard.list_cursor -= 1;
        }
        KeyCode::Down => {
            let max_scroll = app.tmdb.episodes.len();
            if app.wizard.list_cursor < max_scroll {
                app.wizard.list_cursor += 1;
            }
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            app.wizard.input_buffer.push(c);
        }
        KeyCode::Backspace => {
            app.wizard.input_buffer.pop();
        }
        KeyCode::Enter => {
            if !app.tmdb.episodes.is_empty() {
                // Episodes already fetched — check if season changed
                let current_input: Option<u32> = app.wizard.input_buffer.parse().ok();
                if current_input != app.wizard.season_num {
                    // Season changed, re-fetch
                    let season: u32 = match app.wizard.input_buffer.parse() {
                        Ok(s) => s,
                        _ => return,
                    };
                    app.wizard.season_num = Some(season);
                    app.tmdb.episodes.clear();

                    let show_id = app
                        .tmdb
                        .selected_show
                        .and_then(|i| app.tmdb.search_results.get(i))
                        .map(|s| s.id);

                    if let (Some(show_id), Some(ref api_key)) = (show_id, app.tmdb.api_key.clone())
                    {
                        let api_key = api_key.clone();
                        let (tx, rx) = mpsc::channel();
                        std::thread::spawn(move || {
                            let _ = tx.send(BackgroundResult::SeasonFetch(tmdb::get_season(
                                show_id, season, &api_key,
                            )));
                        });
                        app.pending_rx = Some(rx);
                        app.status_message = "Fetching season...".into();
                    }
                } else {
                    // Episodes loaded and season unchanged — auto-assign and proceed
                    let disc_num = app.disc.label_info.as_ref().map(|l| l.disc);
                    let guessed = guess_start_episode(disc_num, app.disc.episodes_pl.len());
                    let start_ep = app.wizard.start_episode.unwrap_or(guessed);
                    app.wizard.episode_assignments =
                        assign_episodes(&app.disc.episodes_pl, &app.tmdb.episodes, start_ep);
                    app.wizard.input_focus = InputFocus::List;
                    app.wizard.input_buffer.clear();
                    app.wizard.list_cursor = 0;
                    app.screen = Screen::PlaylistManager;
                }
            } else {
                // No episodes yet — fetch them
                let season: u32 = match app.wizard.input_buffer.parse() {
                    Ok(s) => s,
                    _ => return,
                };
                app.wizard.season_num = Some(season);

                let show_id = app
                    .tmdb
                    .selected_show
                    .and_then(|i| app.tmdb.search_results.get(i))
                    .map(|s| s.id);

                if let (Some(show_id), Some(ref api_key)) = (show_id, app.tmdb.api_key.clone()) {
                    let api_key = api_key.clone();
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(BackgroundResult::SeasonFetch(tmdb::get_season(
                            show_id, season, &api_key,
                        )));
                    });
                    app.pending_rx = Some(rx);
                    app.status_message = "Fetching season...".into();
                }
            }
        }
        KeyCode::Esc => {
            app.tmdb.episodes.clear();
            app.wizard.input_buffer = app.tmdb.search_query.clone();
            app.wizard.input_focus = InputFocus::TextInput;
            app.wizard.list_cursor = 0;
            app.screen = Screen::TmdbSearch;
        }
        _ => {}
    }
}

fn visible_playlists(app: &App) -> Vec<(usize, &crate::types::Playlist)> {
    app.disc
        .playlists
        .iter()
        .enumerate()
        .filter(|(_, pl)| {
            app.wizard.show_filtered || app.disc.episodes_pl.iter().any(|ep| ep.num == pl.num)
        })
        .collect()
}

fn visible_playlists_view(view: &PlaylistView) -> Vec<(usize, &crate::types::Playlist)> {
    view.playlists
        .iter()
        .enumerate()
        .filter(|(_, pl)| {
            view.show_filtered || view.episodes_pl.iter().any(|ep| ep.num == pl.num)
        })
        .collect()
}

pub fn render_playlist_manager_view(
    f: &mut Frame,
    view: &PlaylistView,
    status: &str,
    area: Rect,
) {
    let chunks = standard_layout(area);

    let disc_text = label_text(&view.label);
    let show_name = &view.show_name;
    let selected_count = view.playlist_selected.iter().filter(|&&s| s).count();
    let visible = visible_playlists_view(view);
    let hidden_count = view.playlists.len() - visible.len();

    let header_text = if disc_text.is_empty() {
        if hidden_count > 0 {
            format!(
                "Show: {}  |  {} selected, {} hidden",
                show_name, selected_count, hidden_count
            )
        } else {
            format!("Show: {}  |  {} selected", show_name, selected_count)
        }
    } else {
        if hidden_count > 0 {
            format!(
                "{}  |  Show: {}  |  {} selected, {} hidden",
                disc_text, show_name, selected_count, hidden_count
            )
        } else {
            format!(
                "{}  |  Show: {}  |  {} selected",
                disc_text, show_name, selected_count
            )
        }
    };

    let title = Paragraph::new(header_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Playlist Manager"),
    );
    f.render_widget(title, chunks[0]);

    let has_ch = !view.chapter_counts.is_empty();
    let is_tv = !view.movie_mode;

    let mut header_cells = vec!["", "#", "Playlist", "Duration"];
    if has_ch {
        header_cells.push("Ch");
    }
    if is_tv {
        header_cells.push("Episode(s)");
    }
    header_cells.push("Filename");
    let header = Row::new(header_cells).style(Style::default().fg(Color::White));

    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(vis_idx, &(real_idx, pl))| {
            let checked = if view
                .playlist_selected
                .get(real_idx)
                .copied()
                .unwrap_or(false)
            {
                "[x]"
            } else {
                "[ ]"
            };
            let cursor_marker = if vis_idx == view.list_cursor {
                ">"
            } else {
                " "
            };
            let marker = format!("{} {}", cursor_marker, checked);

            let is_episode_pl = view.episodes_pl.iter().any(|ep| ep.num == pl.num);
            let is_special = view.specials.contains(&pl.num);
            let is_editing =
                matches!(view.input_focus, InputFocus::InlineEdit(r) if r == vis_idx);

            // Episode column
            let ep_str = if is_editing {
                format!("{}|", view.input_buffer)
            } else if is_special {
                if let Some(eps) = view.episode_assignments.get(&pl.num) {
                    if eps.is_empty() {
                        "(none)".to_string()
                    } else {
                        let season = view.season_num.unwrap_or(0);
                        if eps.len() == 1 {
                            if eps[0].name.is_empty() {
                                format!("S{:02}SP{:02}", season, eps[0].episode_number)
                            } else {
                                format!(
                                    "S{:02}SP{:02} - {}",
                                    season, eps[0].episode_number, eps[0].name
                                )
                            }
                        } else {
                            let first = &eps[0];
                            let last = &eps[eps.len() - 1];
                            if first.name.is_empty() {
                                format!(
                                    "S{:02}SP{:02}-SP{:02}",
                                    season, first.episode_number, last.episode_number
                                )
                            } else {
                                format!(
                                    "S{:02}SP{:02}-SP{:02} - {}",
                                    season, first.episode_number, last.episode_number, first.name
                                )
                            }
                        }
                    }
                } else {
                    "(none)".to_string()
                }
            } else if let Some(eps) = view.episode_assignments.get(&pl.num) {
                if eps.is_empty() {
                    "(none)".to_string()
                } else {
                    let season = view.season_num.unwrap_or(0);
                    if eps.len() == 1 {
                        if eps[0].name.is_empty() {
                            format!("S{:02}E{:02}", season, eps[0].episode_number)
                        } else {
                            format!(
                                "S{:02}E{:02} - {}",
                                season, eps[0].episode_number, eps[0].name
                            )
                        }
                    } else {
                        let first = &eps[0];
                        let last = &eps[eps.len() - 1];
                        if first.name.is_empty() {
                            format!(
                                "S{:02}E{:02}-E{:02}",
                                season, first.episode_number, last.episode_number
                            )
                        } else {
                            format!(
                                "S{:02}E{:02}-E{:02} - {}",
                                season, first.episode_number, last.episode_number, first.name
                            )
                        }
                    }
                }
            } else {
                "(none)".to_string()
            };

            let special_marker = if is_special { " [SP]" } else { "" };
            let ep_display = format!("{}{}", ep_str, special_marker);

            let filename = view
                .filenames
                .get(&pl.num)
                .cloned()
                .unwrap_or_default();
            let ch_str = view
                .chapter_counts
                .get(&pl.num)
                .map(|c| c.to_string())
                .unwrap_or_default();

            let mut cells = vec![
                marker,
                format!("{}", vis_idx + 1),
                pl.num.clone(),
                pl.duration.clone(),
            ];
            if has_ch {
                cells.push(ch_str);
            }
            if is_tv {
                cells.push(ep_display);
            }
            cells.push(filename);

            let row_style = if is_editing {
                Style::default().fg(Color::Yellow)
            } else if vis_idx == view.list_cursor {
                Style::default().fg(Color::White)
            } else if !is_episode_pl {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };

            Row::new(cells).style(row_style)
        })
        .collect();

    let mut widths = vec![
        Constraint::Length(6),
        Constraint::Length(4),
        Constraint::Length(10),
        Constraint::Length(10),
    ];
    if has_ch {
        widths.push(Constraint::Length(4));
    }
    if is_tv {
        widths.push(Constraint::Min(20));
    }
    widths.push(Constraint::Min(20));

    let table = Table::new(rows, &widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(table, chunks[1]);

    let hints_text = if matches!(view.input_focus, InputFocus::InlineEdit(_)) {
        "Enter: Confirm | Esc: Cancel | Format: 3 or 3-4 or 3,5".to_string()
    } else {
        let mut parts = vec!["Space: Toggle", "e: Edit"];
        if is_tv {
            parts.push("s: Special");
        }
        parts.push("r/R: Reset");
        parts.push("f: Show filtered");
        parts.push("Enter: Confirm");
        parts.push("Esc: Back");
        parts.push("Ctrl+S: Settings");
        parts.join(" | ")
    };

    let status_line = if !status.is_empty() {
        format!("{}  {}", hints_text, status)
    } else {
        hints_text
    };
    let hints = Paragraph::new(status_line).style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn render_playlist_manager(f: &mut Frame, app: &App) {
    let show_name = if app.tmdb.show_name.is_empty() {
        app.disc
            .label_info
            .as_ref()
            .map(|l| l.show.clone())
            .unwrap_or_else(|| "Unknown".to_string())
    } else {
        app.tmdb.show_name.clone()
    };

    // Pre-compute filenames for all playlists
    let mut filenames = std::collections::HashMap::new();
    for (i, pl) in app.disc.playlists.iter().enumerate() {
        filenames.insert(pl.num.clone(), playlist_filename(app, i, None));
    }

    let view = PlaylistView {
        movie_mode: app.tmdb.movie_mode,
        show_name,
        season_num: app.wizard.season_num,
        playlists: app.disc.playlists.clone(),
        episodes_pl: app.disc.episodes_pl.clone(),
        playlist_selected: app.wizard.playlist_selected.clone(),
        episode_assignments: app.wizard.episode_assignments.clone(),
        specials: app.wizard.specials.clone(),
        show_filtered: app.wizard.show_filtered,
        list_cursor: app.wizard.list_cursor,
        input_focus: app.wizard.input_focus.clone(),
        input_buffer: app.wizard.input_buffer.clone(),
        chapter_counts: app.disc.chapter_counts.clone(),
        episodes: app.tmdb.episodes.clone(),
        label: app.disc.label.clone(),
        filenames,
    };
    render_playlist_manager_view(f, &view, &app.status_message, f.area());
}

pub fn handle_playlist_manager_input(app: &mut App, key: KeyEvent) {
    if let InputFocus::InlineEdit(edit_vis_row) = app.wizard.input_focus {
        let visible = visible_playlists(app);
        match key.code {
            KeyCode::Char(c) if (c.is_ascii_digit() || c == ',' || c == '-') => {
                app.wizard.input_buffer.push(c);
            }
            KeyCode::Backspace => {
                app.wizard.input_buffer.pop();
            }
            KeyCode::Enter => {
                if let Some(&(real_idx, _)) = visible.get(edit_vis_row) {
                    let pl_num = app.disc.playlists[real_idx].num.clone();
                    match parse_episode_input(&app.wizard.input_buffer) {
                        Some(ep_nums) if ep_nums.is_empty() => {
                            app.wizard.episode_assignments.remove(&pl_num);
                        }
                        Some(ep_nums) => {
                            let ep_by_num: std::collections::HashMap<u32, &crate::types::Episode> =
                                app.tmdb
                                    .episodes
                                    .iter()
                                    .map(|e| (e.episode_number, e))
                                    .collect();
                            let eps: Vec<crate::types::Episode> = ep_nums
                                .iter()
                                .map(|&num| {
                                    ep_by_num.get(&num).map(|e| (*e).clone()).unwrap_or(
                                        crate::types::Episode {
                                            episode_number: num,
                                            name: String::new(),
                                            runtime: None,
                                        },
                                    )
                                })
                                .collect();
                            app.wizard.episode_assignments.insert(pl_num, eps);
                        }
                        None => {
                            return;
                        }
                    }
                }
                app.wizard.input_focus = InputFocus::List;
                app.wizard.input_buffer.clear();
            }
            KeyCode::Esc => {
                app.wizard.input_focus = InputFocus::List;
                app.wizard.input_buffer.clear();
            }
            _ => {}
        }
        return;
    }

    let visible = visible_playlists(app);
    let vis_len = visible.len();

    match key.code {
        KeyCode::Up if app.wizard.list_cursor > 0 => {
            app.wizard.list_cursor -= 1;
        }
        KeyCode::Down if app.wizard.list_cursor + 1 < vis_len => {
            app.wizard.list_cursor += 1;
        }
        KeyCode::Char(' ') => {
            if let Some(&(real_idx, _)) = visible.get(app.wizard.list_cursor) {
                if let Some(sel) = app.wizard.playlist_selected.get_mut(real_idx) {
                    *sel = !*sel;
                }
            }
        }
        KeyCode::Char('e') => {
            if let Some(&(real_idx, _)) = visible.get(app.wizard.list_cursor) {
                let pl_num = &app.disc.playlists[real_idx].num;
                let current = app
                    .wizard
                    .episode_assignments
                    .get(pl_num)
                    .map(|eps| {
                        eps.iter()
                            .map(|e| e.episode_number.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                app.wizard.input_buffer = current;
                app.wizard.input_focus = InputFocus::InlineEdit(app.wizard.list_cursor);
            }
        }
        KeyCode::Char('s') if !app.tmdb.movie_mode => {
            if let Some(&(real_idx, _)) = visible.get(app.wizard.list_cursor) {
                let pl_num = app.disc.playlists[real_idx].num.clone();
                if app.wizard.specials.contains(&pl_num) {
                    app.wizard.specials.remove(&pl_num);
                    app.wizard.episode_assignments.remove(&pl_num);
                } else {
                    // Auto-assign next S00 episode number
                    let max_s00_ep = app
                        .wizard
                        .specials
                        .iter()
                        .filter_map(|snum| {
                            app.wizard
                                .episode_assignments
                                .get(snum)
                                .and_then(|eps| eps.first())
                                .map(|e| e.episode_number)
                        })
                        .max()
                        .unwrap_or(0);
                    let next_ep = max_s00_ep + 1;
                    app.wizard.specials.insert(pl_num.clone());
                    app.wizard.episode_assignments.insert(
                        pl_num,
                        vec![crate::types::Episode {
                            episode_number: next_ep,
                            name: String::new(),
                            runtime: None,
                        }],
                    );
                    // Also select it
                    if let Some(sel) = app.wizard.playlist_selected.get_mut(real_idx) {
                        *sel = true;
                    }
                }
            }
        }
        KeyCode::Char('r') => {
            // Reset current row's episode assignment
            if let Some(&(real_idx, _)) = visible.get(app.wizard.list_cursor) {
                let pl_num = &app.disc.playlists[real_idx].num;
                app.wizard.episode_assignments.remove(pl_num);
                app.wizard.specials.remove(pl_num);
            }
        }
        KeyCode::Char('R') => {
            // Reset all episode assignments and specials
            app.wizard.episode_assignments.clear();
            app.wizard.specials.clear();
        }
        KeyCode::Char('f') => {
            app.wizard.show_filtered = !app.wizard.show_filtered;
            // Clamp cursor if it would be out of bounds
            let new_visible = visible_playlists(app);
            if app.wizard.list_cursor >= new_visible.len() {
                app.wizard.list_cursor = new_visible.len().saturating_sub(1);
            }
        }
        KeyCode::Enter => {
            // Collect ALL selected playlists (not just visible)
            let selected_nums: Vec<String> = app
                .disc
                .playlists
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    app.wizard
                        .playlist_selected
                        .get(*i)
                        .copied()
                        .unwrap_or(false)
                })
                .map(|(_, pl)| pl.num.clone())
                .collect();

            if selected_nums.is_empty() {
                app.status_message = "No playlists selected.".into();
                return;
            }

            if app.pending_rx.is_some() {
                return;
            }

            let device = app.args.device().to_string_lossy().to_string();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let infos: Vec<Option<crate::types::MediaInfo>> = selected_nums
                    .iter()
                    .map(|num| crate::disc::probe_media_info(&device, num))
                    .collect();
                let _ = tx.send(BackgroundResult::MediaProbe(infos));
            });
            app.pending_rx = Some(rx);
            app.status_message = "Probing media info...".into();
        }
        KeyCode::Esc => {
            app.wizard.list_cursor = 0;
            if app.tmdb.movie_mode {
                app.wizard.input_focus = InputFocus::List;
                app.screen = Screen::TmdbSearch;
            } else if app.wizard.season_num.is_some() && app.tmdb.selected_show.is_some() {
                app.wizard.input_focus = InputFocus::TextInput;
                app.wizard.input_buffer = app
                    .wizard
                    .season_num
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                app.screen = Screen::Season;
            } else {
                app.wizard.input_focus = InputFocus::TextInput;
                app.wizard.input_buffer = app.tmdb.search_query.clone();
                app.screen = Screen::TmdbSearch;
            }
        }
        _ => {}
    }
}

pub fn render_confirm_view(
    f: &mut Frame,
    view: &ConfirmView,
    _status: &str,
    area: Rect,
) {
    let chunks = standard_layout(area);

    let disc_text = label_text(&view.label);
    let header_text = if disc_text.is_empty() {
        format!(
            "Ready to rip {} playlist(s) to {}",
            view.filenames.len(),
            view.output_dir,
        )
    } else {
        format!(
            "{}  |  Ready to rip {} playlist(s) to {}",
            disc_text,
            view.filenames.len(),
            view.output_dir,
        )
    };

    let title = Paragraph::new(header_text).block(Block::default().borders(Borders::ALL).title(
        if view.movie_mode {
            "Step 4: Confirm"
        } else {
            "Step 6: Confirm"
        },
    ));
    f.render_widget(title, chunks[0]);

    let header = Row::new(vec!["Playlist", "Duration", "~Size", "Output File"])
        .style(Style::default().fg(Color::Yellow));

    // Fallback: ~20 Mbps (2.5 MB/s) if no probed bitrate available
    const FALLBACK_BYTERATE: u64 = 2_500_000;

    let mut total_seconds: u32 = 0;
    let mut total_est_bytes: u64 = 0;

    let rows: Vec<Row> = view
        .playlists
        .iter()
        .zip(view.filenames.iter())
        .enumerate()
        .map(|(i, (pl, name))| {
            total_seconds += pl.seconds;
            let byterate = view
                .media_infos
                .get(i)
                .and_then(|info| info.as_ref())
                .map(|info| info.bitrate_bps / 8)
                .filter(|&br| br > 0)
                .unwrap_or(FALLBACK_BYTERATE);
            let est_bytes = pl.seconds as u64 * byterate;
            total_est_bytes += est_bytes;
            Row::new(vec![
                pl.num.clone(),
                pl.duration.clone(),
                format!("~{}", crate::util::format_size(est_bytes)),
                name.clone(),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Min(30),
    ];

    let total_h = total_seconds / 3600;
    let total_m = (total_seconds % 3600) / 60;
    let summary_title = format!(
        "Summary — ~{} total, ~{}h {:02}m of content",
        crate::util::format_size(total_est_bytes),
        total_h,
        total_m,
    );

    let table = Table::new(rows, &widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(summary_title));
    f.render_widget(table, chunks[1]);

    let hint_text = if view.dry_run {
        "Enter: Exit (dry run) | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan | Ctrl+S: Settings"
    } else {
        "Enter: Start Ripping | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan | Ctrl+S: Settings"
    };
    let hints = Paragraph::new(hint_text).style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn render_confirm(f: &mut Frame, app: &App) {
    let selected_playlists: Vec<crate::types::Playlist> = app
        .disc
        .playlists
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            app.wizard
                .playlist_selected
                .get(*i)
                .copied()
                .unwrap_or(false)
        })
        .map(|(_, pl)| pl.clone())
        .collect();

    let view = ConfirmView {
        filenames: app.wizard.filenames.clone(),
        playlists: selected_playlists,
        episode_assignments: app.wizard.episode_assignments.clone(),
        list_cursor: app.wizard.list_cursor,
        movie_mode: app.tmdb.movie_mode,
        label: app.disc.label.clone(),
        output_dir: app.args.output.display().to_string(),
        dry_run: app.args.dry_run,
        media_infos: app.wizard.media_infos.clone(),
    };
    render_confirm_view(f, &view, &app.status_message, f.area());
}

pub fn handle_confirm_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            if app.args.dry_run {
                app.status_message = format!(
                    "[DRY RUN] Would rip {} playlist(s) to {}",
                    app.wizard.filenames.len(),
                    app.args.output.display(),
                );
                app.screen = Screen::Done;
                return;
            }

            // Build RipJobs from selected playlists and filenames
            app.rip.jobs.clear();

            let selected_playlists: Vec<crate::types::Playlist> = app
                .disc
                .playlists
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    app.wizard
                        .playlist_selected
                        .get(*i)
                        .copied()
                        .unwrap_or(false)
                })
                .map(|(_, pl)| pl.clone())
                .collect();

            for (pl, filename) in selected_playlists
                .into_iter()
                .zip(app.wizard.filenames.iter())
            {
                let episode = app
                    .wizard
                    .episode_assignments
                    .get(&pl.num)
                    .cloned()
                    .unwrap_or_default();
                app.rip.jobs.push(crate::types::RipJob {
                    playlist: pl,
                    episode,
                    filename: filename.clone(),
                    status: crate::types::PlaylistStatus::Pending,
                });
            }

            app.rip.current_rip = 0;
            app.screen = Screen::Ripping;

            match crate::disc::ensure_mounted(&app.args.device().to_string_lossy()) {
                Ok((mount, did_mount)) => {
                    app.disc.mount_point = Some(mount);
                    app.disc.did_mount = did_mount;
                }
                Err(_) => {
                    app.disc.mount_point = None;
                    app.disc.did_mount = false;
                }
            }
        }
        KeyCode::Esc => {
            app.wizard.list_cursor = 0;
            app.screen = Screen::PlaylistManager;
        }
        _ => {}
    }
}
