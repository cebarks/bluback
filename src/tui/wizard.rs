use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table};
use std::sync::mpsc;

use super::{App, InputFocus, Screen};
use crate::tmdb;
use crate::types::BackgroundResult;
use crate::util::{assign_episodes, guess_start_episode, make_filename, make_movie_filename, parse_episode_input};

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
        let format_template = app.config.resolve_format(
            true,
            app.args.format.as_deref(),
            app.args.format_preset.as_deref(),
        );
        let use_custom = app.args.format.is_some()
            || app.args.format_preset.is_some()
            || app.config.movie_format.is_some()
            || app.config.preset.is_some();
        let fmt = if use_custom {
            Some(format_template.as_str())
        } else {
            None
        };

        let movie = app.tmdb.selected_movie.and_then(|i| app.tmdb.movie_results.get(i));
        let title = movie.map(|m| m.title.as_str()).unwrap_or("movie");
        let year = movie
            .and_then(|m| m.release_date.as_deref())
            .and_then(|d| d.get(..4))
            .unwrap_or("");
        let selected_count = app.wizard.playlist_selected.iter().filter(|&&s| s).count();
        let part = if selected_count > 1 {
            // Determine part number from position among selected playlists
            let part_num = app.disc.playlists.iter().enumerate()
                .filter(|(i, _)| app.wizard.playlist_selected.get(*i).copied().unwrap_or(false))
                .position(|(i, _)| i == playlist_index)
                .map(|p| p as u32 + 1);
            part_num
        } else {
            None
        };
        make_movie_filename(title, year, part, fmt, media_info, Some(&extra))
    } else if is_special {
        let format_template = app.config.resolve_special_format(app.args.format.as_deref());
        let episodes = app
            .wizard
            .episode_assignments
            .get(&pl.num)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        make_filename(
            &pl.num,
            episodes,
            0, // season 0 for specials
            Some(format_template.as_str()),
            media_info,
            Some(&extra),
        )
    } else {
        let format_template = app.config.resolve_format(
            false,
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

fn disc_label_text(app: &App) -> String {
    if app.disc.label.is_empty() {
        String::new()
    } else {
        format!("Disc: {}", app.disc.label)
    }
}

pub fn render_scanning(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let header_text = disc_label_text(app);
    let title = Paragraph::new(header_text).block(
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
        let status = if app.pending_rx.is_some() {
            format!("{} {}", spinner_char(app.spinner_frame), app.status_message)
        } else {
            app.status_message.clone()
        };
        lines.push(Line::from(status));
    }
    let body = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Scanning"),
    );
    f.render_widget(body, chunks[1]);

    let hints = Paragraph::new("q: Quit | Ctrl+E: Eject | Ctrl+R: Rescan | Ctrl+S: Settings")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn render_tmdb_search(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let mode_label = if app.tmdb.movie_mode { "Movie" } else { "TV Show" };
    let step_title = format!("Step 1: TMDb Search ({})", mode_label);
    let disc_text = disc_label_text(app);
    let header_text = if disc_text.is_empty() {
        format!("{} playlists", app.disc.episodes_pl.len())
    } else {
        format!("{}  |  {} playlists", disc_text, app.disc.episodes_pl.len())
    };
    let title = Paragraph::new(header_text)
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
            Paragraph::new("Enter: Save key | Esc: Skip TMDb | Ctrl+E: Eject | Ctrl+R: Rescan | Ctrl+S: Settings")
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
            let status = if app.pending_rx.is_some() {
                format!("{} {}", spinner_char(app.spinner_frame), app.status_message)
            } else {
                app.status_message.clone()
            };
            lines.push(
                Line::from(status).style(Style::default().fg(Color::Yellow)),
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

pub fn render_season(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let show_name = app
        .tmdb
        .selected_show
        .and_then(|i| app.tmdb.search_results.get(i))
        .map(|s| s.name.as_str())
        .unwrap_or("Unknown");

    let disc_text = disc_label_text(app);
    let header_text = if disc_text.is_empty() {
        format!("Show: {}", show_name)
    } else {
        format!("{}  |  Show: {}", disc_text, show_name)
    };

    let title = Paragraph::new(header_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Step 2: Season"),
    );
    f.render_widget(title, chunks[0]);

    let content_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(chunks[1]);

    // Season number input
    let input_active = matches!(app.wizard.input_focus, InputFocus::TextInput);
    let season_display = if input_active {
        format!("{}|", app.wizard.input_buffer)
    } else {
        app.wizard.season_num.map(|s| s.to_string()).unwrap_or_default()
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
    if !app.tmdb.episodes.is_empty() {
        let ep_lines: Vec<Line> = app
            .tmdb
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
        let scroll_offset = app.wizard.list_cursor.min(max_scroll);

        let title = if max_scroll > 0 {
            format!(
                "Season {}: {} episodes (↑/↓ to scroll)",
                app.wizard.season_num.unwrap_or(0),
                total
            )
        } else {
            format!(
                "Season {}: {} episodes",
                app.wizard.season_num.unwrap_or(0),
                total
            )
        };

        let list = Paragraph::new(ep_lines)
            .block(Block::default().borders(Borders::ALL).title(title))
            .scroll((scroll_offset as u16, 0));
        f.render_widget(list, content_chunks[1]);
    } else if !app.status_message.is_empty() {
        let status = if app.pending_rx.is_some() {
            format!("{} {}", spinner_char(app.spinner_frame), app.status_message)
        } else {
            app.status_message.clone()
        };
        let msg = Paragraph::new(status)
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("Episodes"));
        f.render_widget(msg, content_chunks[1]);
    } else {
        let empty = Paragraph::new("Enter season number and press Enter to fetch episodes")
            .block(Block::default().borders(Borders::ALL).title("Episodes"));
        f.render_widget(empty, content_chunks[1]);
    }

    let hints = Paragraph::new("Enter: Confirm/Fetch | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan | Ctrl+S: Settings")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn handle_season_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            if app.wizard.list_cursor > 0 {
                app.wizard.list_cursor -= 1;
            }
        }
        KeyCode::Down => {
            let max_scroll = app.tmdb.episodes.len();
            if app.wizard.list_cursor < max_scroll {
                app.wizard.list_cursor += 1;
            }
        }
        KeyCode::Char(c) => {
            if c.is_ascii_digit() {
                app.wizard.input_buffer.push(c);
            }
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

pub fn render_playlist_manager(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let disc_text = disc_label_text(app);
    let show_name = if app.tmdb.show_name.is_empty() {
        app.disc
            .label_info
            .as_ref()
            .map(|l| l.show.as_str())
            .unwrap_or("Unknown")
    } else {
        &app.tmdb.show_name
    };
    let selected_count = app.wizard.playlist_selected.iter().filter(|&&s| s).count();
    let visible = visible_playlists(app);
    let hidden_count = app.disc.playlists.len() - visible.len();

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

    let title = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).title("Playlist Manager"));
    f.render_widget(title, chunks[0]);

    let has_ch = !app.disc.chapter_counts.is_empty();
    let is_tv = !app.tmdb.movie_mode;

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
            let checked = if app.wizard.playlist_selected.get(real_idx).copied().unwrap_or(false) {
                "[x]"
            } else {
                "[ ]"
            };
            let cursor_marker = if vis_idx == app.wizard.list_cursor {
                ">"
            } else {
                " "
            };
            let marker = format!("{} {}", cursor_marker, checked);

            let is_episode_pl = app.disc.episodes_pl.iter().any(|ep| ep.num == pl.num);
            let is_special = app.wizard.specials.contains(&pl.num);
            let is_editing = matches!(app.wizard.input_focus, InputFocus::InlineEdit(r) if r == vis_idx);

            // Episode column
            let ep_str = if is_editing {
                format!("{}|", app.wizard.input_buffer)
            } else if let Some(eps) = app.wizard.episode_assignments.get(&pl.num) {
                if eps.is_empty() {
                    "(none)".to_string()
                } else {
                    let season = if is_special { 0 } else { app.wizard.season_num.unwrap_or(0) };
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

            let special_marker = if is_special { " [S]" } else { "" };
            let ep_display = format!("{}{}", ep_str, special_marker);

            let filename = playlist_filename(app, real_idx, None);
            let ch_str = app
                .disc
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
            } else if vis_idx == app.wizard.list_cursor {
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

    let hints_text = if matches!(app.wizard.input_focus, InputFocus::InlineEdit(_)) {
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

    let status_line = if !app.status_message.is_empty() {
        format!("{}  {}", hints_text, app.status_message)
    } else {
        hints_text
    };
    let hints = Paragraph::new(status_line).style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn handle_playlist_manager_input(app: &mut App, key: KeyEvent) {
    if let InputFocus::InlineEdit(edit_vis_row) = app.wizard.input_focus {
        let visible = visible_playlists(app);
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
                if let Some(&(real_idx, _)) = visible.get(edit_vis_row) {
                    let pl_num = app.disc.playlists[real_idx].num.clone();
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
        KeyCode::Up => {
            if app.wizard.list_cursor > 0 {
                app.wizard.list_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if app.wizard.list_cursor + 1 < vis_len {
                app.wizard.list_cursor += 1;
            }
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
                .filter(|(i, _)| app.wizard.playlist_selected.get(*i).copied().unwrap_or(false))
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

pub fn render_confirm(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let disc_text = disc_label_text(app);
    let header_text = if disc_text.is_empty() {
        format!(
            "Ready to rip {} playlist(s) to {}",
            app.wizard.filenames.len(),
            app.args.output.display(),
        )
    } else {
        format!(
            "{}  |  Ready to rip {} playlist(s) to {}",
            disc_text,
            app.wizard.filenames.len(),
            app.args.output.display(),
        )
    };

    let title = Paragraph::new(header_text).block(
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
        .playlists
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
        "Enter: Exit (dry run) | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan | Ctrl+S: Settings"
    } else {
        "Enter: Start Ripping | Esc: Back | Ctrl+E: Eject | Ctrl+R: Rescan | Ctrl+S: Settings"
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
                .playlists
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
