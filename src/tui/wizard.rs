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
        .filter(|(_, pl)| {
            view.show_filtered
                || view.episodes_pl.iter().any(|ep| ep.num == pl.num)
                || view.detection_results.iter().any(|d| {
                    d.playlist_num == pl.num
                        && d.suggested_type != crate::detection::SuggestedType::Episode
                        && d.confidence >= crate::detection::Confidence::Medium
                })
        })
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

    let num_cols = 4 + if has_ch { 1 } else { 0 } + if is_tv { 1 } else { 0 } + 1;
    let mut rows: Vec<Row> = Vec::new();
    for (vis_idx, &(real_idx, pl)) in visible.iter().enumerate() {
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

        let detection_indicator = view
            .detection_results
            .iter()
            .find(|d| d.playlist_num == pl.num)
            .and_then(|d| match (d.suggested_type, d.confidence) {
                (crate::detection::SuggestedType::Special, crate::detection::Confidence::High) => {
                    Some(" [S!]")
                }
                (
                    crate::detection::SuggestedType::Special,
                    crate::detection::Confidence::Medium,
                ) => Some(" [S?]"),
                (crate::detection::SuggestedType::Special, crate::detection::Confidence::Low) => {
                    Some(" [s.]")
                }
                (
                    crate::detection::SuggestedType::MultiEpisode,
                    crate::detection::Confidence::High,
                ) => Some(" [M!]"),
                (crate::detection::SuggestedType::MultiEpisode, _) => Some(" [M?]"),
                _ => None,
            });
        let special_marker: &str = if is_special {
            " [SP]"
        } else {
            detection_indicator.unwrap_or_default()
        };
        let ep_display = format!("{}{}", ep_str, special_marker);

        let filename = view.filenames.get(&pl.num).cloned().unwrap_or_default();

        let ch_str = if let Some(selections) = view.track_selections.get(&pl.num) {
            if let Some(info) = view.stream_infos.get(&pl.num) {
                let nv = info
                    .video_streams
                    .iter()
                    .filter(|s| selections.contains(&s.index))
                    .count();
                let na = info
                    .audio_streams
                    .iter()
                    .filter(|s| selections.contains(&s.index))
                    .count();
                let ns = info
                    .subtitle_streams
                    .iter()
                    .filter(|s| selections.contains(&s.index))
                    .count();
                format!("{}v {}a {}s*", nv, na, ns)
            } else {
                view.chapter_counts
                    .get(&pl.num)
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            }
        } else {
            view.chapter_counts
                .get(&pl.num)
                .map(|c| c.to_string())
                .unwrap_or_default()
        };

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

        rows.push(Row::new(cells).style(row_style));

        // Track expansion rows
        if view.expanded_playlist == Some(real_idx) {
            if let Some(info) = view.stream_infos.get(&pl.num) {
                let selections = view.track_selections.get(&pl.num);
                let mut sub_idx = 0usize;
                let is_track_edit = matches!(view.input_focus, InputFocus::TrackEdit(_));
                let track_cursor = if let InputFocus::TrackEdit(c) = &view.input_focus {
                    *c
                } else {
                    0
                };

                let make_empty_cells = |label: String, style: Style| -> Row {
                    let mut cells: Vec<String> = vec![String::new(); num_cols];
                    // Put the label in the last column (spans widest)
                    *cells.last_mut().unwrap() = label;
                    Row::new(cells).style(style)
                };

                // Video streams
                if !info.video_streams.is_empty() {
                    rows.push(make_empty_cells(
                        "  VIDEO".into(),
                        Style::default().fg(Color::DarkGray),
                    ));

                    for (type_idx, vs) in info.video_streams.iter().enumerate() {
                        let selected = selections.map(|s| s.contains(&vs.index)).unwrap_or(true);
                        let cursor = is_track_edit && track_cursor == sub_idx;
                        let checkbox = if selected { "[X]" } else { "[ ]" };
                        let label = format!("  {} v{}  {}", checkbox, type_idx, vs.display_line());
                        let style = if cursor {
                            Style::default().fg(Color::Yellow)
                        } else if selected {
                            Style::default().fg(Color::White)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        rows.push(make_empty_cells(label, style));
                        sub_idx += 1;
                    }
                }

                // Audio streams
                if !info.audio_streams.is_empty() {
                    rows.push(make_empty_cells(
                        "  AUDIO".into(),
                        Style::default().fg(Color::DarkGray),
                    ));

                    for (type_idx, audio) in info.audio_streams.iter().enumerate() {
                        let selected = selections.map(|s| s.contains(&audio.index)).unwrap_or(true);
                        let cursor = is_track_edit && track_cursor == sub_idx;
                        let checkbox = if selected { "[X]" } else { "[ ]" };
                        let label =
                            format!("  {} a{}  {}", checkbox, type_idx, audio.display_line());
                        let style = if cursor {
                            Style::default().fg(Color::Yellow)
                        } else if selected {
                            Style::default().fg(Color::White)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        rows.push(make_empty_cells(label, style));
                        sub_idx += 1;
                    }
                }

                // Subtitle streams
                if !info.subtitle_streams.is_empty() {
                    rows.push(make_empty_cells(
                        "  SUBTITLES".into(),
                        Style::default().fg(Color::DarkGray),
                    ));

                    for (type_idx, sub) in info.subtitle_streams.iter().enumerate() {
                        let selected = selections.map(|s| s.contains(&sub.index)).unwrap_or(true);
                        let cursor = is_track_edit && track_cursor == sub_idx;
                        let checkbox = if selected { "[X]" } else { "[ ]" };
                        let label = format!("  {} s{}  {}", checkbox, type_idx, sub.display_line());
                        let style = if cursor {
                            Style::default().fg(Color::Yellow)
                        } else if selected {
                            Style::default().fg(Color::White)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        rows.push(make_empty_cells(label, style));
                        sub_idx += 1;
                    }
                }
            }
        }
    }

    let mut widths = vec![
        Constraint::Length(6),
        Constraint::Length(4),
        Constraint::Length(10),
        Constraint::Length(10),
    ];
    if has_ch {
        widths.push(Constraint::Length(8));
    }
    if is_tv {
        widths.push(Constraint::Min(20));
    }
    widths.push(Constraint::Min(20));

    let table = Table::new(rows, &widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(table, chunks[1]);

    let hints_text = if matches!(view.input_focus, InputFocus::TrackEdit(_)) {
        "Up/Down: Navigate | Space: Toggle | t/Esc: Close tracks".to_string()
    } else if matches!(view.input_focus, InputFocus::InlineEdit(_)) {
        "Enter: Confirm | Esc: Cancel | Format: 3 or 3-4 or 3,5".to_string()
    } else {
        let mut parts = vec!["Space: Toggle", "e: Edit", "t: Tracks"];
        if is_tv {
            parts.push("s: Special");
        }
        let has_suggestions = is_tv
            && view.detection_results.iter().any(|d| {
                d.suggested_type == crate::detection::SuggestedType::Special
                    && d.confidence >= crate::detection::Confidence::Medium
                    && !view.specials.contains(&d.playlist_num)
            });
        if has_suggestions {
            parts.push("A: Accept");
        }
        parts.push("r/R: Reset");
        parts.push("f: Show filtered");
        parts.push("Enter: Confirm");
        parts.push("Esc: Back");
        parts.push("Ctrl+S: Settings");
        parts.join(" | ")
    };

    // Show detection reason for cursor row
    let detection_reason = visible
        .get(view.list_cursor)
        .and_then(|&(_, pl)| {
            view.detection_results
                .iter()
                .find(|d| d.playlist_num == pl.num)
        })
        .filter(|d| d.suggested_type != crate::detection::SuggestedType::Episode)
        .map(|d| d.reasons.join("; "));

    let status_line = match (status.is_empty(), detection_reason) {
        (true, None) => hints_text,
        (true, Some(reason)) => format!("{}  [{}]", hints_text, reason),
        (false, None) => format!("{}  {}", hints_text, status),
        (false, Some(reason)) => format!("{}  {}  [{}]", hints_text, status, reason),
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
        .map(|(pl, name)| {
            total_seconds += pl.seconds;
            let byterate = view
                .media_infos
                .get(&pl.num)
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

/// Run auto-detection if enabled, pre-marking high-confidence specials.
/// Clears detection_results if auto_detect is disabled.
pub fn run_detection_if_enabled(session: &mut crate::session::DriveSession) {
    if session.auto_detect {
        session.wizard.detection_results = crate::detection::run_detection_with_chapters(
            &session.disc.playlists,
            session
                .config
                .min_duration
                .unwrap_or(crate::config::DEFAULT_MIN_DURATION),
            if session.tmdb.episodes.is_empty() {
                None
            } else {
                Some(&session.tmdb.episodes)
            },
            if session.tmdb.specials.is_empty() {
                None
            } else {
                Some(&session.tmdb.specials)
            },
            &session.disc.chapter_counts,
        );
        // Pre-mark high-confidence specials
        for det in &session.wizard.detection_results.clone() {
            if det.suggested_type == crate::detection::SuggestedType::Special
                && det.confidence == crate::detection::Confidence::High
            {
                let pl_num = &det.playlist_num;
                if !session.wizard.specials.contains(pl_num) {
                    let max_sp = session
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
                    session.wizard.specials.insert(pl_num.clone());
                    session.wizard.episode_assignments.insert(
                        pl_num.clone(),
                        vec![crate::types::Episode {
                            episode_number: max_sp + 1,
                            name: String::new(),
                            runtime: None,
                        }],
                    );
                    if let Some(real_idx) =
                        session.disc.playlists.iter().position(|p| p.num == *pl_num)
                    {
                        if let Some(sel) = session.wizard.playlist_selected.get_mut(real_idx) {
                            *sel = true;
                        }
                    }
                }
            }
        }
    } else {
        session.wizard.detection_results.clear();
    }
}

/// Accept all medium+ confidence special suggestions that haven't been accepted yet.
fn accept_detection_suggestions(session: &mut crate::session::DriveSession) {
    for det in &session.wizard.detection_results.clone() {
        if det.suggested_type == crate::detection::SuggestedType::Special
            && det.confidence >= crate::detection::Confidence::Medium
            && !session.wizard.specials.contains(&det.playlist_num)
        {
            let pl_num = det.playlist_num.clone();
            let max_sp = session
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
            session.wizard.specials.insert(pl_num.clone());
            session.wizard.episode_assignments.insert(
                pl_num.clone(),
                vec![crate::types::Episode {
                    episode_number: max_sp + 1,
                    name: String::new(),
                    runtime: None,
                }],
            );
            if let Some(real_idx) = session.disc.playlists.iter().position(|p| p.num == pl_num) {
                if let Some(sel) = session.wizard.playlist_selected.get_mut(real_idx) {
                    *sel = true;
                }
            }
        }
    }
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
                    if !session.tmdb.movie_mode {
                        run_detection_if_enabled(session);
                    }
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
                                    let regular = tmdb::get_season(show_id, season, &api_key);
                                    let specials = if season != 0 {
                                        tmdb::get_season(show_id, 0, &api_key).ok()
                                    } else {
                                        None
                                    };
                                    let _ =
                                        tx.send(BackgroundResult::SeasonFetch(regular, specials));
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
        InputFocus::InlineEdit(_) | InputFocus::TrackEdit(_) => {}
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
                            let regular = tmdb::get_season(show_id, season, &api_key);
                            let specials = if season != 0 {
                                tmdb::get_season(show_id, 0, &api_key).ok()
                            } else {
                                None
                            };
                            let _ = tx.send(BackgroundResult::SeasonFetch(regular, specials));
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
                    run_detection_if_enabled(session);
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
                        let regular = tmdb::get_season(show_id, season, &api_key);
                        let specials = if season != 0 {
                            tmdb::get_season(show_id, 0, &api_key).ok()
                        } else {
                            None
                        };
                        let _ = tx.send(BackgroundResult::SeasonFetch(regular, specials));
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
    if let InputFocus::TrackEdit(sub_row) = session.wizard.input_focus {
        match key.code {
            KeyCode::Up if sub_row > 0 => {
                session.wizard.input_focus = InputFocus::TrackEdit(sub_row - 1);
            }
            KeyCode::Down => {
                if let Some(real_idx) = session.wizard.expanded_playlist {
                    let pl_num = &session.disc.playlists[real_idx].num;
                    if let Some(info) = session.wizard.stream_infos.get(pl_num) {
                        let total = info.video_streams.len()
                            + info.audio_streams.len()
                            + info.subtitle_streams.len();
                        if sub_row + 1 < total {
                            session.wizard.input_focus = InputFocus::TrackEdit(sub_row + 1);
                        }
                    }
                }
            }
            KeyCode::Char(' ') => {
                if let Some(real_idx) = session.wizard.expanded_playlist {
                    let pl_num = session.disc.playlists[real_idx].num.clone();
                    if let Some(info) = session.wizard.stream_infos.get(&pl_num) {
                        let all_indices: Vec<usize> = info
                            .video_streams
                            .iter()
                            .map(|s| s.index)
                            .chain(info.audio_streams.iter().map(|s| s.index))
                            .chain(info.subtitle_streams.iter().map(|s| s.index))
                            .collect();

                        if let Some(&abs_idx) = all_indices.get(sub_row) {
                            let selections = session
                                .wizard
                                .track_selections
                                .entry(pl_num)
                                .or_insert_with(|| all_indices.clone());

                            if let Some(pos) = selections.iter().position(|&i| i == abs_idx) {
                                selections.remove(pos);
                            } else {
                                selections.push(abs_idx);
                                selections.sort_unstable();
                            }
                        }
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char('t') | KeyCode::Char('T') => {
                session.wizard.expanded_playlist = None;
                session.wizard.input_focus = InputFocus::List;
            }
            _ => {}
        }
        return;
    }

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
        KeyCode::Char('A') if !session.tmdb.movie_mode => {
            accept_detection_suggestions(session);
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
        KeyCode::Char('t') | KeyCode::Char('T') => {
            if let Some(&(real_idx, _)) = visible.get(session.wizard.list_cursor) {
                if session.wizard.expanded_playlist == Some(real_idx) {
                    session.wizard.expanded_playlist = None;
                    session.wizard.input_focus = InputFocus::List;
                } else {
                    session.wizard.expanded_playlist = Some(real_idx);
                    let pl_num = &session.disc.playlists[real_idx].num;

                    if !session.wizard.stream_infos.contains_key(pl_num) {
                        let device = session.device.to_string_lossy().to_string();
                        let num = pl_num.clone();
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let mut results = std::collections::HashMap::new();
                            if let Ok((media, streams)) =
                                crate::media::probe::probe_playlist(&device, &num)
                            {
                                results.insert(num, (media, streams));
                            }
                            crate::aacs::kill_makemkvcon_children();
                            let _ = tx.send(crate::types::BackgroundResult::BulkProbe(results));
                        });
                        session.probe_rx = Some(rx);
                        session.status_message = "Probing streams...".into();
                    } else {
                        session.wizard.input_focus = InputFocus::TrackEdit(0);
                    }
                }
            }
        }
        KeyCode::Char('f') => {
            session.wizard.expanded_playlist = None;
            if matches!(session.wizard.input_focus, InputFocus::TrackEdit(_)) {
                session.wizard.input_focus = InputFocus::List;
            }
            session.wizard.show_filtered = !session.wizard.show_filtered;
            let new_visible = session.visible_playlists();
            if session.wizard.list_cursor >= new_visible.len() {
                session.wizard.list_cursor = new_visible.len().saturating_sub(1);
            }
        }
        KeyCode::Enter => {
            let selected_indices: Vec<usize> = session
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
                .map(|(i, _)| i)
                .collect();

            let filenames: Vec<String> = selected_indices
                .iter()
                .map(|&idx| {
                    let pl = &session.disc.playlists[idx];
                    let media_info = session.wizard.media_infos.get(&pl.num);
                    session.playlist_filename(idx, media_info)
                })
                .collect();

            session.wizard.filenames = filenames;

            if session.wizard.filenames.is_empty() {
                session.status_message = "No playlists selected.".into();
            } else {
                // Validate track selections before transitioning
                for &idx in &selected_indices {
                    let pl = &session.disc.playlists[idx];
                    if let Some(selections) = session.wizard.track_selections.get(&pl.num) {
                        if let Some(info) = session.wizard.stream_infos.get(&pl.num) {
                            let errors = crate::streams::validate_track_selection(selections, info);
                            if !errors.is_empty() {
                                session.status_message =
                                    format!("Playlist {}: {}", pl.num, errors.join(", "));
                                return;
                            }
                        }
                    }
                }

                session.wizard.list_cursor = 0;
                session.status_message.clear();
                session.screen = Screen::Confirm;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::collections::HashMap;

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn make_test_stream_info() -> StreamInfo {
        StreamInfo {
            video_streams: vec![VideoStream {
                index: 0,
                codec: "hevc".into(),
                resolution: "1920x1080".into(),
                hdr: "SDR".into(),
                framerate: "23.976".into(),
                bit_depth: "8".into(),
            }],
            audio_streams: vec![
                AudioStream {
                    index: 1,
                    codec: "truehd".into(),
                    channels: 8,
                    channel_layout: "7.1".into(),
                    language: Some("eng".into()),
                    profile: Some("TrueHD".into()),
                },
                AudioStream {
                    index: 2,
                    codec: "ac3".into(),
                    channels: 2,
                    channel_layout: "stereo".into(),
                    language: Some("eng".into()),
                    profile: None,
                },
            ],
            subtitle_streams: vec![SubtitleStream {
                index: 3,
                codec: "hdmv_pgs_subtitle".into(),
                language: Some("eng".into()),
                forced: false,
            }],
        }
    }

    fn make_test_playlist_view() -> PlaylistView {
        let mut stream_infos = HashMap::new();
        stream_infos.insert("00001".to_string(), make_test_stream_info());

        let playlist = Playlist {
            num: "00001".into(),
            duration: "1:00:00".into(),
            seconds: 3600,
            video_streams: 1,
            audio_streams: 2,
            subtitle_streams: 1,
        };

        PlaylistView {
            movie_mode: false,
            show_name: "Test Show".into(),
            season_num: Some(1),
            playlists: vec![playlist.clone()],
            episodes_pl: vec![playlist],
            playlist_selected: vec![true],
            episode_assignments: HashMap::new(),
            specials: std::collections::HashSet::new(),
            show_filtered: false,
            list_cursor: 0,
            input_focus: InputFocus::List,
            input_buffer: String::new(),
            chapter_counts: HashMap::new(),
            episodes: vec![],
            label: "TEST_DISC".into(),
            filenames: HashMap::new(),
            stream_infos,
            track_selections: HashMap::new(),
            expanded_playlist: None,
            detection_results: Vec::new(),
        }
    }

    fn make_test_session() -> crate::session::DriveSession {
        let config = crate::config::Config::default();
        let (_cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let (msg_tx, _msg_rx) = std::sync::mpsc::channel();
        let mut session = crate::session::DriveSession::new(
            std::path::PathBuf::from("/dev/sr0"),
            config,
            crate::streams::StreamFilter::default(),
            cmd_rx,
            msg_tx,
        );
        session.screen = Screen::PlaylistManager;
        session.disc.playlists = vec![Playlist {
            num: "00001".into(),
            duration: "1:00:00".into(),
            seconds: 3600,
            video_streams: 1,
            audio_streams: 2,
            subtitle_streams: 1,
        }];
        session.disc.episodes_pl = session.disc.playlists.clone();
        session.wizard.playlist_selected = vec![true];
        session.wizard.input_focus = InputFocus::List;
        session
            .wizard
            .stream_infos
            .insert("00001".to_string(), make_test_stream_info());
        session
    }

    // --- Rendering tests ---

    #[test]
    fn test_render_playlist_view_no_expansion() {
        let view = make_test_playlist_view();
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_playlist_manager_view(f, &view, "", f.area());
            })
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(
            text.contains("00001"),
            "should show playlist number: {}",
            text
        );
        assert!(text.contains("1:00:00"), "should show duration: {}", text);
        assert!(
            !text.contains("VIDEO"),
            "should not show track sections when collapsed: {}",
            text
        );
    }

    #[test]
    fn test_render_playlist_view_with_expansion() {
        let mut view = make_test_playlist_view();
        view.expanded_playlist = Some(0);
        view.input_focus = InputFocus::TrackEdit(0);

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_playlist_manager_view(f, &view, "", f.area());
            })
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(
            text.contains("VIDEO"),
            "should show VIDEO section: {}",
            text
        );
        assert!(
            text.contains("AUDIO"),
            "should show AUDIO section: {}",
            text
        );
        assert!(
            text.contains("SUBTITLES"),
            "should show SUBTITLES section: {}",
            text
        );
        assert!(text.contains("[X]"), "should show checkboxes: {}", text);
        assert!(
            text.contains("v0"),
            "should show type-local index v0: {}",
            text
        );
        assert!(
            text.contains("a0"),
            "should show type-local index a0: {}",
            text
        );
        assert!(
            text.contains("s0"),
            "should show type-local index s0: {}",
            text
        );
    }

    #[test]
    fn test_render_playlist_view_custom_track_summary() {
        let mut view = make_test_playlist_view();
        // Custom track selections: only video + first audio (indices 0, 1)
        view.track_selections
            .insert("00001".to_string(), vec![0, 1]);
        // Ch column only renders when chapter_counts is non-empty
        view.chapter_counts.insert("00001".to_string(), 5);

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_playlist_manager_view(f, &view, "", f.area());
            })
            .unwrap();
        let text = buffer_text(&terminal);
        // The Ch column is Length(8) so the trailing * may be truncated, but the counts render
        assert!(
            text.contains("1v 1a 0s"),
            "should show custom track summary: {}",
            text
        );
    }

    // --- Input handling tests ---

    #[test]
    fn test_t_expands_playlist() {
        let mut session = make_test_session();
        handle_playlist_manager_input_session(&mut session, make_key(KeyCode::Char('t')));
        assert_eq!(session.wizard.expanded_playlist, Some(0));
        assert!(matches!(
            session.wizard.input_focus,
            InputFocus::TrackEdit(0)
        ));
    }

    #[test]
    fn test_t_collapses_expanded_playlist() {
        let mut session = make_test_session();
        session.wizard.expanded_playlist = Some(0);
        session.wizard.input_focus = InputFocus::TrackEdit(0);
        handle_playlist_manager_input_session(&mut session, make_key(KeyCode::Char('t')));
        assert_eq!(session.wizard.expanded_playlist, None);
        assert_eq!(session.wizard.input_focus, InputFocus::List);
    }

    #[test]
    fn test_space_toggles_track() {
        let mut session = make_test_session();
        session.wizard.expanded_playlist = Some(0);
        session.wizard.input_focus = InputFocus::TrackEdit(0); // cursor on video stream (index 0)
                                                               // Space toggles — should deselect video stream
        handle_playlist_manager_input_session(&mut session, make_key(KeyCode::Char(' ')));
        let selections = session.wizard.track_selections.get("00001").unwrap();
        assert!(
            !selections.contains(&0),
            "video stream should be deselected"
        );
        assert!(
            selections.contains(&1),
            "first audio stream should still be selected"
        );
    }

    #[test]
    fn test_esc_collapses_track_edit() {
        let mut session = make_test_session();
        session.wizard.expanded_playlist = Some(0);
        session.wizard.input_focus = InputFocus::TrackEdit(1);
        handle_playlist_manager_input_session(&mut session, make_key(KeyCode::Esc));
        assert_eq!(session.wizard.expanded_playlist, None);
        assert_eq!(session.wizard.input_focus, InputFocus::List);
    }

    #[test]
    fn test_f_collapses_expansion_before_toggle() {
        let mut session = make_test_session();
        session.wizard.expanded_playlist = Some(0);
        session.wizard.input_focus = InputFocus::List;
        handle_playlist_manager_input_session(&mut session, make_key(KeyCode::Char('f')));
        assert_eq!(
            session.wizard.expanded_playlist, None,
            "f should collapse expansion"
        );
        assert!(
            session.wizard.show_filtered,
            "f should toggle show_filtered"
        );
    }

    #[test]
    fn test_down_navigates_tracks() {
        let mut session = make_test_session();
        session.wizard.expanded_playlist = Some(0);
        session.wizard.input_focus = InputFocus::TrackEdit(0);
        handle_playlist_manager_input_session(&mut session, make_key(KeyCode::Down));
        assert!(matches!(
            session.wizard.input_focus,
            InputFocus::TrackEdit(1)
        ));
    }

    #[test]
    fn test_down_stops_at_last_track() {
        let mut session = make_test_session();
        session.wizard.expanded_playlist = Some(0);
        // Total streams: 1 video + 2 audio + 1 subtitle = 4, last index = 3
        session.wizard.input_focus = InputFocus::TrackEdit(3);
        handle_playlist_manager_input_session(&mut session, make_key(KeyCode::Down));
        assert!(matches!(
            session.wizard.input_focus,
            InputFocus::TrackEdit(3)
        ));
    }

    #[test]
    fn test_up_stops_at_first_track() {
        let mut session = make_test_session();
        session.wizard.expanded_playlist = Some(0);
        session.wizard.input_focus = InputFocus::TrackEdit(0);
        handle_playlist_manager_input_session(&mut session, make_key(KeyCode::Up));
        assert!(matches!(
            session.wizard.input_focus,
            InputFocus::TrackEdit(0)
        ));
    }
}
