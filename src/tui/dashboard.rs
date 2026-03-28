use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap};
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use super::Screen;
use crate::rip;
use crate::types::{DashboardView, DoneView, PlaylistStatus};
use crate::util::format_size;

pub fn render_dashboard_view(f: &mut Frame, view: &DashboardView, _status: &str, area: Rect) {
    let done_count = view
        .jobs
        .iter()
        .filter(|j| {
            matches!(
                j.status,
                PlaylistStatus::Done(_) | PlaylistStatus::Skipped(_)
            )
        })
        .count();
    let total = view.jobs.len();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title + stats
            Constraint::Min(4),    // job table
            Constraint::Length(1), // key hints
        ])
        .split(area);

    // Title with stats
    let stats_text = active_rip_stats_view(view);
    let title_text = if stats_text.is_empty() {
        format!("Ripping: {}/{} complete", done_count, total)
    } else {
        format!(
            "Ripping: {}/{} complete  │  {}",
            done_count, total, stats_text
        )
    };
    let block_title = if view.label.is_empty() {
        "bluback".to_string()
    } else {
        format!("bluback — {}", view.label)
    };
    let title =
        Paragraph::new(title_text).block(Block::default().borders(Borders::ALL).title(block_title));
    f.render_widget(title, chunks[0]);

    // Job table
    let header = Row::new(["#", "Playlist", "Episode", "File", "Status", "Size", "ETA"]).style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = view
        .jobs
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let ep_name = if job.episode.is_empty() {
                String::new()
            } else if job.episode.len() == 1 {
                format!(
                    "E{:02} {}",
                    job.episode[0].episode_number, job.episode[0].name
                )
            } else {
                let first = &job.episode[0];
                let last = &job.episode[job.episode.len() - 1];
                format!(
                    "E{:02}-E{:02} {}",
                    first.episode_number, last.episode_number, first.name
                )
            };

            let (status, size, eta) = match &job.status {
                PlaylistStatus::Pending => ("Pending".to_string(), String::new(), String::new()),
                PlaylistStatus::Ripping(prog) => {
                    let pct = if job.playlist.seconds > 0 {
                        (prog.out_time_secs as f64 / job.playlist.seconds as f64 * 100.0).min(100.0)
                            as u32
                    } else {
                        0
                    };
                    let bar = render_progress_bar(pct, 20);
                    let size_str = format_size(prog.total_size);
                    let eta_str = rip::estimate_eta(prog, job.playlist.seconds)
                        .map(rip::format_eta)
                        .unwrap_or_default();
                    (format!("{} {}%", bar, pct), size_str, eta_str)
                }
                PlaylistStatus::Done(sz) => {
                    ("Completed".to_string(), format_size(*sz), String::new())
                }
                PlaylistStatus::Skipped(sz) => (
                    format!("Skipped ({})", format_size(*sz)),
                    String::new(),
                    String::new(),
                ),
                PlaylistStatus::Failed(msg) => {
                    (format!("Failed: {}", msg), String::new(), String::new())
                }
            };

            let row = Row::new([
                if matches!(job.status, PlaylistStatus::Ripping(_)) {
                    ">".to_string()
                } else {
                    format!("{}", i + 1)
                },
                job.playlist.num.clone(),
                ep_name,
                job.filename.clone(),
                status,
                size,
                eta,
            ]);
            match &job.status {
                PlaylistStatus::Ripping(_) => row.style(Style::default().fg(Color::Cyan)),
                PlaylistStatus::Done(_) | PlaylistStatus::Skipped(_) => {
                    row.style(Style::default().fg(Color::DarkGray))
                }
                PlaylistStatus::Failed(_) => row.style(Style::default().fg(Color::Red)),
                _ => row,
            }
        })
        .collect();

    let widths = [
        Constraint::Length(3),
        Constraint::Length(8),
        Constraint::Min(15),
        Constraint::Min(20),
        Constraint::Length(30),
        Constraint::Length(12),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Jobs"));
    f.render_widget(table, chunks[1]);

    // Key hints / confirmation prompts
    let hint = if view.confirm_abort {
        Paragraph::new("Really abort? [y] Yes  [n] No")
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
    } else if view.confirm_rescan {
        Paragraph::new("Rescan disc? This will abort the current rip. [y] Yes  [n] No").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Paragraph::new("[q] Abort  [Ctrl+R] Rescan  [Ctrl+S] Settings")
            .style(Style::default().fg(Color::DarkGray))
    };
    f.render_widget(hint, chunks[2]);
}

pub fn render_done_view(f: &mut Frame, view: &DoneView, area: Rect) {
    let completed: Vec<_> = view
        .jobs
        .iter()
        .filter_map(|j| match &j.status {
            PlaylistStatus::Done(sz) => Some((j.filename.as_str(), *sz)),
            _ => None,
        })
        .collect();
    let skipped: Vec<_> = view
        .jobs
        .iter()
        .filter_map(|j| match &j.status {
            PlaylistStatus::Skipped(sz) => Some((j.filename.as_str(), *sz)),
            _ => None,
        })
        .collect();
    let failed_count = view
        .jobs
        .iter()
        .filter(|j| matches!(j.status, PlaylistStatus::Failed(_)))
        .count();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(area);

    // When showing an error with no rip jobs, put a short summary in the title
    // and the full message in the results body where it can wrap.
    let error_in_body =
        !view.status_message.is_empty() && view.jobs.is_empty() && view.filenames.is_empty();

    let summary = if error_in_body {
        view.status_message
            .split(':')
            .next()
            .expect("split always yields at least one element")
            .to_string()
    } else if !view.status_message.is_empty() {
        view.status_message.clone()
    } else if failed_count > 0 || !skipped.is_empty() {
        let mut parts = vec![format!("{} ripped", completed.len())];
        if !skipped.is_empty() {
            parts.push(format!("{} skipped", skipped.len()));
        }
        if failed_count > 0 {
            parts.push(format!("{} failed", failed_count));
        }
        format!(
            "Completed {} of {} playlist(s) ({})",
            completed.len() + skipped.len(),
            view.jobs.len(),
            parts.join(", ")
        )
    } else {
        format!("All done! Backed up {} playlist(s)", completed.len())
    };

    let title =
        Paragraph::new(summary).block(Block::default().borders(Borders::ALL).title("bluback"));
    f.render_widget(title, chunks[0]);

    let mut lines: Vec<Line> = Vec::new();
    if error_in_body {
        lines.push(
            Line::from(format!("  {}", view.status_message)).style(Style::default().fg(Color::Red)),
        );
    } else if view.jobs.is_empty() && !view.filenames.is_empty() {
        // Dry run: show what would have been ripped
        for name in &view.filenames {
            lines.push(Line::from(format!("  {}", name)));
        }
    } else {
        for (filename, sz) in &completed {
            lines.push(Line::from(format!("  {} ({})", filename, format_size(*sz))));
        }
        for (filename, sz) in &skipped {
            lines.push(
                Line::from(format!("  {} - SKIPPED ({})", filename, format_size(*sz)))
                    .style(Style::default().fg(Color::DarkGray)),
            );
        }
        for job in &view.jobs {
            if let PlaylistStatus::Failed(msg) = &job.status {
                lines.push(
                    Line::from(format!("  {} - FAILED: {}", job.filename, msg))
                        .style(Style::default().fg(Color::Red)),
                );
            }
        }
    }

    let body = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Results"))
        .wrap(Wrap { trim: false });
    f.render_widget(body, chunks[1]);

    let hint = Paragraph::new(
        "[Enter/Ctrl+R] Rescan  [Ctrl+E] Eject  [Ctrl+S] Settings  [any other key] Exit",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[2]);

    if let Some(ref label) = view.disc_detected_label {
        let popup_area = centered_rect(60, 5, area);
        f.render_widget(Clear, popup_area);
        let popup = Paragraph::new(format!(
            "New disc detected: {}\n\nPress Enter to start, any other key to exit",
            label
        ))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title("New Disc"));
        f.render_widget(popup, popup_area);
    }
}

fn render_progress_bar(pct: u32, width: usize) -> String {
    let filled = (pct as usize * width / 100).min(width);
    let empty = width - filled;
    let mut bar = String::with_capacity(width + 2);
    bar.push('[');
    for _ in 0..filled {
        bar.push('#');
    }
    for _ in 0..empty {
        bar.push('-');
    }
    bar.push(']');
    bar
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_width = area.width * percent_x / 100;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect::new(area.x + x, area.y + y, popup_width, height)
}

// --- Session variants of dashboard handlers ---

pub fn handle_input_session(session: &mut crate::session::DriveSession, key: KeyEvent) {
    if session.rip.confirm_abort {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                session.rip.cancel.store(true, Ordering::Relaxed);
                session.rip.progress_rx = None;
                // Session doesn't set quit — the coordinator handles lifecycle
                // Instead, transition to Done so the session reports completion
                session.screen = Screen::Done;
                session.status_message = "Rip aborted.".into();
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                session.rip.confirm_abort = false;
            }
            _ => {}
        }
        return;
    }

    if key.code == KeyCode::Char('q') {
        session.rip.confirm_abort = true;
    }
}

/// Tick the rip engine for a DriveSession. Returns true if state changed.
pub fn tick_session(session: &mut crate::session::DriveSession) -> bool {
    if check_all_done_session(session) {
        return true;
    }

    if session.rip.progress_rx.is_none() {
        let started = start_next_job_session(session);
        return started;
    }

    poll_active_job_session(session)
}

fn check_all_done_session(session: &mut crate::session::DriveSession) -> bool {
    let all_done = session.rip.jobs.iter().all(|j| {
        matches!(
            j.status,
            PlaylistStatus::Done(_) | PlaylistStatus::Skipped(_) | PlaylistStatus::Failed(_)
        )
    });

    if all_done && !session.rip.jobs.is_empty() {
        session.rip.progress_rx = None;
        session.screen = Screen::Done;
        if session.disc.did_mount {
            let _ = crate::disc::unmount_disc(&session.device.to_string_lossy());
            session.disc.did_mount = false;
        }
        // Start scanning for next disc
        session.start_disc_scan();
        session.screen = Screen::Done; // Override start_disc_scan's screen change
        true
    } else {
        false
    }
}

fn start_next_job_session(session: &mut crate::session::DriveSession) -> bool {
    let next_idx = session
        .rip
        .jobs
        .iter()
        .position(|j| matches!(j.status, PlaylistStatus::Pending));

    let Some(idx) = next_idx else {
        return false;
    };

    session.rip.current_rip = idx;
    let job_playlist = session.rip.jobs[idx].playlist.clone();
    let device = session.device.to_string_lossy().to_string();
    let outfile = session.output_dir.join(&session.rip.jobs[idx].filename);
    if let Some(parent) = outfile.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            session.rip.jobs[idx].status = PlaylistStatus::Failed(format!(
                "Failed to create output directory {}: {}",
                parent.display(),
                e
            ));
            return true;
        }
    }

    match crate::workflow::check_overwrite(
        &outfile,
        session.config.overwrite() || session.overwrite,
    ) {
        Ok(crate::workflow::OverwriteAction::Proceed) => {}
        Ok(crate::workflow::OverwriteAction::Skip(size)) => {
            session.rip.jobs[idx].status = PlaylistStatus::Skipped(size);
            return true;
        }
        Ok(crate::workflow::OverwriteAction::DeleteAndProceed(_)) => {}
        Err(e) => {
            session.rip.jobs[idx].status =
                PlaylistStatus::Failed(format!("Overwrite check failed: {}", e));
            return true;
        }
    }

    let stream_selection = session.config.resolve_stream_selection();
    let cancel = session.rip.cancel.clone();
    cancel.store(false, Ordering::Relaxed);

    let metadata = {
        let metadata_enabled = session.config.metadata_enabled() && !session.no_metadata;
        let custom_tags = session.config.metadata_tags();
        let episodes = &session.rip.jobs[idx].episode;
        let date = if session.tmdb.movie_mode {
            session.tmdb.movie_results
                .get(session.tmdb.selected_movie.unwrap_or(0))
                .and_then(|m| m.release_date.as_deref())
        } else {
            session.tmdb.search_results
                .get(session.tmdb.selected_show.unwrap_or(0))
                .and_then(|s| s.first_air_date.as_deref())
        };
        let movie_title = if session.tmdb.movie_mode {
            session.tmdb.movie_results
                .get(session.tmdb.selected_movie.unwrap_or(0))
                .map(|m| m.title.as_str())
        } else {
            None
        };
        crate::workflow::build_metadata(
            metadata_enabled,
            session.tmdb.movie_mode,
            Some(&session.tmdb.show_name).filter(|s| !s.is_empty()).map(|s| s.as_str()),
            session.wizard.season_num,
            episodes,
            movie_title,
            date,
            &custom_tags,
        )
    };

    let options = crate::workflow::prepare_remux_options(
        &device,
        &job_playlist,
        &outfile,
        session.disc.mount_point.as_deref(),
        stream_selection,
        cancel,
        session.config.reserve_index_space(),
        metadata,
    );

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let tx_progress = tx.clone();
        let result = crate::media::remux::remux(options, |progress| {
            let _ = tx_progress.send(Ok(progress.clone()));
        });
        match result {
            Ok(_chapters_added) => {} // success — sender drops, receiver sees Disconnected
            Err(e) => {
                let _ = tx.send(Err(e));
            }
        }
    });

    session.rip.progress_rx = Some(rx);
    session.rip.jobs[idx].status = PlaylistStatus::Ripping(crate::types::RipProgress::default());
    true
}

fn poll_active_job_session(session: &mut crate::session::DriveSession) -> bool {
    let rx = match session.rip.progress_rx {
        Some(ref rx) => rx,
        None => return false,
    };

    let mut changed = false;
    loop {
        match rx.try_recv() {
            Ok(Ok(progress)) => {
                let idx = session.rip.current_rip;
                session.rip.jobs[idx].status = PlaylistStatus::Ripping(progress);
                changed = true;
            }
            Ok(Err(crate::media::MediaError::Cancelled)) => {
                let idx = session.rip.current_rip;
                let outfile = session.output_dir.join(&session.rip.jobs[idx].filename);
                if outfile.exists() {
                    let _ = std::fs::remove_file(&outfile);
                }
                session.rip.jobs[idx].status = PlaylistStatus::Failed("Cancelled".into());
                session.rip.progress_rx = None;
                return true;
            }
            Ok(Err(e)) => {
                let idx = session.rip.current_rip;
                let outfile = session.output_dir.join(&session.rip.jobs[idx].filename);
                if outfile.exists() {
                    let _ = std::fs::remove_file(&outfile);
                }
                session.rip.jobs[idx].status = PlaylistStatus::Failed(e.to_string());
                session.rip.progress_rx = None;
                return true;
            }
            Err(mpsc::TryRecvError::Empty) => {
                return changed;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                let idx = session.rip.current_rip;
                if matches!(session.rip.jobs[idx].status, PlaylistStatus::Ripping(_)) {
                    let outfile = session.output_dir.join(&session.rip.jobs[idx].filename);
                    let file_size = std::fs::metadata(&outfile).map(|m| m.len()).unwrap_or(0);
                    session.rip.jobs[idx].status = PlaylistStatus::Done(file_size);
                }
                session.rip.progress_rx = None;
                return true;
            }
        }
    }
}

fn active_rip_stats_view(view: &DashboardView) -> String {
    if let Some(job) = view.jobs.get(view.current_rip) {
        if let PlaylistStatus::Ripping(ref prog) = job.status {
            let time_str = rip::format_eta(prog.out_time_secs);
            let bitrate_str =
                if prog.bitrate.is_empty() || prog.bitrate == "N/A" || prog.bitrate == "0" {
                    "-".to_string()
                } else {
                    prog.bitrate.clone()
                };
            let speed_str = if prog.speed > 0.0 {
                format!("{:.2}x", prog.speed)
            } else {
                "-".to_string()
            };
            return format!(
                "fps: {:.1}  size: {}  time: {}  bitrate: {}  speed: {}",
                prog.fps,
                format_size(prog.total_size),
                time_str,
                bitrate_str,
                speed_str,
            );
        }
    }
    String::new()
}
