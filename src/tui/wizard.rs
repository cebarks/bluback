use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table};
use std::sync::mpsc;

use super::{App, InputFocus, Screen};
use crate::tmdb;
use crate::types::BackgroundResult;
use crate::util::{assign_episodes, guess_start_episode, make_filename, make_movie_filename, parse_episode_input};

pub fn playlist_filename(
    app: &App,
    playlist_index: usize,
    media_info: Option<&crate::types::MediaInfo>,
) -> String {
    let pl = &app.disc.episodes_pl[playlist_index];

    let format_template = app.config.resolve_format(
        app.tmdb.movie_mode,
        app.args.format.as_deref(),
        app.args.format_preset.as_deref(),
    );
    let use_custom = app.args.format.is_some()
        || app.args.format_preset.is_some()
        || app.config.tv_format.is_some()
        || app.config.movie_format.is_some()
        || app.config.preset.is_some();
    let fmt = if use_custom {
        Some(format_template.as_str())
    } else {
        None
    };

    // Build extra vars
    let mut extra: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    let show_name = if !app.tmdb.show_name.is_empty() {
        app.tmdb.show_name.clone()
    } else {
        app.disc
            .label_info
            .as_ref()
            .map(|l| l.show.clone())
            .unwrap_or_else(|| "Unknown".to_string())
    };
    extra.insert("show", show_name);
    extra.insert(
        "disc",
        app.disc
            .label_info
            .as_ref()
            .map(|l| l.disc.to_string())
            .unwrap_or_default(),
    );
    extra.insert("label", app.disc.label.clone());
    extra.insert("playlist", pl.num.clone());

    if app.tmdb.movie_mode {
        let movie = app.tmdb.selected_movie.and_then(|i| app.tmdb.movie_results.get(i));
        let title = movie.map(|m| m.title.as_str()).unwrap_or("movie");
        let year = movie
            .and_then(|m| m.release_date.as_deref())
            .and_then(|d| d.get(..4))
            .unwrap_or("");
        let part = if app.disc.episodes_pl.len() > 1 {
            Some(playlist_index as u32 + 1)
        } else {
            None
        };
        make_movie_filename(title, year, part, fmt, media_info, Some(&extra))
    } else {
        let episodes = app
            .wizard
            .episode_assignments
            .get(&pl.num)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        make_filename(
            &pl.num,
            episodes,
            app.wizard.season_num.unwrap_or(0),
            fmt,
            media_info,
            Some(&extra),
        )
    }
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

pub fn render_scanning(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let title = Paragraph::new("bluback").block(
        Block::default()
            .borders(Borders::ALL)
            .title("Blu-ray Backup"),
    );
    f.render_widget(title, chunks[0]);

    let mut lines: Vec<Line> = app
        .disc
        .scan_log
        .iter()
        .map(|s| Line::from(s.as_str()).style(Style::default().fg(Color::DarkGray)))
        .collect();
    if !app.status_message.is_empty() {
        lines.push(Line::from(app.status_message.as_str()));
    }
    let body = Paragraph::new(lines);
    f.render_widget(body, chunks[1]);

    let hints = Paragraph::new("q: Quit | Ctrl+E: Eject | Ctrl+R: Rescan")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn render_tmdb_search(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let mode_label = if app.tmdb.movie_mode { "Movie" } else { "TV Show" };
    let step_title = format!("Step 1: TMDb Search ({})", mode_label);
    let title = Paragraph::new(format!(
        "Disc: {}  |  {} playlists",
        if app.disc.label.is_empty() {
            "(no label)"
        } else {
            &app.disc.label
        },
        app.disc.episodes_pl.len(),
    ))
    .block(Block::default().borders(Borders::ALL).title(step_title));
    f.render_widget(title, chunks[0]);

    if app.tmdb.api_key.is_none() {
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

        let input = Paragraph::new(format!("{}|", app.wizard.input_buffer))
            .block(Block::default().borders(Borders::ALL).title("TMDb API Key"));
        f.render_widget(input, content_chunks[1]);

        let hints =
            Paragraph::new("Enter: Save key | Esc: Skip TMDb | Ctrl+E: Eject | Ctrl+R: Rescan")
                .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hints, chunks[2]);
    } else {
        // Search input + inline results
        let has_results = if app.tmdb.movie_mode {
            !app.tmdb.movie_results.is_empty()
        } else {
            !app.tmdb.search_results.is_empty()
        };

        let content_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
            ])
            .split(chunks[1]);

        // Search input field
        let input_style = if app.wizard.input_focus == InputFocus::TextInput {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let cursor = if app.wizard.input_focus == InputFocus::TextInput { "|" } else { "" };
        let mut lines = vec![Line::from(format!("{}{}", app.wizard.input_buffer, cursor))];
        if !app.status_message.is_empty() {
            lines.push(
                Line::from(app.status_message.as_str()).style(Style::default().fg(Color::Yellow)),
            );
        }
        let input = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Search query").border_style(input_style));
        f.render_widget(input, content_chunks[0]);

        // Results list (when results exist)
        if has_results {
            let items: Vec<ListItem> = if app.tmdb.movie_mode {
                app.tmdb
                    .movie_results
                    .iter()
                    .enumerate()
                    .map(|(i, movie)| {
                        let year = movie
                            .release_date
                            .as_deref()
                            .unwrap_or("")
                            .get(..4)
                            .unwrap_or("");
                        let marker = if app.wizard.input_focus == InputFocus::List && i == app.wizard.list_cursor { "> " } else { "  " };
                        ListItem::new(format!("{}{} ({})", marker, movie.title, year))
                    })
                    .collect()
            } else {
                app.tmdb
                    .search_results
                    .iter()
                    .enumerate()
                    .map(|(i, show)| {
                        let year = show
                            .first_air_date
                            .as_deref()
                            .unwrap_or("")
                            .get(..4)
                            .unwrap_or("");
                        let marker = if app.wizard.input_focus == InputFocus::List && i == app.wizard.list_cursor { "> " } else { "  " };
                        ListItem::new(format!("{}{} ({})", marker, show.name, year))
                    })
                    .collect()
            };

            let list_style = if app.wizard.input_focus == InputFocus::List {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Results").border_style(list_style))
                .highlight_style(Style::default().fg(Color::Yellow));
            f.render_widget(list, content_chunks[1]);
        }

        let toggle = if app.tmdb.movie_mode { "TV Show" } else { "Movie" };
        let hints_text = if app.wizard.input_focus == InputFocus::List {
            "Up/Down: Navigate | Enter: Select | Esc: Back to search | Ctrl+R: Rescan".to_string()
        } else {
            format!(
                "Enter: Search | Down: Results | Tab: Switch to {} | Esc: Skip TMDb | Ctrl+R: Rescan",
                toggle
            )
        };
        let hints = Paragraph::new(hints_text).style(Style::default().fg(Color::DarkGray));
        f.render_widget(hints, chunks[2]);
    }
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
                KeyCode::Tab => {
                    if app.tmdb.api_key.is_some() {
                        app.tmdb.movie_mode = !app.tmdb.movie_mode;
                    }
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
                KeyCode::Down => {
                    if app.wizard.list_cursor + 1 < result_count {
                        app.wizard.list_cursor += 1;
                    }
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

                        app.wizard.season_field = 0;
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

#[allow(dead_code)]
pub fn render_show_select(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let step_title = if app.tmdb.movie_mode {
        "Step 2: Select Movie"
    } else {
        "Step 2: Select Show"
    };
    let prompt = if app.tmdb.movie_mode {
        "Select a movie from the search results"
    } else {
        "Select a show from the search results"
    };
    let title =
        Paragraph::new(prompt).block(Block::default().borders(Borders::ALL).title(step_title));
    f.render_widget(title, chunks[0]);

    let items: Vec<ListItem> = if app.tmdb.movie_mode {
        app.tmdb
            .movie_results
            .iter()
            .enumerate()
            .map(|(i, movie)| {
                let year = movie
                    .release_date
                    .as_deref()
                    .unwrap_or("")
                    .get(..4)
                    .unwrap_or("");
                let marker = if i == app.wizard.list_cursor { "> " } else { "  " };
                ListItem::new(format!("{}{} ({})", marker, movie.title, year))
            })
            .collect()
    } else {
        app.tmdb
            .search_results
            .iter()
            .enumerate()
            .map(|(i, show)| {
                let year = show
                    .first_air_date
                    .as_deref()
                    .unwrap_or("")
                    .get(..4)
                    .unwrap_or("");
                let marker = if i == app.wizard.list_cursor { "> " } else { "  " };
                ListItem::new(format!("{}{} ({})", marker, show.name, year))
            })
            .collect()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Results"))
        .highlight_style(Style::default().fg(Color::Yellow));
    f.render_widget(list, chunks[1]);

    let hints = Paragraph::new(
        "Up/Down: Navigate | Enter: Select | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

#[allow(dead_code)]
pub fn handle_show_select_input(app: &mut App, key: KeyEvent) {
    let result_count = if app.tmdb.movie_mode {
        app.tmdb.movie_results.len()
    } else {
        app.tmdb.search_results.len()
    };

    match key.code {
        KeyCode::Up => {
            if app.wizard.list_cursor > 0 {
                app.wizard.list_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if app.wizard.list_cursor + 1 < result_count {
                app.wizard.list_cursor += 1;
            }
        }
        KeyCode::Enter => {
            if result_count == 0 {
                return;
            }

            if app.tmdb.movie_mode {
                app.tmdb.selected_movie = Some(app.wizard.list_cursor);
                app.tmdb.show_name = app.tmdb.movie_results[app.wizard.list_cursor].title.clone();
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
                            let _ = tx.send(BackgroundResult::SeasonFetch(tmdb::get_season(
                                show_id, season, &api_key,
                            )));
                        });
                        app.pending_rx = Some(rx);
                        app.status_message = "Fetching season...".into();
                    }
                }

                app.wizard.season_field = 0;
                app.wizard.input_buffer = app.wizard.season_num.map(|s| s.to_string()).unwrap_or_default();
                app.wizard.input_focus = InputFocus::TextInput;
                app.wizard.list_cursor = 0;
                app.screen = Screen::Season;
            }
        }
        KeyCode::Esc => {
            app.wizard.input_buffer = app.tmdb.search_query.clone();
            app.wizard.input_focus = InputFocus::TextInput;
            app.wizard.list_cursor = 0;
            app.screen = Screen::TmdbSearch;
        }
        _ => {}
    }
}

pub fn render_season_episode(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let show_name = app
        .tmdb
        .selected_show
        .and_then(|i| app.tmdb.search_results.get(i))
        .map(|s| s.name.as_str())
        .unwrap_or("Unknown");

    let title = Paragraph::new(format!("Show: {}", show_name)).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Step 3: Season & Starting Episode"),
    );
    f.render_widget(title, chunks[0]);

    let content_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(chunks[1]);

    let input_active = matches!(
        app.wizard.input_focus,
        InputFocus::TextInput | InputFocus::InlineEdit(_)
    );

    // Season input
    let season_active = app.wizard.season_field == 0;
    let season_display = if season_active && input_active {
        format!("{}|", app.wizard.input_buffer)
    } else {
        app.wizard.season_num.map(|s| s.to_string()).unwrap_or_default()
    };
    let season_style = if season_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let season_input = Paragraph::new(season_display).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Season number")
            .border_style(season_style),
    );
    f.render_widget(season_input, content_chunks[0]);

    // Start episode input
    let start_active = app.wizard.season_field == 1;
    let disc_num = app.disc.label_info.as_ref().map(|l| l.disc);
    let guessed = guess_start_episode(disc_num, app.disc.episodes_pl.len());

    let start_display = if start_active && input_active {
        format!("{}|", app.wizard.input_buffer)
    } else {
        app.wizard.start_episode.unwrap_or(guessed).to_string()
    };
    let start_style = if start_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let start_input = Paragraph::new(start_display).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Starting episode (guess: {})", guessed))
            .border_style(start_style),
    );
    f.render_widget(start_input, content_chunks[1]);

    // Episode list preview
    if !app.tmdb.episodes.is_empty() {
        let items: Vec<ListItem> = app
            .tmdb
            .episodes
            .iter()
            .map(|ep| {
                let runtime = ep.runtime.unwrap_or(0);
                ListItem::new(format!(
                    "  E{:02} - {} ({} min)",
                    ep.episode_number, ep.name, runtime
                ))
            })
            .collect();

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
            "Season {}: {} episodes",
            app.wizard.season_num.unwrap_or(0),
            app.tmdb.episodes.len()
        )));
        f.render_widget(list, content_chunks[2]);
    } else if !app.status_message.is_empty() {
        let msg = Paragraph::new(app.status_message.as_str())
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("Episodes"));
        f.render_widget(msg, content_chunks[2]);
    } else {
        let empty = Paragraph::new("Enter season number and press Enter to fetch episodes")
            .block(Block::default().borders(Borders::ALL).title("Episodes"));
        f.render_widget(empty, content_chunks[2]);
    }

    let hints = Paragraph::new(
        "Tab: Switch field | Enter: Confirm/Fetch | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn handle_season_episode_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => {
            if c.is_ascii_digit() {
                app.wizard.input_buffer.push(c);
            }
        }
        KeyCode::Backspace => {
            app.wizard.input_buffer.pop();
        }
        KeyCode::Tab | KeyCode::BackTab => {
            // Save current field value before switching
            if app.wizard.season_field == 0 {
                if let Ok(s) = app.wizard.input_buffer.parse::<u32>() {
                    app.wizard.season_num = Some(s);
                }
                // Switch to start episode field
                app.wizard.season_field = 1;
                let disc_num = app.disc.label_info.as_ref().map(|l| l.disc);
                let guessed = guess_start_episode(disc_num, app.disc.episodes_pl.len());
                app.wizard.input_buffer = app.wizard.start_episode.unwrap_or(guessed).to_string();
            } else {
                if let Ok(s) = app.wizard.input_buffer.parse::<u32>() {
                    app.wizard.start_episode = Some(s);
                }
                // Switch to season field
                app.wizard.season_field = 0;
                app.wizard.input_buffer = app.wizard.season_num.map(|s| s.to_string()).unwrap_or_default();
            }
        }
        KeyCode::Enter => {
            if app.wizard.season_field == 0 {
                // Entering season number — fetch episodes
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

                // Switch to start episode field
                app.wizard.season_field = 1;
                let disc_num = app.disc.label_info.as_ref().map(|l| l.disc);
                let guessed = guess_start_episode(disc_num, app.disc.episodes_pl.len());
                app.wizard.input_buffer = app.wizard.start_episode.unwrap_or(guessed).to_string();
            } else {
                // Entering start episode — confirm and proceed
                let start_ep: u32 = match app.wizard.input_buffer.parse() {
                    Ok(s) if s > 0 => s,
                    _ => return,
                };
                app.wizard.start_episode = Some(start_ep);

                app.wizard.episode_assignments =
                    assign_episodes(&app.disc.episodes_pl, &app.tmdb.episodes, start_ep);

                app.wizard.input_focus = InputFocus::List;
                app.wizard.input_buffer.clear();
                app.wizard.season_field = 0;
                app.wizard.list_cursor = 0;
                app.screen = Screen::PlaylistManager;
            }
        }
        KeyCode::Esc => {
            app.tmdb.episodes.clear();
            app.wizard.input_buffer.clear();
            app.wizard.season_field = 0;
            app.wizard.list_cursor = 0;
            app.wizard.input_focus = InputFocus::List;
            app.screen = Screen::TmdbSearch;
        }
        _ => {}
    }
}

#[allow(dead_code)]
pub fn render_episode_mapping(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let show_name = app
        .tmdb
        .selected_show
        .and_then(|i| app.tmdb.search_results.get(i))
        .map(|s| s.name.as_str())
        .unwrap_or("Unknown");

    let title = Paragraph::new(format!("Show: {}", show_name)).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Step 4: Episode Mapping"),
    );
    f.render_widget(title, chunks[0]);

    let header = Row::new(["", "#", "Playlist", "Duration", "Episode(s)"])
        .style(Style::default().fg(Color::Yellow));

    let rows: Vec<Row> = app
        .disc
        .episodes_pl
        .iter()
        .enumerate()
        .map(|(i, pl)| {
            let cursor = if i == app.wizard.list_cursor { ">" } else { " " };

            let ep_str = if matches!(app.wizard.input_focus, InputFocus::InlineEdit(r) if r == i) {
                format!("{}|", app.wizard.input_buffer)
            } else if let Some(eps) = app.wizard.episode_assignments.get(&pl.num) {
                if eps.is_empty() {
                    "(none)".to_string()
                } else {
                    eps.iter()
                        .map(|e| {
                            if e.name.is_empty() {
                                format!("E{:02}", e.episode_number)
                            } else {
                                format!("E{:02} - {}", e.episode_number, e.name)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            } else {
                "(none)".to_string()
            };

            let row_style = if matches!(app.wizard.input_focus, InputFocus::InlineEdit(r) if r == i) {
                Style::default().fg(Color::Yellow)
            } else if i == app.wizard.list_cursor {
                Style::default().fg(Color::White)
            } else {
                Style::default()
            };

            Row::new(vec![
                cursor.to_string(),
                format!("{}", i + 1),
                pl.num.clone(),
                pl.duration.clone(),
                ep_str,
            ])
            .style(row_style)
        })
        .collect();

    let widths = [
        Constraint::Length(2),
        Constraint::Length(4),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Min(30),
    ];

    let table = Table::new(rows, &widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(table, chunks[1]);

    let hints = if matches!(app.wizard.input_focus, InputFocus::InlineEdit(_)) {
        "Enter: Confirm | Esc: Cancel | Format: 3 or 3-4 or 3,5"
    } else {
        "e: Edit | Enter: Accept | Up/Down: Navigate | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan"
    };
    let hints = Paragraph::new(hints).style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

#[allow(dead_code)]
pub fn handle_episode_mapping_input(app: &mut App, key: KeyEvent) {
    if let InputFocus::InlineEdit(edit_row) = app.wizard.input_focus {
        // Inline edit mode
        match key.code {
            KeyCode::Char(c) => {
                if c.is_ascii_digit() || c == ',' || c == '-' {
                    app.wizard.input_buffer.push(c);
                }
            }
            KeyCode::Backspace => {
                app.wizard.input_buffer.pop();
            }
            KeyCode::Enter => {
                let pl_num = app.disc.episodes_pl[edit_row].num.clone();
                match parse_episode_input(&app.wizard.input_buffer) {
                    Some(ep_nums) if ep_nums.is_empty() => {
                        app.wizard.episode_assignments.remove(&pl_num);
                    }
                    Some(ep_nums) => {
                        let ep_by_num: std::collections::HashMap<u32, &crate::types::Episode> =
                            app.tmdb.episodes.iter().map(|e| (e.episode_number, e)).collect();
                        let eps: Vec<crate::types::Episode> = ep_nums
                            .iter()
                            .map(|&num| {
                                ep_by_num
                                    .get(&num)
                                    .map(|e| (*e).clone())
                                    .unwrap_or(crate::types::Episode {
                                        episode_number: num,
                                        name: String::new(),
                                        runtime: None,
                                    })
                            })
                            .collect();
                        app.wizard.episode_assignments.insert(pl_num, eps);
                    }
                    None => {
                        // Invalid input, stay in edit mode
                        return;
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

    // Navigation mode
    match key.code {
        KeyCode::Up => {
            if app.wizard.list_cursor > 0 {
                app.wizard.list_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if app.wizard.list_cursor + 1 < app.disc.episodes_pl.len() {
                app.wizard.list_cursor += 1;
            }
        }
        KeyCode::Char('e') => {
            let pl_num = &app.disc.episodes_pl[app.wizard.list_cursor].num;
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
        KeyCode::Enter => {
            app.wizard.list_cursor = 0;
            app.screen = Screen::PlaylistManager;
        }
        KeyCode::Esc => {
            app.wizard.list_cursor = 0;
            app.wizard.input_focus = InputFocus::TextInput;
            let disc_num = app.disc.label_info.as_ref().map(|l| l.disc);
            let guessed = app
                .wizard
                .start_episode
                .unwrap_or_else(|| guess_start_episode(disc_num, app.disc.episodes_pl.len()));
            app.wizard.input_buffer = guessed.to_string();
            app.wizard.season_field = 1;
            app.screen = Screen::Season;
        }
        _ => {}
    }
}

pub fn render_playlist_select(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let selected_count = app.wizard.playlist_selected.iter().filter(|&&s| s).count();
    let title = Paragraph::new(format!(
        "{} playlists ({} selected)",
        app.disc.episodes_pl.len(),
        selected_count,
    ))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(if app.tmdb.movie_mode {
                "Step 3: Select Playlists"
            } else {
                "Step 5: Select Playlists"
            }),
    );
    f.render_widget(title, chunks[0]);

    let has_eps = !app.wizard.episode_assignments.is_empty();
    let has_ch = !app.disc.chapter_counts.is_empty();
    let header_cells = match (has_eps, has_ch) {
        (true, true) => vec!["", "#", "Playlist", "Duration", "Ch", "Episode", "Filename"],
        (true, false) => vec!["", "#", "Playlist", "Duration", "Episode", "Filename"],
        (false, true) => vec!["", "#", "Playlist", "Duration", "Ch", "Filename"],
        (false, false) => vec!["", "#", "Playlist", "Duration", "Filename"],
    };
    let header = Row::new(header_cells).style(Style::default().fg(Color::Yellow));

    let rows: Vec<Row> = app
        .disc
        .episodes_pl
        .iter()
        .enumerate()
        .map(|(i, pl)| {
            let checked = if app.wizard.playlist_selected.get(i).copied().unwrap_or(false) {
                "[x]"
            } else {
                "[ ]"
            };
            let cursor = if i == app.wizard.list_cursor { ">" } else { " " };
            let marker = format!("{} {}", cursor, checked);

            let ep_info = if let Some(eps) = app.wizard.episode_assignments.get(&pl.num) {
                if eps.len() == 1 {
                    format!(
                        "S{:02}E{:02} - {}",
                        app.wizard.season_num.unwrap_or(0),
                        eps[0].episode_number,
                        eps[0].name
                    )
                } else if eps.len() > 1 {
                    let first = &eps[0];
                    let last = &eps[eps.len() - 1];
                    format!(
                        "S{:02}E{:02}-E{:02} - {}",
                        app.wizard.season_num.unwrap_or(0),
                        first.episode_number,
                        last.episode_number,
                        first.name
                    )
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let filename = playlist_filename(app, i, None);
            let ch_str = app
                .disc
                .chapter_counts
                .get(&pl.num)
                .map(|c| c.to_string())
                .unwrap_or_default();

            let mut cells = vec![
                marker,
                format!("{}", i + 1),
                pl.num.clone(),
                pl.duration.clone(),
            ];
            if has_ch {
                cells.push(ch_str);
            }
            if has_eps {
                cells.push(ep_info);
            }
            cells.push(filename);
            Row::new(cells)
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
    if has_eps {
        widths.push(Constraint::Min(20));
    }
    widths.push(Constraint::Min(20));

    let table = Table::new(rows, &widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(table, chunks[1]);

    let hints = Paragraph::new(
        "Space: Toggle | Up/Down: Navigate | Enter: Confirm | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn handle_playlist_select_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            if app.wizard.list_cursor > 0 {
                app.wizard.list_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if app.wizard.list_cursor + 1 < app.disc.episodes_pl.len() {
                app.wizard.list_cursor += 1;
            }
        }
        KeyCode::Char(' ') => {
            if let Some(sel) = app.wizard.playlist_selected.get_mut(app.wizard.list_cursor) {
                *sel = !*sel;
            }
        }
        KeyCode::Enter => {
            // Spawn media info probes in background thread
            let selected_nums: Vec<String> = app
                .disc
                .episodes_pl
                .iter()
                .enumerate()
                .filter(|(i, _)| app.wizard.playlist_selected.get(*i).copied().unwrap_or(false))
                .map(|(_, pl)| pl.num.clone())
                .collect();

            if selected_nums.is_empty() {
                app.status_message = "No playlists selected.".into();
                return;
            }

            if app.pending_rx.is_some() {
                return; // Already waiting
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
            if app.tmdb.movie_mode && app.tmdb.selected_movie.is_some() {
                app.wizard.input_focus = InputFocus::List;
                app.screen = Screen::TmdbSearch;
            } else if !app.wizard.episode_assignments.is_empty() {
                app.screen = Screen::PlaylistManager;
            } else {
                app.wizard.input_focus = InputFocus::TextInput;
                app.wizard.input_buffer = app.tmdb.search_query.clone();
                app.screen = Screen::TmdbSearch;
            }
        }
        _ => {}
    }
}

// Stub: delegates to old render_season_episode (will be replaced in Task 5)
pub fn render_season(f: &mut Frame, app: &App) {
    render_season_episode(f, app);
}

// Stub: delegates to old handle_season_episode_input (will be replaced in Task 5)
pub fn handle_season_input(app: &mut App, key: KeyEvent) {
    handle_season_episode_input(app, key);
}

// Stub: delegates to old render_playlist_select (will be replaced in Task 6)
pub fn render_playlist_manager(f: &mut Frame, app: &App) {
    render_playlist_select(f, app);
}

// Stub: delegates to old handle_playlist_select_input (will be replaced in Task 6)
pub fn handle_playlist_manager_input(app: &mut App, key: KeyEvent) {
    handle_playlist_select_input(app, key);
}

pub fn render_confirm(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let title = Paragraph::new(format!(
        "Ready to rip {} playlist(s) to {}",
        app.wizard.filenames.len(),
        app.args.output.display(),
    ))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(if app.tmdb.movie_mode {
                "Step 4: Confirm"
            } else {
                "Step 6: Confirm"
            }),
    );
    f.render_widget(title, chunks[0]);

    let header = Row::new(vec!["Playlist", "Duration", "~Size", "Output File"])
        .style(Style::default().fg(Color::Yellow));

    let selected_playlists: Vec<&crate::types::Playlist> = app
        .disc
        .episodes_pl
        .iter()
        .enumerate()
        .filter(|(i, _)| app.wizard.playlist_selected.get(*i).copied().unwrap_or(false))
        .map(|(_, pl)| pl)
        .collect();

    // Fallback: ~20 Mbps (2.5 MB/s) if no probed bitrate available
    const FALLBACK_BYTERATE: u64 = 2_500_000;

    let mut total_seconds: u32 = 0;
    let mut total_est_bytes: u64 = 0;

    let rows: Vec<Row> = selected_playlists
        .iter()
        .zip(app.wizard.filenames.iter())
        .enumerate()
        .map(|(i, (pl, name))| {
            total_seconds += pl.seconds;
            let byterate = app
                .wizard
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

    let hint_text = if app.args.dry_run {
        "Enter: Exit (dry run) | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan"
    } else {
        "Enter: Start Ripping | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan"
    };
    let hints = Paragraph::new(hint_text).style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
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
                .episodes_pl
                .iter()
                .enumerate()
                .filter(|(i, _)| app.wizard.playlist_selected.get(*i).copied().unwrap_or(false))
                .map(|(_, pl)| pl.clone())
                .collect();

            for (pl, filename) in selected_playlists.into_iter().zip(app.wizard.filenames.iter()) {
                let episode = app.wizard.episode_assignments.get(&pl.num).cloned().unwrap_or_default();
                app.rip.jobs.push(crate::types::RipJob {
                    playlist: pl,
                    episode,
                    filename: filename.clone(),
                    status: crate::types::PlaylistStatus::Pending,
                });
            }

            app.rip.current_rip = 0;
            app.screen = Screen::Ripping;

            if app.has_mkvpropedit {
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
        }
        KeyCode::Esc => {
            app.wizard.list_cursor = 0;
            app.screen = Screen::PlaylistManager;
        }
        _ => {}
    }
}
