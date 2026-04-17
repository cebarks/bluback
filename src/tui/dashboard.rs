use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap};
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use super::Screen;
use crate::rip;
use crate::types::{DashboardView, DoneView, PlaylistStatus};
use crate::util::format_size;

fn build_post_rip_vars(
    session: &crate::session::DriveSession,
    job_idx: usize,
    status: &str,
    error: &str,
) -> std::collections::HashMap<&'static str, String> {
    let job = &session.rip.jobs[job_idx];
    let file_size = match &job.status {
        crate::types::PlaylistStatus::Done(size)
        | crate::types::PlaylistStatus::Verified(size, _)
        | crate::types::PlaylistStatus::VerifyFailed(size, _) => *size,
        _ => 0,
    };
    let outfile = session.output_dir.join(&job.filename);

    let mut vars = std::collections::HashMap::new();
    vars.insert("file", outfile.display().to_string());
    vars.insert("filename", job.filename.clone());
    vars.insert("dir", session.output_dir.display().to_string());
    vars.insert("size", file_size.to_string());
    vars.insert("chapters", "0".to_string());
    vars.insert(
        "title",
        if session.tmdb.movie_mode {
            session
                .tmdb
                .movie_results
                .get(session.tmdb.selected_movie.unwrap_or(0))
                .map(|m| m.title.clone())
                .unwrap_or_default()
        } else {
            session.tmdb.show_name.clone()
        },
    );
    vars.insert(
        "season",
        session
            .wizard
            .season_num
            .map(|n| n.to_string())
            .unwrap_or_default(),
    );
    vars.insert(
        "episode",
        job.episode
            .first()
            .map(|e| e.episode_number.to_string())
            .unwrap_or_default(),
    );
    vars.insert(
        "episode_name",
        job.episode
            .first()
            .map(|e| e.name.clone())
            .unwrap_or_default(),
    );
    vars.insert("playlist", job.playlist.num.clone());
    vars.insert("label", session.disc.label.clone());
    vars.insert(
        "mode",
        if session.tmdb.movie_mode {
            "movie"
        } else {
            "tv"
        }
        .to_string(),
    );
    vars.insert("device", session.device.display().to_string());
    vars.insert("status", status.to_string());
    vars.insert("error", error.to_string());
    let (verify_status, verify_detail) = match &job.status {
        crate::types::PlaylistStatus::Verified(_, _) => ("passed".to_string(), String::new()),
        crate::types::PlaylistStatus::VerifyFailed(_, ref result) => {
            let detail = result
                .checks
                .iter()
                .filter(|c| !c.passed)
                .map(|c| c.name)
                .collect::<Vec<_>>()
                .join(",");
            ("failed".to_string(), detail)
        }
        _ => ("skipped".to_string(), String::new()),
    };
    vars.insert("verify", verify_status);
    vars.insert("verify_detail", verify_detail);
    vars
}

pub fn render_dashboard_view(f: &mut Frame, view: &DashboardView, _status: &str, area: Rect) {
    let done_count = view
        .jobs
        .iter()
        .filter(|j| {
            matches!(
                j.status,
                PlaylistStatus::Done(_)
                    | PlaylistStatus::Verified(..)
                    | PlaylistStatus::VerifyFailed(..)
                    | PlaylistStatus::Skipped(_)
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
    let block_title = if view.batch_disc_count > 0 {
        if view.label.is_empty() {
            format!("bluback \u{2014} Disc {} | Batch", view.batch_disc_count)
        } else {
            format!(
                "bluback \u{2014} {} \u{2014} Disc {} | Batch",
                view.label, view.batch_disc_count
            )
        }
    } else if view.label.is_empty() {
        "bluback".to_string()
    } else {
        format!("bluback \u{2014} {}", view.label)
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

            let est_str = format!("~{}", format_size(job.estimated_size));

            let (status, size, eta) = match &job.status {
                PlaylistStatus::Pending => ("Pending".to_string(), est_str, String::new()),
                PlaylistStatus::Ripping(prog) => {
                    // Use the actual stream duration from FFmpeg when available,
                    // falling back to the playlist duration from libbluray log
                    let duration = if prog.duration_secs > 0 {
                        prog.duration_secs
                    } else {
                        job.playlist.seconds
                    };
                    let pct = if duration > 0 {
                        (prog.out_time_secs as f64 / duration as f64 * 100.0).min(100.0) as u32
                    } else {
                        0
                    };
                    let bar = render_progress_bar(pct, 20);
                    let size_str = format!("{}/{}", format_size(prog.total_size), est_str);
                    let eta_str = rip::estimate_eta(prog, duration)
                        .map(rip::format_eta)
                        .unwrap_or_default();
                    (format!("{} {}%", bar, pct), size_str, eta_str)
                }
                PlaylistStatus::Verifying => ("Verifying...".to_string(), est_str, String::new()),
                PlaylistStatus::Done(sz) => {
                    ("Completed".to_string(), format_size(*sz), String::new())
                }
                PlaylistStatus::Verified(sz, _) => {
                    ("Verified".to_string(), format_size(*sz), String::new())
                }
                PlaylistStatus::VerifyFailed(sz, _) => {
                    ("Verify failed".to_string(), format_size(*sz), String::new())
                }
                PlaylistStatus::Skipped(sz) => (
                    format!("Skipped ({})", format_size(*sz)),
                    est_str,
                    String::new(),
                ),
                PlaylistStatus::Failed(msg) => (format!("Failed: {}", msg), est_str, String::new()),
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
                PlaylistStatus::Pending => row,
                PlaylistStatus::Ripping(_) => row.style(Style::default().fg(Color::Cyan)),
                PlaylistStatus::Verifying => row.style(Style::default().fg(Color::Yellow)),
                PlaylistStatus::Done(_)
                | PlaylistStatus::Verified(..)
                | PlaylistStatus::Skipped(_) => row.style(Style::default().fg(Color::DarkGray)),
                PlaylistStatus::VerifyFailed(..) => row.style(Style::default().fg(Color::Yellow)),
                PlaylistStatus::Failed(_) => row.style(Style::default().fg(Color::Red)),
            }
        })
        .collect();

    let widths = [
        Constraint::Length(3),
        Constraint::Length(8),
        Constraint::Min(15),
        Constraint::Min(20),
        Constraint::Length(30),
        Constraint::Length(22),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Jobs"));
    f.render_widget(table, chunks[1]);

    // Key hints / confirmation prompts
    if let Some(fail_idx) = view.verify_failed_idx {
        if let Some(job) = view.jobs.get(fail_idx) {
            if let PlaylistStatus::VerifyFailed(_, ref result) = job.status {
                let failed_details: Vec<&str> = result
                    .checks
                    .iter()
                    .filter(|c| !c.passed)
                    .map(|c| c.detail.as_str())
                    .collect();
                let msg = format!(
                    "{}: {}  [D]elete & retry  [K]eep  [S]kip",
                    job.filename,
                    failed_details.join("; ")
                );
                let hint = Paragraph::new(msg).style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
                f.render_widget(hint, chunks[2]);
                return;
            }
        }
    }

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
            PlaylistStatus::Done(sz) | PlaylistStatus::Verified(sz, _) => {
                Some((j.filename.as_str(), *sz))
            }
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
    let verify_failed_count = view
        .jobs
        .iter()
        .filter(|j| matches!(j.status, PlaylistStatus::VerifyFailed(..)))
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
    } else if failed_count > 0 || verify_failed_count > 0 || !skipped.is_empty() {
        let mut parts = vec![format!("{} ripped", completed.len())];
        if !skipped.is_empty() {
            parts.push(format!("{} skipped", skipped.len()));
        }
        if failed_count > 0 {
            parts.push(format!("{} failed", failed_count));
        }
        if verify_failed_count > 0 {
            parts.push(format!("{} verify failed", verify_failed_count));
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

    let done_block_title = if view.batch_disc_count > 0 {
        if view.label.is_empty() {
            format!("bluback \u{2014} Disc {} | Batch", view.batch_disc_count)
        } else {
            format!(
                "bluback \u{2014} {} \u{2014} Disc {} | Batch",
                view.label, view.batch_disc_count
            )
        }
    } else if view.label.is_empty() {
        "bluback".to_string()
    } else {
        format!("bluback \u{2014} {}", view.label)
    };
    let title = Paragraph::new(summary).block(
        Block::default()
            .borders(Borders::ALL)
            .title(done_block_title),
    );
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
            if let PlaylistStatus::VerifyFailed(sz, ref result) = job.status {
                let failed_details: Vec<String> = result
                    .checks
                    .iter()
                    .filter(|c| !c.passed)
                    .map(|c| format!("{}: {}", c.name, c.detail))
                    .collect();
                lines.push(
                    Line::from(format!(
                        "  {} ({}) - VERIFY FAILED: {}",
                        job.filename,
                        format_size(sz),
                        failed_details.join("; ")
                    ))
                    .style(Style::default().fg(Color::Red)),
                );
            }
        }
    }

    if view.history_session_saved {
        lines.push(Line::from(""));
        lines.push(
            Line::from("  Session saved to history.").style(Style::default().fg(Color::DarkGray)),
        );
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

/// Record a file status event in history. Best-effort: logs and continues on error.
fn record_file_event(
    session: &crate::session::DriveSession,
    history_db: &Option<crate::history::HistoryDb>,
    job_idx: usize,
    status: crate::history::FileStatus,
    error: Option<&str>,
) {
    let (db, sid) = match (history_db, session.history_session_id) {
        (Some(db), Some(sid)) => (db, sid),
        _ => return,
    };
    let job = &session.rip.jobs[job_idx];

    // Build episode JSON from assignments
    let episodes_json = session
        .wizard
        .episode_assignments
        .get(&job.playlist.num)
        .map(|eps| {
            serde_json::to_string(&eps.iter().map(|e| e.episode_number).collect::<Vec<_>>())
                .unwrap_or_default()
        });

    let file_size = match &job.status {
        crate::types::PlaylistStatus::Done(sz)
        | crate::types::PlaylistStatus::Verified(sz, _)
        | crate::types::PlaylistStatus::VerifyFailed(sz, _) => Some(*sz as i64),
        crate::types::PlaylistStatus::Skipped(sz) => Some(*sz as i64),
        _ => None,
    };

    let outfile = session.output_dir.join(&job.filename);
    let file_info = crate::history::RippedFileInfo {
        playlist: job.playlist.num.clone(),
        episodes: episodes_json,
        output_path: outfile.display().to_string(),
        file_size,
        duration_ms: Some((job.playlist.seconds as i64) * 1000),
        streams: None,
        chapters: None,
    };

    match db.record_file(sid, &file_info) {
        Ok(fid) => {
            if let Err(e) = db.update_file_status(fid, status, error) {
                log::warn!("history: failed to update file status: {}", e);
            }
        }
        Err(e) => {
            log::warn!("history: failed to record file: {}", e);
        }
    }
}

pub fn handle_input_session(session: &mut crate::session::DriveSession, key: KeyEvent) {
    if let Some(fail_idx) = session.rip.verify_failed_idx {
        match key.code {
            KeyCode::Char('d') | KeyCode::Char('D') => {
                // Delete and retry
                let outfile = session
                    .output_dir
                    .join(&session.rip.jobs[fail_idx].filename);
                let _ = std::fs::remove_file(&outfile);
                session.rip.jobs[fail_idx].status = PlaylistStatus::Pending;
                session.rip.verify_failed_idx = None;
            }
            KeyCode::Char('k') | KeyCode::Char('K') => {
                // Keep as-is
                if let PlaylistStatus::VerifyFailed(sz, _) = &session.rip.jobs[fail_idx].status {
                    session.rip.jobs[fail_idx].status = PlaylistStatus::Done(*sz);
                }
                session.rip.verify_failed_idx = None;
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                // Skip (delete file)
                let outfile = session
                    .output_dir
                    .join(&session.rip.jobs[fail_idx].filename);
                let _ = std::fs::remove_file(&outfile);
                session.rip.jobs[fail_idx].status = PlaylistStatus::Skipped(0);
                session.rip.verify_failed_idx = None;
            }
            _ => {}
        }
        return;
    }

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
pub fn tick_session(
    session: &mut crate::session::DriveSession,
    history_db: &Option<crate::history::HistoryDb>,
) -> bool {
    if check_all_done_session(session, history_db) {
        return true;
    }

    if session.rip.progress_rx.is_none() {
        let started = start_next_job_session(session, history_db);
        return started;
    }

    poll_active_job_session(session, history_db)
}

fn check_all_done_session(
    session: &mut crate::session::DriveSession,
    history_db: &Option<crate::history::HistoryDb>,
) -> bool {
    let all_done = session.rip.jobs.iter().all(|j| {
        matches!(
            j.status,
            PlaylistStatus::Done(_)
                | PlaylistStatus::Verified(..)
                | PlaylistStatus::VerifyFailed(..)
                | PlaylistStatus::Skipped(_)
                | PlaylistStatus::Failed(_)
        )
    });

    if all_done && !session.rip.jobs.is_empty() {
        session.rip.progress_rx = None;
        session.screen = Screen::Done;

        // Finish history session
        if let (Some(db), Some(sid)) = (history_db, session.history_session_id) {
            let has_success = session.rip.jobs.iter().any(|j| {
                matches!(
                    j.status,
                    PlaylistStatus::Done(_) | PlaylistStatus::Verified(..)
                )
            });
            let status = if has_success {
                crate::history::SessionStatus::Completed
            } else {
                crate::history::SessionStatus::Failed
            };
            if let Err(e) = db.finish_session(sid, status) {
                log::warn!("history: failed to finish session: {}", e);
            } else {
                session.history_session_saved = true;
            }
            // Clear session ID so rescan starts a fresh session
            session.history_session_id = None;
        }
        if session.disc.did_mount {
            let _ = crate::disc::unmount_disc(&session.device.to_string_lossy());
            session.disc.did_mount = false;
        }

        // Post-session hook
        {
            let (succeeded, failed, skipped) =
                session
                    .rip
                    .jobs
                    .iter()
                    .fold((0u32, 0u32, 0u32), |(s, f, sk), j| match j.status {
                        PlaylistStatus::Done(_) | PlaylistStatus::Verified(..) => (s + 1, f, sk),
                        PlaylistStatus::Failed(_) | PlaylistStatus::VerifyFailed(..) => {
                            (s, f + 1, sk)
                        }
                        PlaylistStatus::Skipped(_) => (s, f, sk + 1),
                        _ => (s, f, sk),
                    });
            let total = succeeded + failed + skipped;
            let mut vars = std::collections::HashMap::new();
            vars.insert(
                "title",
                if session.tmdb.movie_mode {
                    session
                        .tmdb
                        .movie_results
                        .get(session.tmdb.selected_movie.unwrap_or(0))
                        .map(|m| m.title.clone())
                        .unwrap_or_default()
                } else {
                    session.tmdb.show_name.clone()
                },
            );
            vars.insert(
                "season",
                session
                    .wizard
                    .season_num
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
            );
            vars.insert("label", session.disc.label.clone());
            vars.insert("device", session.device.display().to_string());
            vars.insert(
                "mode",
                if session.tmdb.movie_mode {
                    "movie"
                } else {
                    "tv"
                }
                .to_string(),
            );
            vars.insert("dir", session.output_dir.display().to_string());
            vars.insert("total", total.to_string());
            vars.insert("succeeded", succeeded.to_string());
            vars.insert("failed", failed.to_string());
            vars.insert("skipped", skipped.to_string());
            crate::hooks::run_post_session(&session.config, &vars, session.no_hooks);
        }

        // Auto-eject in batch mode
        if session.batch {
            let device = session.device.to_string_lossy();
            log::info!("Batch mode: ejecting disc {}", device);
            if let Err(e) = crate::disc::eject_disc(&device) {
                log::warn!("Failed to eject disc: {}", e);
                session.status_message = format!("Eject failed: {}", e);
            }
        }

        // Start scanning for next disc
        session.start_disc_scan();
        session.screen = Screen::Done; // Override start_disc_scan's screen change
        true
    } else {
        false
    }
}

fn start_next_job_session(
    session: &mut crate::session::DriveSession,
    history_db: &Option<crate::history::HistoryDb>,
) -> bool {
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

    let estimated_size = session.rip.jobs[idx].estimated_size;
    match crate::workflow::check_overwrite(
        &outfile,
        session.config.overwrite() || session.overwrite,
        Some(estimated_size).filter(|&s| s > 0),
    ) {
        Ok(crate::workflow::OverwriteAction::Proceed) => {}
        Ok(crate::workflow::OverwriteAction::Skip(size)) => {
            session.rip.jobs[idx].status = PlaylistStatus::Skipped(size);
            // Record skip in history
            record_file_event(
                session,
                history_db,
                idx,
                crate::history::FileStatus::Skipped,
                None,
            );
            return true;
        }
        Ok(crate::workflow::OverwriteAction::PartialReplace(size)) => {
            session.status_message = format!(
                "Re-ripping partial file {} (was {})",
                session.rip.jobs[idx].filename,
                format_size(size),
            );
        }
        Ok(crate::workflow::OverwriteAction::DeleteAndProceed(_)) => {}
        Err(e) => {
            session.rip.jobs[idx].status =
                PlaylistStatus::Failed(format!("Overwrite check failed: {}", e));
            return true;
        }
    }

    // Per-playlist stream selection: manual track picks > stream filter > all
    let stream_selection =
        if let Some(indices) = session.wizard.track_selections.get(&job_playlist.num) {
            crate::media::StreamSelection::Manual(indices.clone())
        } else if !session.stream_filter.is_empty() {
            if let Some(info) = session.wizard.stream_infos.get(&job_playlist.num) {
                crate::media::StreamSelection::Manual(session.stream_filter.apply(info))
            } else {
                crate::media::StreamSelection::All
            }
        } else {
            crate::media::StreamSelection::All
        };
    let cancel = session.rip.cancel.clone();
    cancel.store(false, Ordering::Relaxed);

    let metadata = {
        let metadata_enabled = session.config.metadata_enabled() && !session.no_metadata;
        let custom_tags = session.config.metadata_tags();
        let episodes = &session.rip.jobs[idx].episode;
        let date = if session.tmdb.movie_mode {
            session
                .tmdb
                .movie_results
                .get(session.tmdb.selected_movie.unwrap_or(0))
                .and_then(|m| m.release_date.as_deref())
        } else {
            session
                .tmdb
                .search_results
                .get(session.tmdb.selected_show.unwrap_or(0))
                .and_then(|s| s.first_air_date.as_deref())
        };
        let movie_title = if session.tmdb.movie_mode {
            session
                .tmdb
                .movie_results
                .get(session.tmdb.selected_movie.unwrap_or(0))
                .map(|m| m.title.as_str())
        } else {
            None
        };
        crate::workflow::build_metadata(
            metadata_enabled,
            session.tmdb.movie_mode,
            Some(&session.tmdb.show_name)
                .filter(|s| !s.is_empty())
                .map(|s| s.as_str()),
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

    // Mark session as in_progress and record file start in history
    if let (Some(db), Some(sid)) = (history_db, session.history_session_id) {
        // Mark in_progress on first rip job only
        if idx == 0 {
            if let Err(e) = db.finish_session(sid, crate::history::SessionStatus::InProgress) {
                log::warn!("history: failed to mark session in_progress: {}", e);
            }
        }
        // Record file start
        let episodes_json = session
            .wizard
            .episode_assignments
            .get(&job_playlist.num)
            .map(|eps| {
                serde_json::to_string(&eps.iter().map(|e| e.episode_number).collect::<Vec<_>>())
                    .unwrap_or_default()
            });
        let file_info = crate::history::RippedFileInfo {
            playlist: job_playlist.num.clone(),
            episodes: episodes_json,
            output_path: outfile.display().to_string(),
            file_size: None,
            duration_ms: Some((job_playlist.seconds as i64) * 1000),
            streams: None,
            chapters: None,
        };
        match db.record_file(sid, &file_info) {
            Ok(_fid) => {}
            Err(e) => log::warn!("history: failed to record file: {}", e),
        }
    }

    session.rip.chapters_added.store(0, Ordering::Relaxed);
    let chapters_added_arc = session.rip.chapters_added.clone();

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let tx_progress = tx.clone();
        let result = crate::media::remux::remux(options, |progress| {
            let _ = tx_progress.send(Ok(progress.clone()));
        });

        match result {
            Ok(added) => {
                chapters_added_arc.store(added, std::sync::atomic::Ordering::Relaxed);
            }
            Err(e) => {
                let _ = tx.send(Err(e));
            }
        }
    });

    session.rip.progress_rx = Some(rx);
    session.rip.jobs[idx].status = PlaylistStatus::Ripping(crate::types::RipProgress::default());
    true
}

fn poll_active_job_session(
    session: &mut crate::session::DriveSession,
    history_db: &Option<crate::history::HistoryDb>,
) -> bool {
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
                record_file_event(
                    session,
                    history_db,
                    idx,
                    crate::history::FileStatus::Failed,
                    Some("Cancelled"),
                );
                let vars = build_post_rip_vars(session, idx, "failed", "Cancelled");
                crate::hooks::run_post_rip(&session.config, &vars, session.no_hooks);
                return true;
            }
            Ok(Err(e)) => {
                let idx = session.rip.current_rip;
                let err_msg = e.to_string();
                let outfile = session.output_dir.join(&session.rip.jobs[idx].filename);
                if outfile.exists() {
                    let _ = std::fs::remove_file(&outfile);
                }
                session.rip.jobs[idx].status = PlaylistStatus::Failed(err_msg.clone());
                session.rip.progress_rx = None;
                record_file_event(
                    session,
                    history_db,
                    idx,
                    crate::history::FileStatus::Failed,
                    Some(&err_msg),
                );
                let vars = build_post_rip_vars(session, idx, "failed", &err_msg);
                crate::hooks::run_post_rip(&session.config, &vars, session.no_hooks);
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

                    if session.verify {
                        let playlist = &session.rip.jobs[idx].playlist;
                        let chapters = session
                            .rip
                            .chapters_added
                            .load(std::sync::atomic::Ordering::Relaxed);

                        // Compute expected stream counts accounting for manual
                        // stream selection (output may have fewer streams than source)
                        let (exp_video, exp_audio, exp_subtitle) = if let Some(indices) =
                            session.wizard.track_selections.get(&playlist.num)
                        {
                            if let Some(info) = session.wizard.stream_infos.get(&playlist.num) {
                                crate::streams::count_selected_streams(indices, info)
                            } else {
                                (
                                    playlist.video_streams,
                                    playlist.audio_streams,
                                    playlist.subtitle_streams,
                                )
                            }
                        } else if !session.stream_filter.is_empty() {
                            if let Some(info) = session.wizard.stream_infos.get(&playlist.num) {
                                let indices = session.stream_filter.apply(info);
                                crate::streams::count_selected_streams(&indices, info)
                            } else {
                                (
                                    playlist.video_streams,
                                    playlist.audio_streams,
                                    playlist.subtitle_streams,
                                )
                            }
                        } else {
                            (
                                playlist.video_streams,
                                playlist.audio_streams,
                                playlist.subtitle_streams,
                            )
                        };

                        let expected = crate::verify::VerifyExpected {
                            duration_secs: playlist.seconds,
                            video_streams: exp_video,
                            audio_streams: exp_audio,
                            subtitle_streams: exp_subtitle,
                            chapters,
                        };
                        let result =
                            crate::verify::verify_output(&outfile, &expected, session.verify_level);
                        if result.passed {
                            session.rip.jobs[idx].status =
                                PlaylistStatus::Verified(file_size, result);
                        } else {
                            let detail: String = result
                                .checks
                                .iter()
                                .filter(|c| !c.passed)
                                .map(|c| c.detail.clone())
                                .collect::<Vec<_>>()
                                .join("; ");
                            log::warn!(
                                "Verification failed for {}: {}",
                                session.rip.jobs[idx].filename,
                                detail
                            );
                            session.rip.jobs[idx].status =
                                PlaylistStatus::VerifyFailed(file_size, result);
                            session.rip.verify_failed_idx = Some(idx);
                        }
                    } else {
                        session.rip.jobs[idx].status = PlaylistStatus::Done(file_size);
                    }
                    // Record file completion in history
                    record_file_event(
                        session,
                        history_db,
                        idx,
                        crate::history::FileStatus::Completed,
                        None,
                    );
                    let vars = build_post_rip_vars(session, idx, "success", "");
                    crate::hooks::run_post_rip(&session.config, &vars, session.no_hooks);
                }
                session.rip.progress_rx = None;
                return true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PlaylistStatus;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn make_test_session_with_verify_failure() -> crate::session::DriveSession {
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
        session.screen = Screen::Ripping;
        session.output_dir = std::env::temp_dir();

        // Set up a job in VerifyFailed state
        let verify_result = crate::verify::VerifyResult {
            passed: false,
            level: crate::verify::VerifyLevel::Quick,
            checks: vec![crate::verify::VerifyCheck {
                name: "duration",
                passed: false,
                detail: "expected 3600s, got 3000s".into(),
            }],
        };
        session.rip.jobs = vec![crate::types::RipJob {
            playlist: crate::types::Playlist {
                num: "00001".into(),
                duration: "1:00:00".into(),
                seconds: 3600,
                video_streams: 1,
                audio_streams: 2,
                subtitle_streams: 3,
            },
            episode: vec![],
            filename: "test_verify.mkv".into(),
            status: PlaylistStatus::VerifyFailed(1_000_000, verify_result),
            estimated_size: 9_000_000,
        }];
        session.rip.verify_failed_idx = Some(0);
        session
    }

    #[test]
    fn test_verify_prompt_keep() {
        let mut session = make_test_session_with_verify_failure();
        handle_input_session(&mut session, make_key(KeyCode::Char('k')));
        assert!(session.rip.verify_failed_idx.is_none());
        assert!(matches!(
            session.rip.jobs[0].status,
            PlaylistStatus::Done(1_000_000)
        ));
    }

    #[test]
    fn test_verify_prompt_keep_uppercase() {
        let mut session = make_test_session_with_verify_failure();
        handle_input_session(&mut session, make_key(KeyCode::Char('K')));
        assert!(session.rip.verify_failed_idx.is_none());
        assert!(matches!(
            session.rip.jobs[0].status,
            PlaylistStatus::Done(1_000_000)
        ));
    }

    #[test]
    fn test_verify_prompt_delete_resets_to_pending() {
        let mut session = make_test_session_with_verify_failure();
        handle_input_session(&mut session, make_key(KeyCode::Char('d')));
        assert!(session.rip.verify_failed_idx.is_none());
        assert!(matches!(
            session.rip.jobs[0].status,
            PlaylistStatus::Pending
        ));
    }

    #[test]
    fn test_verify_prompt_skip() {
        let mut session = make_test_session_with_verify_failure();
        handle_input_session(&mut session, make_key(KeyCode::Char('s')));
        assert!(session.rip.verify_failed_idx.is_none());
        assert!(matches!(
            session.rip.jobs[0].status,
            PlaylistStatus::Skipped(0)
        ));
    }

    #[test]
    fn test_verify_prompt_ignores_other_keys() {
        let mut session = make_test_session_with_verify_failure();
        handle_input_session(&mut session, make_key(KeyCode::Char('x')));
        assert!(session.rip.verify_failed_idx.is_some()); // Still showing prompt
        assert!(matches!(
            session.rip.jobs[0].status,
            PlaylistStatus::VerifyFailed(..)
        ));
    }

    #[test]
    fn test_verify_prompt_blocks_abort_confirmation() {
        let mut session = make_test_session_with_verify_failure();
        // 'q' should be swallowed by verify prompt, not trigger abort
        handle_input_session(&mut session, make_key(KeyCode::Char('q')));
        assert!(!session.rip.confirm_abort);
        assert!(session.rip.verify_failed_idx.is_some());
    }

    // =========================================================================
    // Rendering tests — dashboard and done views via TestBackend
    // =========================================================================

    use crate::types::{DashboardView, DoneView, Episode, Playlist, RipJob, RipProgress};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                text.push_str(cell.symbol());
            }
            text.push('\n');
        }
        text
    }

    fn render_dashboard(view: &DashboardView) -> String {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_dashboard_view(f, view, "", f.area());
            })
            .unwrap();
        buffer_text(&terminal)
    }

    fn render_done(view: &DoneView) -> String {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_done_view(f, view, f.area());
            })
            .unwrap();
        buffer_text(&terminal)
    }

    fn make_playlist() -> Playlist {
        Playlist {
            num: "00001".into(),
            duration: "1:00:00".into(),
            seconds: 3600,
            video_streams: 1,
            audio_streams: 2,
            subtitle_streams: 3,
        }
    }

    fn make_job(status: PlaylistStatus) -> RipJob {
        RipJob {
            playlist: make_playlist(),
            episode: vec![Episode {
                episode_number: 1,
                name: "Pilot".into(),
                runtime: None,
            }],
            filename: "S01E01_Pilot.mkv".into(),
            status,
            estimated_size: 9_000_000,
        }
    }

    fn default_dashboard_view(jobs: Vec<RipJob>) -> DashboardView {
        DashboardView {
            jobs,
            current_rip: 0,
            confirm_abort: false,
            confirm_rescan: false,
            label: String::new(),
            verify_failed_idx: None,
            batch_disc_count: 0,
        }
    }

    fn default_done_view(jobs: Vec<RipJob>) -> DoneView {
        DoneView {
            jobs,
            label: String::new(),
            disc_detected_label: None,
            eject: false,
            status_message: String::new(),
            filenames: vec![],
            batch_disc_count: 0,
            history_session_saved: false,
        }
    }

    // --- Dashboard job status rendering ---

    #[test]
    fn test_render_dashboard_pending_job() {
        let view = default_dashboard_view(vec![make_job(PlaylistStatus::Pending)]);
        let text = render_dashboard(&view);
        assert!(text.contains("Pending"), "should show Pending: {}", text);
    }

    #[test]
    fn test_render_dashboard_ripping_job() {
        let prog = RipProgress {
            frame: 1000,
            fps: 24.0,
            total_size: 500_000_000,
            out_time_secs: 1800,
            bitrate: "30000".into(),
            speed: 1.5,
            ..Default::default()
        };
        let view = default_dashboard_view(vec![make_job(PlaylistStatus::Ripping(prog))]);
        let text = render_dashboard(&view);
        assert!(text.contains("[#"), "should show progress bar: {}", text);
        assert!(text.contains("50%"), "should show 50%: {}", text);
    }

    #[test]
    fn test_render_dashboard_done_job() {
        let view = default_dashboard_view(vec![make_job(PlaylistStatus::Done(1_000_000_000))]);
        let text = render_dashboard(&view);
        assert!(
            text.contains("Completed"),
            "should show Completed: {}",
            text
        );
    }

    #[test]
    fn test_render_dashboard_skipped_job() {
        let view = default_dashboard_view(vec![make_job(PlaylistStatus::Skipped(500_000_000))]);
        let text = render_dashboard(&view);
        assert!(text.contains("Skipped"), "should show Skipped: {}", text);
    }

    #[test]
    fn test_render_dashboard_failed_job() {
        let view =
            default_dashboard_view(vec![make_job(PlaylistStatus::Failed("AACS error".into()))]);
        let text = render_dashboard(&view);
        assert!(
            text.contains("Failed:"),
            "should show Failed prefix: {}",
            text
        );
        assert!(
            text.contains("AACS error"),
            "should show error message: {}",
            text
        );
    }

    #[test]
    fn test_render_dashboard_verifying_job() {
        let view = default_dashboard_view(vec![make_job(PlaylistStatus::Verifying)]);
        let text = render_dashboard(&view);
        assert!(
            text.contains("Verifying..."),
            "should show Verifying...: {}",
            text
        );
    }

    #[test]
    fn test_render_dashboard_verified_job() {
        let result = crate::verify::VerifyResult {
            passed: true,
            level: crate::verify::VerifyLevel::Quick,
            checks: vec![],
        };
        let view = default_dashboard_view(vec![make_job(PlaylistStatus::Verified(
            1_000_000_000,
            result,
        ))]);
        let text = render_dashboard(&view);
        assert!(text.contains("Verified"), "should show Verified: {}", text);
    }

    #[test]
    fn test_render_dashboard_verify_failed_job() {
        let result = crate::verify::VerifyResult {
            passed: false,
            level: crate::verify::VerifyLevel::Quick,
            checks: vec![crate::verify::VerifyCheck {
                name: "duration",
                passed: false,
                detail: "expected 3600s, got 3000s".into(),
            }],
        };
        let view = default_dashboard_view(vec![make_job(PlaylistStatus::VerifyFailed(
            1_000_000_000,
            result,
        ))]);
        let text = render_dashboard(&view);
        assert!(
            text.contains("Verify failed"),
            "should show Verify failed: {}",
            text
        );
    }

    // --- Dashboard prompts/overlays ---

    #[test]
    fn test_render_dashboard_abort_confirmation() {
        let mut view = default_dashboard_view(vec![make_job(PlaylistStatus::Pending)]);
        view.confirm_abort = true;
        let text = render_dashboard(&view);
        assert!(
            text.contains("Really abort?"),
            "should show abort confirmation: {}",
            text
        );
        assert!(text.contains("[y] Yes"), "should show yes option: {}", text);
        assert!(text.contains("[n] No"), "should show no option: {}", text);
    }

    #[test]
    fn test_render_dashboard_rescan_confirmation() {
        let mut view = default_dashboard_view(vec![make_job(PlaylistStatus::Pending)]);
        view.confirm_rescan = true;
        let text = render_dashboard(&view);
        assert!(
            text.contains("Rescan disc?"),
            "should show rescan confirmation: {}",
            text
        );
    }

    #[test]
    fn test_render_dashboard_verify_prompt() {
        let result = crate::verify::VerifyResult {
            passed: false,
            level: crate::verify::VerifyLevel::Quick,
            checks: vec![crate::verify::VerifyCheck {
                name: "duration",
                passed: false,
                detail: "expected 3600s, got 3000s".into(),
            }],
        };
        let mut view = default_dashboard_view(vec![make_job(PlaylistStatus::VerifyFailed(
            1_000_000_000,
            result,
        ))]);
        view.verify_failed_idx = Some(0);
        let text = render_dashboard(&view);
        assert!(
            text.contains("S01E01_Pilot.mkv"),
            "should show filename: {}",
            text
        );
        assert!(
            text.contains("[D]elete"),
            "should show delete option: {}",
            text
        );
    }

    #[test]
    fn test_render_dashboard_normal_hints() {
        let view = default_dashboard_view(vec![make_job(PlaylistStatus::Pending)]);
        let text = render_dashboard(&view);
        assert!(
            text.contains("[q] Abort"),
            "should show abort hint: {}",
            text
        );
        assert!(
            text.contains("[Ctrl+R] Rescan"),
            "should show rescan hint: {}",
            text
        );
        assert!(
            text.contains("[Ctrl+S] Settings"),
            "should show settings hint: {}",
            text
        );
    }

    // --- Dashboard header/stats ---

    #[test]
    fn test_render_dashboard_progress_stats() {
        let prog = RipProgress {
            frame: 1000,
            fps: 24.0,
            total_size: 500_000_000,
            out_time_secs: 1800,
            bitrate: "30000".into(),
            speed: 1.5,
            ..Default::default()
        };
        let view = default_dashboard_view(vec![make_job(PlaylistStatus::Ripping(prog))]);
        let text = render_dashboard(&view);
        assert!(text.contains("fps:"), "should show fps stat: {}", text);
        assert!(text.contains("size:"), "should show size stat: {}", text);
        assert!(text.contains("time:"), "should show time stat: {}", text);
        assert!(
            text.contains("bitrate:"),
            "should show bitrate stat: {}",
            text
        );
        assert!(text.contains("speed:"), "should show speed stat: {}", text);
    }

    #[test]
    fn test_render_dashboard_completion_count() {
        let view = default_dashboard_view(vec![
            make_job(PlaylistStatus::Done(1_000_000)),
            make_job(PlaylistStatus::Pending),
        ]);
        let text = render_dashboard(&view);
        assert!(
            text.contains("1/2 complete"),
            "should show 1/2 complete: {}",
            text
        );
    }

    #[test]
    fn test_render_dashboard_label() {
        let mut view = default_dashboard_view(vec![make_job(PlaylistStatus::Pending)]);
        view.label = "MY_DISC_LABEL".into();
        let text = render_dashboard(&view);
        assert!(
            text.contains("MY_DISC_LABEL"),
            "should show disc label in title: {}",
            text
        );
    }

    // --- Done view tests ---

    #[test]
    fn test_render_done_completed_files() {
        let view = default_done_view(vec![make_job(PlaylistStatus::Done(1_073_741_824))]);
        let text = render_done(&view);
        assert!(
            text.contains("S01E01_Pilot.mkv"),
            "should show filename: {}",
            text
        );
        assert!(text.contains("1.0 GiB"), "should show size: {}", text);
    }

    #[test]
    fn test_render_done_skipped_files() {
        let view = default_done_view(vec![make_job(PlaylistStatus::Skipped(500_000_000))]);
        let text = render_done(&view);
        assert!(text.contains("SKIPPED"), "should show SKIPPED: {}", text);
    }

    #[test]
    fn test_render_done_failed_files() {
        let view = default_done_view(vec![make_job(PlaylistStatus::Failed("I/O error".into()))]);
        let text = render_done(&view);
        assert!(
            text.contains("FAILED:"),
            "should show FAILED prefix: {}",
            text
        );
        assert!(
            text.contains("I/O error"),
            "should show error message: {}",
            text
        );
    }

    #[test]
    fn test_render_done_verified_files() {
        let result = crate::verify::VerifyResult {
            passed: true,
            level: crate::verify::VerifyLevel::Quick,
            checks: vec![],
        };
        let view = default_done_view(vec![make_job(PlaylistStatus::Verified(
            1_073_741_824,
            result,
        ))]);
        let text = render_done(&view);
        // Verified files appear in the completed list with size
        assert!(
            text.contains("S01E01_Pilot.mkv"),
            "should show filename: {}",
            text
        );
        assert!(text.contains("1.0 GiB"), "should show size: {}", text);
    }

    #[test]
    fn test_render_done_verify_failed_files() {
        let result = crate::verify::VerifyResult {
            passed: false,
            level: crate::verify::VerifyLevel::Quick,
            checks: vec![crate::verify::VerifyCheck {
                name: "duration",
                passed: false,
                detail: "expected 3600s, got 3000s".into(),
            }],
        };
        let view = default_done_view(vec![make_job(PlaylistStatus::VerifyFailed(
            1_073_741_824,
            result,
        ))]);
        let text = render_done(&view);
        assert!(
            text.contains("VERIFY FAILED"),
            "should show VERIFY FAILED: {}",
            text
        );
        assert!(
            text.contains("duration"),
            "should show check name: {}",
            text
        );
    }

    #[test]
    fn test_render_done_all_success_summary() {
        let view = default_done_view(vec![
            make_job(PlaylistStatus::Done(1_000_000)),
            make_job(PlaylistStatus::Done(2_000_000)),
        ]);
        let text = render_done(&view);
        assert!(
            text.contains("All done! Backed up 2 playlist(s)"),
            "should show all success summary: {}",
            text
        );
    }

    #[test]
    fn test_render_done_mixed_summary() {
        let view = default_done_view(vec![
            make_job(PlaylistStatus::Done(1_000_000)),
            make_job(PlaylistStatus::Skipped(500_000)),
            make_job(PlaylistStatus::Failed("error".into())),
        ]);
        let text = render_done(&view);
        assert!(
            text.contains("Completed 2 of 3 playlist(s)"),
            "should show mixed summary: {}",
            text
        );
        assert!(
            text.contains("1 ripped"),
            "should show ripped count: {}",
            text
        );
        assert!(
            text.contains("1 skipped"),
            "should show skipped count: {}",
            text
        );
        assert!(
            text.contains("1 failed"),
            "should show failed count: {}",
            text
        );
    }

    #[test]
    fn test_render_done_disc_detected_popup() {
        let mut view = default_done_view(vec![make_job(PlaylistStatus::Done(1_000_000))]);
        view.disc_detected_label = Some("NEW_DISC".into());
        let text = render_done(&view);
        assert!(
            text.contains("New disc detected"),
            "should show popup title: {}",
            text
        );
        assert!(
            text.contains("NEW_DISC"),
            "should show disc label: {}",
            text
        );
    }

    #[test]
    fn test_render_done_error_in_body() {
        let view = DoneView {
            jobs: vec![],
            label: String::new(),
            disc_detected_label: None,
            eject: false,
            status_message: "AACS: decryption failed for this disc".into(),
            filenames: vec![],
            batch_disc_count: 0,
            history_session_saved: false,
        };
        let text = render_done(&view);
        assert!(text.contains("AACS"), "should show error in body: {}", text);
        assert!(
            text.contains("decryption failed"),
            "should show full error: {}",
            text
        );
    }

    #[test]
    fn test_render_done_dry_run() {
        let view = DoneView {
            jobs: vec![],
            label: String::new(),
            disc_detected_label: None,
            eject: false,
            status_message: String::new(),
            filenames: vec!["S01E01_Pilot.mkv".into(), "S01E02_Second.mkv".into()],
            batch_disc_count: 0,
            history_session_saved: false,
        };
        let text = render_done(&view);
        assert!(
            text.contains("S01E01_Pilot.mkv"),
            "should show first dry run file: {}",
            text
        );
        assert!(
            text.contains("S01E02_Second.mkv"),
            "should show second dry run file: {}",
            text
        );
    }

    #[test]
    fn test_render_done_key_hints() {
        let view = default_done_view(vec![make_job(PlaylistStatus::Done(1_000_000))]);
        let text = render_done(&view);
        assert!(
            text.contains("[Enter/Ctrl+R] Rescan"),
            "should show rescan hint: {}",
            text
        );
        assert!(
            text.contains("[Ctrl+E] Eject"),
            "should show eject hint: {}",
            text
        );
        assert!(
            text.contains("[Ctrl+S] Settings"),
            "should show settings hint: {}",
            text
        );
    }

    // --- Hook variable tests for {verify} and {verify_detail} ---

    fn make_session_with_job(status: PlaylistStatus) -> crate::session::DriveSession {
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
        session.screen = Screen::Ripping;
        session.output_dir = std::env::temp_dir();
        session.rip.jobs = vec![crate::types::RipJob {
            playlist: crate::types::Playlist {
                num: "00001".into(),
                duration: "1:00:00".into(),
                seconds: 3600,
                video_streams: 1,
                audio_streams: 2,
                subtitle_streams: 3,
            },
            episode: vec![],
            filename: "test.mkv".into(),
            status,
            estimated_size: 9_000_000,
        }];
        session
    }

    #[test]
    fn test_hook_vars_verify_passed() {
        let result = crate::verify::VerifyResult {
            passed: true,
            level: crate::verify::VerifyLevel::Quick,
            checks: vec![
                crate::verify::VerifyCheck {
                    name: "duration",
                    passed: true,
                    detail: "ok".into(),
                },
                crate::verify::VerifyCheck {
                    name: "video_streams",
                    passed: true,
                    detail: "1".into(),
                },
            ],
        };
        let session = make_session_with_job(PlaylistStatus::Verified(1_000_000, result));
        let vars = build_post_rip_vars(&session, 0, "success", "");
        assert_eq!(vars["verify"], "passed");
        assert_eq!(vars["verify_detail"], "");
    }

    #[test]
    fn test_hook_vars_verify_failed() {
        let result = crate::verify::VerifyResult {
            passed: false,
            level: crate::verify::VerifyLevel::Quick,
            checks: vec![
                crate::verify::VerifyCheck {
                    name: "duration",
                    passed: false,
                    detail: "expected 3600s, got 3000s".into(),
                },
                crate::verify::VerifyCheck {
                    name: "video_streams",
                    passed: true,
                    detail: "1".into(),
                },
                crate::verify::VerifyCheck {
                    name: "audio_streams",
                    passed: false,
                    detail: "expected 2, got 1".into(),
                },
            ],
        };
        let session = make_session_with_job(PlaylistStatus::VerifyFailed(1_000_000, result));
        let vars = build_post_rip_vars(&session, 0, "success", "");
        assert_eq!(vars["verify"], "failed");
        assert_eq!(vars["verify_detail"], "duration,audio_streams");
    }

    #[test]
    fn test_hook_vars_verify_skipped() {
        let session = make_session_with_job(PlaylistStatus::Done(1_000_000));
        let vars = build_post_rip_vars(&session, 0, "success", "");
        assert_eq!(vars["verify"], "skipped");
        assert_eq!(vars["verify_detail"], "");
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
