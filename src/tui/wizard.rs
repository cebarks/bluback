use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table};

use super::{App, Screen};
use crate::tmdb;
use crate::util::{assign_episodes, guess_start_episode, sanitize_filename};

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

    let title = Paragraph::new("bluback")
        .block(Block::default().borders(Borders::ALL).title("Blu-ray Backup"));
    f.render_widget(title, chunks[0]);

    let body = Paragraph::new(app.status_message.as_str());
    f.render_widget(body, chunks[1]);
}

pub fn render_tmdb_search(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let title = Paragraph::new(format!(
        "Disc: {}  |  {} episode-length playlists",
        if app.label.is_empty() { "(no label)" } else { &app.label },
        app.episodes_pl.len(),
    ))
    .block(Block::default().borders(Borders::ALL).title("Step 1: TMDb Search"));
    f.render_widget(title, chunks[0]);

    if app.api_key.is_none() {
        let content_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Length(3), Constraint::Min(0)])
            .split(chunks[1]);

        let msg = Paragraph::new("No TMDb API key found. Enter your key to enable episode naming:");
        f.render_widget(msg, content_chunks[0]);

        let input = Paragraph::new(format!("{}|", app.input_buffer))
            .block(Block::default().borders(Borders::ALL).title("TMDb API Key"));
        f.render_widget(input, content_chunks[1]);

        let hints = Paragraph::new("Enter: Save key | Esc: Skip TMDb")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hints, chunks[2]);
    } else {
        let mut lines = vec![Line::from(format!("{}|", app.input_buffer))];
        if !app.status_message.is_empty() {
            lines.push(Line::from(app.status_message.as_str()).style(Style::default().fg(Color::Yellow)));
        }
        let input = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Search query"));
        f.render_widget(input, chunks[1]);

        let hints = Paragraph::new("Enter: Search | Esc: Skip TMDb | q: Quit")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hints, chunks[2]);
    }
}

pub fn handle_tmdb_search_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => app.input_buffer.push(c),
        KeyCode::Backspace => { app.input_buffer.pop(); }
        KeyCode::Enter => {
            let input = app.input_buffer.trim().to_string();
            if input.is_empty() {
                return;
            }

            // If no API key yet, treat input as the API key
            if app.api_key.is_none() {
                if let Err(e) = tmdb::save_api_key(&input) {
                    app.status_message = format!("Failed to save API key: {}", e);
                    return;
                }
                app.api_key = Some(input);
                app.input_buffer = app.search_query.clone();
                app.status_message.clear();
                return;
            }

            // Otherwise treat input as search query
            if let Some(ref api_key) = app.api_key.clone() {
                match tmdb::search_show(&input, api_key) {
                    Ok(results) => {
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
                    Err(e) => {
                        app.status_message = format!("TMDb search failed: {}", e);
                    }
                }
            }
        }
        KeyCode::Esc => {
            app.input_active = false;
            app.list_cursor = 0;
            app.screen = Screen::PlaylistSelect;
        }
        _ => {}
    }
}

pub fn render_show_select(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let title = Paragraph::new("Select a show from the search results")
        .block(Block::default().borders(Borders::ALL).title("Step 2: Select Show"));
    f.render_widget(title, chunks[0]);

    let items: Vec<ListItem> = app.search_results.iter().enumerate().map(|(i, show)| {
        let year = show.first_air_date.as_deref()
            .unwrap_or("")
            .get(..4)
            .unwrap_or("");
        let marker = if i == app.list_cursor { "> " } else { "  " };
        ListItem::new(format!("{}{} ({})", marker, show.name, year))
    }).collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Results"))
        .highlight_style(Style::default().fg(Color::Yellow));
    f.render_widget(list, chunks[1]);

    let hints = Paragraph::new("Up/Down: Navigate | Enter: Select | Esc: Back")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn handle_show_select_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            if app.list_cursor > 0 {
                app.list_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if app.list_cursor + 1 < app.search_results.len() {
                app.list_cursor += 1;
            }
        }
        KeyCode::Enter => {
            if app.search_results.is_empty() {
                return;
            }
            app.selected_show = Some(app.list_cursor);
            let show = &app.search_results[app.list_cursor];

            // If we already have a season number, fetch episodes immediately
            if let Some(season) = app.season_num {
                if let Some(ref api_key) = app.api_key.clone() {
                    match tmdb::get_season(show.id, season, api_key) {
                        Ok(eps) => {
                            app.episodes = eps;
                        }
                        Err(e) => {
                            app.status_message = format!("Failed to fetch season: {}", e);
                            app.episodes.clear();
                        }
                    }
                }
            }

            // If episodes already fetched, start on start-episode field
            if !app.episodes.is_empty() {
                app.season_field = 1;
                let disc_num = app.label_info.as_ref().map(|l| l.disc);
                let guessed = guess_start_episode(disc_num, app.episodes_pl.len());
                app.input_buffer = app.start_episode.unwrap_or(guessed).to_string();
            } else {
                app.season_field = 0;
                app.input_buffer = app.season_num.map(|s| s.to_string()).unwrap_or_default();
            }
            app.input_active = true;
            app.list_cursor = 0;
            app.screen = Screen::SeasonEpisode;
        }
        KeyCode::Esc => {
            app.input_buffer = app.search_query.clone();
            app.input_active = true;
            app.list_cursor = 0;
            app.screen = Screen::TmdbSearch;
        }
        _ => {}
    }
}

pub fn render_season_episode(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let show_name = app.selected_show
        .and_then(|i| app.search_results.get(i))
        .map(|s| s.name.as_str())
        .unwrap_or("Unknown");

    let title = Paragraph::new(format!("Show: {}", show_name))
        .block(Block::default().borders(Borders::ALL).title("Step 3: Season & Starting Episode"));
    f.render_widget(title, chunks[0]);

    let content_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(chunks[1]);

    // Season input
    let season_active = app.season_field == 0;
    let season_display = if season_active && app.input_active {
        format!("{}|", app.input_buffer)
    } else {
        app.season_num.map(|s| s.to_string()).unwrap_or_default()
    };
    let season_style = if season_active { Style::default().fg(Color::Yellow) } else { Style::default() };
    let season_input = Paragraph::new(season_display)
        .block(Block::default().borders(Borders::ALL).title("Season number").border_style(season_style));
    f.render_widget(season_input, content_chunks[0]);

    // Start episode input
    let start_active = app.season_field == 1;
    let disc_num = app.label_info.as_ref().map(|l| l.disc);
    let guessed = guess_start_episode(disc_num, app.episodes_pl.len());

    let start_display = if start_active && app.input_active {
        format!("{}|", app.input_buffer)
    } else {
        app.start_episode.unwrap_or(guessed).to_string()
    };
    let start_style = if start_active { Style::default().fg(Color::Yellow) } else { Style::default() };
    let start_input = Paragraph::new(start_display)
        .block(Block::default().borders(Borders::ALL).title(format!("Starting episode (guess: {})", guessed)).border_style(start_style));
    f.render_widget(start_input, content_chunks[1]);

    // Episode list preview
    if !app.episodes.is_empty() {
        let items: Vec<ListItem> = app.episodes.iter().map(|ep| {
            let runtime = ep.runtime.unwrap_or(0);
            ListItem::new(format!("  E{:02} - {} ({} min)", ep.episode_number, ep.name, runtime))
        }).collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(
                format!("Season {}: {} episodes", app.season_num.unwrap_or(0), app.episodes.len())
            ));
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

    let hints = Paragraph::new("Tab: Switch field | Enter: Confirm/Fetch | Esc: Back")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn handle_season_episode_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => {
            if c.is_ascii_digit() {
                app.input_buffer.push(c);
            }
        }
        KeyCode::Backspace => { app.input_buffer.pop(); }
        KeyCode::Tab | KeyCode::BackTab => {
            // Save current field value before switching
            if app.season_field == 0 {
                if let Ok(s) = app.input_buffer.parse::<u32>() {
                    app.season_num = Some(s);
                }
                // Switch to start episode field
                app.season_field = 1;
                let disc_num = app.label_info.as_ref().map(|l| l.disc);
                let guessed = guess_start_episode(disc_num, app.episodes_pl.len());
                app.input_buffer = app.start_episode.unwrap_or(guessed).to_string();
            } else {
                if let Ok(s) = app.input_buffer.parse::<u32>() {
                    app.start_episode = Some(s);
                }
                // Switch to season field
                app.season_field = 0;
                app.input_buffer = app.season_num.map(|s| s.to_string()).unwrap_or_default();
            }
        }
        KeyCode::Enter => {
            if app.season_field == 0 {
                // Entering season number — fetch episodes
                let season: u32 = match app.input_buffer.parse() {
                    Ok(s) => s,
                    _ => return,
                };
                app.season_num = Some(season);

                let show_id = app.selected_show
                    .and_then(|i| app.search_results.get(i))
                    .map(|s| s.id);

                if let (Some(show_id), Some(ref api_key)) = (show_id, app.api_key.clone()) {
                    match tmdb::get_season(show_id, season, &api_key) {
                        Ok(eps) => {
                            app.episodes = eps;
                            app.status_message.clear();
                        }
                        Err(e) => {
                            app.status_message = format!("Failed to fetch season: {}", e);
                            app.episodes.clear();
                        }
                    }
                }

                // Switch to start episode field
                app.season_field = 1;
                let disc_num = app.label_info.as_ref().map(|l| l.disc);
                let guessed = guess_start_episode(disc_num, app.episodes_pl.len());
                app.input_buffer = app.start_episode.unwrap_or(guessed).to_string();
            } else {
                // Entering start episode — confirm and proceed
                let start_ep: u32 = match app.input_buffer.parse() {
                    Ok(s) if s > 0 => s,
                    _ => return,
                };
                app.start_episode = Some(start_ep);

                app.episode_assignments = assign_episodes(
                    &app.episodes_pl,
                    &app.episodes,
                    start_ep,
                );

                app.input_active = false;
                app.input_buffer.clear();
                app.season_field = 0;
                app.list_cursor = 0;
                app.screen = Screen::PlaylistSelect;
            }
        }
        KeyCode::Esc => {
            app.episodes.clear();
            app.input_buffer.clear();
            app.season_field = 0;
            app.list_cursor = 0;
            app.input_active = false;
            app.screen = Screen::ShowSelect;
        }
        _ => {}
    }
}

pub fn render_playlist_select(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let selected_count = app.playlist_selected.iter().filter(|&&s| s).count();
    let title = Paragraph::new(format!(
        "{} playlists ({} selected)",
        app.episodes_pl.len(),
        selected_count,
    ))
    .block(Block::default().borders(Borders::ALL).title("Step 4: Select Playlists"));
    f.render_widget(title, chunks[0]);

    let has_eps = !app.episode_assignments.is_empty();
    let header_cells = if has_eps {
        vec!["", "#", "Playlist", "Duration", "Episode", "Filename"]
    } else {
        vec!["", "#", "Playlist", "Duration", "Filename"]
    };
    let header = Row::new(header_cells).style(Style::default().fg(Color::Yellow));

    let rows: Vec<Row> = app.episodes_pl.iter().enumerate().map(|(i, pl)| {
        let checked = if app.playlist_selected.get(i).copied().unwrap_or(false) {
            "[x]"
        } else {
            "[ ]"
        };
        let cursor = if i == app.list_cursor { ">" } else { " " };
        let marker = format!("{} {}", cursor, checked);

        let ep_info = if let Some(ep) = app.episode_assignments.get(&pl.num) {
            format!("S{:02}E{:02} - {}", app.season_num.unwrap_or(0), ep.episode_number, ep.name)
        } else {
            String::new()
        };

        let filename = if let Some(ep) = app.episode_assignments.get(&pl.num) {
            format!("S{:02}E{:02}_{}.mkv",
                app.season_num.unwrap_or(0),
                ep.episode_number,
                sanitize_filename(&ep.name)
            )
        } else {
            format!("playlist{}.mkv", pl.num)
        };

        if has_eps {
            Row::new(vec![
                marker,
                format!("{}", i + 1),
                pl.num.clone(),
                pl.duration.clone(),
                ep_info,
                filename,
            ])
        } else {
            Row::new(vec![
                marker,
                format!("{}", i + 1),
                pl.num.clone(),
                pl.duration.clone(),
                filename,
            ])
        }
    }).collect();

    let widths = if has_eps {
        vec![
            Constraint::Length(6),
            Constraint::Length(4),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Min(20),
            Constraint::Min(20),
        ]
    } else {
        vec![
            Constraint::Length(6),
            Constraint::Length(4),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Min(20),
        ]
    };

    let table = Table::new(rows, &widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(table, chunks[1]);

    let hints = Paragraph::new("Space: Toggle | Up/Down: Navigate | Enter: Confirm | Esc: Back")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn handle_playlist_select_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            if app.list_cursor > 0 {
                app.list_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if app.list_cursor + 1 < app.episodes_pl.len() {
                app.list_cursor += 1;
            }
        }
        KeyCode::Char(' ') => {
            if let Some(sel) = app.playlist_selected.get_mut(app.list_cursor) {
                *sel = !*sel;
            }
        }
        KeyCode::Enter => {
            // Generate filenames for selected playlists
            app.filenames.clear();
            for (i, pl) in app.episodes_pl.iter().enumerate() {
                if !app.playlist_selected.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let name = if let Some(ep) = app.episode_assignments.get(&pl.num) {
                    format!("S{:02}E{:02}_{}.mkv",
                        app.season_num.unwrap_or(0),
                        ep.episode_number,
                        sanitize_filename(&ep.name)
                    )
                } else {
                    format!("playlist{}.mkv", pl.num)
                };
                app.filenames.push(name);
            }

            if app.filenames.is_empty() {
                app.status_message = "No playlists selected.".into();
                return;
            }

            app.list_cursor = 0;
            app.screen = Screen::Confirm;
        }
        KeyCode::Esc => {
            app.list_cursor = 0;
            if !app.episode_assignments.is_empty() {
                // Go back to season/episode if TMDb was used
                app.input_active = true;
                let disc_num = app.label_info.as_ref().map(|l| l.disc);
                let guessed = app.start_episode.unwrap_or_else(|| {
                    guess_start_episode(disc_num, app.episodes_pl.len())
                });
                app.input_buffer = guessed.to_string();
                app.screen = Screen::SeasonEpisode;
            } else {
                app.input_active = true;
                app.input_buffer = app.search_query.clone();
                app.screen = Screen::TmdbSearch;
            }
        }
        _ => {}
    }
}

pub fn render_confirm(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let title = Paragraph::new(format!(
        "Ready to rip {} playlist(s) to {}",
        app.filenames.len(),
        app.args.output.display(),
    ))
    .block(Block::default().borders(Borders::ALL).title("Step 5: Confirm"));
    f.render_widget(title, chunks[0]);

    let header = Row::new(vec!["Playlist", "Duration", "~Size", "Output File"])
        .style(Style::default().fg(Color::Yellow));

    let selected_playlists: Vec<&crate::types::Playlist> = app.episodes_pl.iter()
        .enumerate()
        .filter(|(i, _)| app.playlist_selected.get(*i).copied().unwrap_or(false))
        .map(|(_, pl)| pl)
        .collect();

    // Estimate ~40 Mbps (5 MB/s) for Blu-ray remux
    const ESTIMATED_BYTERATE: u64 = 5 * 1024 * 1024;

    let mut total_seconds: u32 = 0;
    let mut total_est_bytes: u64 = 0;

    let rows: Vec<Row> = selected_playlists.iter().zip(app.filenames.iter()).map(|(pl, name)| {
        total_seconds += pl.seconds;
        let est_bytes = pl.seconds as u64 * ESTIMATED_BYTERATE;
        total_est_bytes += est_bytes;
        Row::new(vec![
            pl.num.clone(),
            pl.duration.clone(),
            format!("~{}", crate::util::format_size(est_bytes)),
            name.clone(),
        ])
    }).collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Min(30),
    ];

    // Estimate rip time at ~2x read speed (conservative for USB Blu-ray drives)
    let est_rip_secs = total_seconds / 2;
    let rip_h = est_rip_secs / 3600;
    let rip_m = (est_rip_secs % 3600) / 60;
    let content_h = total_seconds / 3600;
    let content_m = (total_seconds % 3600) / 60;
    let summary_title = format!(
        "Summary — ~{}, ~{}h {:02}m content, ~{}h {:02}m to rip",
        crate::util::format_size(total_est_bytes),
        content_h, content_m,
        rip_h, rip_m,
    );

    let table = Table::new(rows, &widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(summary_title));
    f.render_widget(table, chunks[1]);

    let hints = Paragraph::new("Enter: Start Ripping | Esc: Back")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}

pub fn handle_confirm_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            // Build RipJobs from selected playlists and filenames
            app.rip_jobs.clear();

            let selected_playlists: Vec<crate::types::Playlist> = app.episodes_pl.iter()
                .enumerate()
                .filter(|(i, _)| app.playlist_selected.get(*i).copied().unwrap_or(false))
                .map(|(_, pl)| pl.clone())
                .collect();

            for (pl, filename) in selected_playlists.into_iter().zip(app.filenames.iter()) {
                let episode = app.episode_assignments.get(&pl.num).cloned();
                app.rip_jobs.push(crate::types::RipJob {
                    playlist: pl,
                    episode,
                    filename: filename.clone(),
                    status: crate::types::PlaylistStatus::Pending,
                });
            }

            app.current_rip = 0;
            app.screen = Screen::Ripping;
        }
        KeyCode::Esc => {
            app.list_cursor = 0;
            app.screen = Screen::PlaylistSelect;
        }
        _ => {}
    }
}
