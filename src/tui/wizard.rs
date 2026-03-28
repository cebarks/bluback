use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table};
use std::sync::mpsc;

use super::{InputFocus, Screen};
use crate::tmdb;
use crate::types::{
    BackgroundResult, ConfirmView, PlaylistView, ScanningView, SeasonView, TmdbView,
};
use crate::util::{assign_episodes, guess_start_episode, parse_episode_input};

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
                        let marker =
                            if view.input_focus == InputFocus::List && i == view.list_cursor {
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
                        let marker =
                            if view.input_focus == InputFocus::List && i == view.list_cursor {
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

        let toggle = if view.movie_mode { "TV Show" } else { "Movie" };
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
        view.season_num.map(|s| s.to_string()).unwrap_or_default()
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

fn visible_playlists_view(view: &PlaylistView) -> Vec<(usize, &crate::types::Playlist)> {
    view.playlists
        .iter()
        .enumerate()
        .filter(|(_, pl)| view.show_filtered || view.episodes_pl.iter().any(|ep| ep.num == pl.num))
        .collect()
}

pub fn render_playlist_manager_view(f: &mut Frame, view: &PlaylistView, status: &str, area: Rect) {
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
            let is_editing = matches!(view.input_focus, InputFocus::InlineEdit(r) if r == vis_idx);

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

            let filename = view.filenames.get(&pl.num).cloned().unwrap_or_default();
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

pub fn render_confirm_view(f: &mut Frame, view: &ConfirmView, _status: &str, area: Rect) {
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

// --- Session variants of input handlers ---
// These are mechanical ports of the App-based handlers above, operating on
// DriveSession fields instead. The logic is identical.

pub fn handle_tmdb_search_input_session(session: &mut crate::session::DriveSession, key: KeyEvent) {
    match session.wizard.input_focus {
        InputFocus::TextInput => {
            match key.code {
                KeyCode::Char(c) => session.wizard.input_buffer.push(c),
                KeyCode::Backspace => {
                    session.wizard.input_buffer.pop();
                }
                KeyCode::Enter => {
                    let input = session.wizard.input_buffer.trim().to_string();
                    if input.is_empty() {
                        return;
                    }

                    // If no API key yet, treat input as the API key
                    if session.tmdb_api_key.is_none() {
                        if let Err(e) = tmdb::save_api_key(&input) {
                            session.status_message = format!("Failed to save API key: {}", e);
                            return;
                        }
                        session.tmdb_api_key = Some(input);
                        session.wizard.input_buffer = session.tmdb.search_query.clone();
                        session.status_message.clear();
                        return;
                    }

                    // Spawn TMDb search in background thread
                    if let Some(ref api_key) = session.tmdb_api_key.clone() {
                        if session.pending_rx.is_some() {
                            return;
                        }
                        let api_key = api_key.clone();
                        let query = input;
                        let (tx, rx) = mpsc::channel();
                        if session.tmdb.movie_mode {
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
                        session.pending_rx = Some(rx);
                        session.status_message = "Searching TMDb...".into();
                    }
                }
                KeyCode::Down => {
                    let has_results = if session.tmdb.movie_mode {
                        !session.tmdb.movie_results.is_empty()
                    } else {
                        !session.tmdb.search_results.is_empty()
                    };
                    if has_results {
                        session.wizard.input_focus = InputFocus::List;
                        session.wizard.list_cursor = 0;
                    }
                }
                KeyCode::Tab if session.tmdb_api_key.is_some() => {
                    session.tmdb.movie_mode = !session.tmdb.movie_mode;
                }
                KeyCode::Esc => {
                    session.wizard.input_focus = InputFocus::List;
                    session.wizard.list_cursor = 0;
                    session.screen = Screen::PlaylistManager;
                }
                _ => {}
            }
        }
        InputFocus::List => {
            let result_count = if session.tmdb.movie_mode {
                session.tmdb.movie_results.len()
            } else {
                session.tmdb.search_results.len()
            };

            match key.code {
                KeyCode::Up => {
                    if session.wizard.list_cursor == 0 {
                        session.wizard.input_focus = InputFocus::TextInput;
                    } else {
                        session.wizard.list_cursor -= 1;
                    }
                }
                KeyCode::Down if session.wizard.list_cursor + 1 < result_count => {
                    session.wizard.list_cursor += 1;
                }
                KeyCode::Enter => {
                    if result_count == 0 {
                        return;
                    }

                    if session.tmdb.movie_mode {
                        session.tmdb.selected_movie = Some(session.wizard.list_cursor);
                        session.tmdb.show_name = session.tmdb.movie_results
                            [session.wizard.list_cursor]
                            .title
                            .clone();
                        session.wizard.input_focus = InputFocus::List;
                        session.wizard.list_cursor = 0;
                        session.screen = Screen::PlaylistManager;
                    } else {
                        session.tmdb.selected_show = Some(session.wizard.list_cursor);
                        let show = &session.tmdb.search_results[session.wizard.list_cursor];
                        session.tmdb.show_name = show.name.clone();

                        // If we already have a season number, fetch episodes in background
                        if let Some(season) = session.wizard.season_num {
                            if let Some(ref api_key) = session.tmdb_api_key.clone() {
                                let api_key = api_key.clone();
                                let show_id = show.id;
                                let (tx, rx) = mpsc::channel();
                                std::thread::spawn(move || {
                                    let _ = tx.send(BackgroundResult::SeasonFetch(
                                        tmdb::get_season(show_id, season, &api_key),
                                    ));
                                });
                                session.pending_rx = Some(rx);
                                session.status_message = "Fetching season...".into();
                            }
                        }

                        session.wizard.input_buffer = session
                            .wizard
                            .season_num
                            .map(|s| s.to_string())
                            .unwrap_or_default();
                        session.wizard.input_focus = InputFocus::TextInput;
                        session.wizard.list_cursor = 0;
                        session.screen = Screen::Season;
                    }
                }
                KeyCode::Esc => {
                    session.tmdb.search_results.clear();
                    session.tmdb.movie_results.clear();
                    session.wizard.input_focus = InputFocus::TextInput;
                }
                _ => {}
            }
        }
        InputFocus::InlineEdit(_) => {}
    }
}

pub fn handle_season_input_session(session: &mut crate::session::DriveSession, key: KeyEvent) {
    match key.code {
        KeyCode::Up if session.wizard.list_cursor > 0 => {
            session.wizard.list_cursor -= 1;
        }
        KeyCode::Down => {
            let max_scroll = session.tmdb.episodes.len();
            if session.wizard.list_cursor < max_scroll {
                session.wizard.list_cursor += 1;
            }
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            session.wizard.input_buffer.push(c);
        }
        KeyCode::Backspace => {
            session.wizard.input_buffer.pop();
        }
        KeyCode::Enter => {
            if !session.tmdb.episodes.is_empty() {
                let current_input: Option<u32> = session.wizard.input_buffer.parse().ok();
                if current_input != session.wizard.season_num {
                    let season: u32 = match session.wizard.input_buffer.parse() {
                        Ok(s) => s,
                        _ => return,
                    };
                    session.wizard.season_num = Some(season);
                    session.tmdb.episodes.clear();

                    let show_id = session
                        .tmdb
                        .selected_show
                        .and_then(|i| session.tmdb.search_results.get(i))
                        .map(|s| s.id);

                    if let (Some(show_id), Some(ref api_key)) =
                        (show_id, session.tmdb_api_key.clone())
                    {
                        let api_key = api_key.clone();
                        let (tx, rx) = mpsc::channel();
                        std::thread::spawn(move || {
                            let _ = tx.send(BackgroundResult::SeasonFetch(tmdb::get_season(
                                show_id, season, &api_key,
                            )));
                        });
                        session.pending_rx = Some(rx);
                        session.status_message = "Fetching season...".into();
                    }
                } else {
                    let disc_num = session.disc.label_info.as_ref().map(|l| l.disc);
                    let guessed = guess_start_episode(disc_num, session.disc.episodes_pl.len());
                    let start_ep = session.wizard.start_episode.unwrap_or(guessed);
                    session.wizard.episode_assignments = assign_episodes(
                        &session.disc.episodes_pl,
                        &session.tmdb.episodes,
                        start_ep,
                    );
                    session.wizard.input_focus = InputFocus::List;
                    session.wizard.input_buffer.clear();
                    session.wizard.list_cursor = 0;
                    session.screen = Screen::PlaylistManager;
                }
            } else {
                let season: u32 = match session.wizard.input_buffer.parse() {
                    Ok(s) => s,
                    _ => return,
                };
                session.wizard.season_num = Some(season);

                let show_id = session
                    .tmdb
                    .selected_show
                    .and_then(|i| session.tmdb.search_results.get(i))
                    .map(|s| s.id);

                if let (Some(show_id), Some(ref api_key)) = (show_id, session.tmdb_api_key.clone())
                {
                    let api_key = api_key.clone();
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(BackgroundResult::SeasonFetch(tmdb::get_season(
                            show_id, season, &api_key,
                        )));
                    });
                    session.pending_rx = Some(rx);
                    session.status_message = "Fetching season...".into();
                }
            }
        }
        KeyCode::Esc => {
            session.tmdb.episodes.clear();
            session.wizard.input_buffer = session.tmdb.search_query.clone();
            session.wizard.input_focus = InputFocus::TextInput;
            session.wizard.list_cursor = 0;
            session.screen = Screen::TmdbSearch;
        }
        _ => {}
    }
}

pub fn handle_playlist_manager_input_session(
    session: &mut crate::session::DriveSession,
    key: KeyEvent,
) {
    if let InputFocus::InlineEdit(edit_vis_row) = session.wizard.input_focus {
        let visible = session.visible_playlists();
        match key.code {
            KeyCode::Char(c) if (c.is_ascii_digit() || c == ',' || c == '-') => {
                session.wizard.input_buffer.push(c);
            }
            KeyCode::Backspace => {
                session.wizard.input_buffer.pop();
            }
            KeyCode::Enter => {
                if let Some(&(real_idx, _)) = visible.get(edit_vis_row) {
                    let pl_num = session.disc.playlists[real_idx].num.clone();
                    match parse_episode_input(&session.wizard.input_buffer) {
                        Some(ep_nums) if ep_nums.is_empty() => {
                            session.wizard.episode_assignments.remove(&pl_num);
                        }
                        Some(ep_nums) => {
                            let ep_by_num: std::collections::HashMap<u32, &crate::types::Episode> =
                                session
                                    .tmdb
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
                            session.wizard.episode_assignments.insert(pl_num, eps);
                        }
                        None => {
                            return;
                        }
                    }
                }
                session.wizard.input_focus = InputFocus::List;
                session.wizard.input_buffer.clear();
            }
            KeyCode::Esc => {
                session.wizard.input_focus = InputFocus::List;
                session.wizard.input_buffer.clear();
            }
            _ => {}
        }
        return;
    }

    let visible = session.visible_playlists();
    let vis_len = visible.len();

    match key.code {
        KeyCode::Up if session.wizard.list_cursor > 0 => {
            session.wizard.list_cursor -= 1;
        }
        KeyCode::Down if session.wizard.list_cursor + 1 < vis_len => {
            session.wizard.list_cursor += 1;
        }
        KeyCode::Char(' ') => {
            if let Some(&(real_idx, _)) = visible.get(session.wizard.list_cursor) {
                if let Some(sel) = session.wizard.playlist_selected.get_mut(real_idx) {
                    *sel = !*sel;
                }
            }
        }
        KeyCode::Char('e') => {
            if let Some(&(real_idx, _)) = visible.get(session.wizard.list_cursor) {
                let pl_num = &session.disc.playlists[real_idx].num;
                let current = session
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
                session.wizard.input_buffer = current;
                session.wizard.input_focus = InputFocus::InlineEdit(session.wizard.list_cursor);
            }
        }
        KeyCode::Char('s') if !session.tmdb.movie_mode => {
            if let Some(&(real_idx, _)) = visible.get(session.wizard.list_cursor) {
                let pl_num = session.disc.playlists[real_idx].num.clone();
                if session.wizard.specials.contains(&pl_num) {
                    session.wizard.specials.remove(&pl_num);
                    session.wizard.episode_assignments.remove(&pl_num);
                } else {
                    let max_s00_ep = session
                        .wizard
                        .specials
                        .iter()
                        .filter_map(|snum| {
                            session
                                .wizard
                                .episode_assignments
                                .get(snum)
                                .and_then(|eps| eps.first())
                                .map(|e| e.episode_number)
                        })
                        .max()
                        .unwrap_or(0);
                    let next_ep = max_s00_ep + 1;
                    session.wizard.specials.insert(pl_num.clone());
                    session.wizard.episode_assignments.insert(
                        pl_num,
                        vec![crate::types::Episode {
                            episode_number: next_ep,
                            name: String::new(),
                            runtime: None,
                        }],
                    );
                    if let Some(sel) = session.wizard.playlist_selected.get_mut(real_idx) {
                        *sel = true;
                    }
                }
            }
        }
        KeyCode::Char('r') => {
            if let Some(&(real_idx, _)) = visible.get(session.wizard.list_cursor) {
                let pl_num = &session.disc.playlists[real_idx].num;
                session.wizard.episode_assignments.remove(pl_num);
                session.wizard.specials.remove(pl_num);
            }
        }
        KeyCode::Char('R') => {
            session.wizard.episode_assignments.clear();
            session.wizard.specials.clear();
        }
        KeyCode::Char('f') => {
            session.wizard.show_filtered = !session.wizard.show_filtered;
            let new_visible = session.visible_playlists();
            if session.wizard.list_cursor >= new_visible.len() {
                session.wizard.list_cursor = new_visible.len().saturating_sub(1);
            }
        }
        KeyCode::Enter => {
            let selected_nums: Vec<String> = session
                .disc
                .playlists
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    session
                        .wizard
                        .playlist_selected
                        .get(*i)
                        .copied()
                        .unwrap_or(false)
                })
                .map(|(_, pl)| pl.num.clone())
                .collect();

            if selected_nums.is_empty() {
                session.status_message = "No playlists selected.".into();
                return;
            }

            if session.pending_rx.is_some() {
                return;
            }

            let device = session.device.to_string_lossy().to_string();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let infos: Vec<Option<crate::types::MediaInfo>> = selected_nums
                    .iter()
                    .map(|num| crate::disc::probe_media_info(&device, num))
                    .collect();
                let _ = tx.send(BackgroundResult::MediaProbe(infos));
            });
            session.pending_rx = Some(rx);
            session.status_message = "Probing media info...".into();
        }
        KeyCode::Esc => {
            session.wizard.list_cursor = 0;
            if session.tmdb.movie_mode {
                session.wizard.input_focus = InputFocus::List;
                session.screen = Screen::TmdbSearch;
            } else if session.wizard.season_num.is_some() && session.tmdb.selected_show.is_some() {
                session.wizard.input_focus = InputFocus::TextInput;
                session.wizard.input_buffer = session
                    .wizard
                    .season_num
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                session.screen = Screen::Season;
            } else {
                session.wizard.input_focus = InputFocus::TextInput;
                session.wizard.input_buffer = session.tmdb.search_query.clone();
                session.screen = Screen::TmdbSearch;
            }
        }
        _ => {}
    }
}

pub fn handle_confirm_input_session(session: &mut crate::session::DriveSession, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            // DriveSession doesn't support dry_run — always proceed to rip
            session.rip.jobs.clear();

            let selected_playlists: Vec<crate::types::Playlist> = session
                .disc
                .playlists
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    session
                        .wizard
                        .playlist_selected
                        .get(*i)
                        .copied()
                        .unwrap_or(false)
                })
                .map(|(_, pl)| pl.clone())
                .collect();

            for (pl, filename) in selected_playlists
                .into_iter()
                .zip(session.wizard.filenames.iter())
            {
                let episode = session
                    .wizard
                    .episode_assignments
                    .get(&pl.num)
                    .cloned()
                    .unwrap_or_default();
                session.rip.jobs.push(crate::types::RipJob {
                    playlist: pl,
                    episode,
                    filename: filename.clone(),
                    status: crate::types::PlaylistStatus::Pending,
                });
            }

            session.rip.current_rip = 0;
            session.screen = Screen::Ripping;

            let device_str = session.device.to_string_lossy().to_string();
            match crate::disc::ensure_mounted(&device_str) {
                Ok((mount, did_mount)) => {
                    session.disc.mount_point = Some(mount);
                    session.disc.did_mount = did_mount;
                }
                Err(_) => {
                    session.disc.mount_point = None;
                    session.disc.did_mount = false;
                }
            }
        }
        KeyCode::Esc => {
            session.wizard.list_cursor = 0;
            session.screen = Screen::PlaylistManager;
        }
        _ => {}
    }
}
