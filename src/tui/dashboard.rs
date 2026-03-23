use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap};
use std::io::BufRead;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use super::{App, Screen};
use crate::types::PlaylistStatus;
use crate::util::format_size;
use crate::{disc, rip};

pub fn render(f: &mut Frame, app: &App) {
    let done_count = app
        .rip
        .jobs
        .iter()
        .filter(|j| matches!(j.status, PlaylistStatus::Done(_)))
        .count();
    let total = app.rip.jobs.len();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title + stats
            Constraint::Min(4),    // job table
            Constraint::Length(1), // key hints
        ])
        .split(f.area());

    // Title with stats
    let stats_text = active_rip_stats(app);
    let title_text = if stats_text.is_empty() {
        format!("Ripping: {}/{} complete", done_count, total)
    } else {
        format!(
            "Ripping: {}/{} complete  │  {}",
            done_count, total, stats_text
        )
    };
    let title =
        Paragraph::new(title_text).block(Block::default().borders(Borders::ALL).title("bluback"));
    f.render_widget(title, chunks[0]);

    // Job table
    let header = Row::new(["#", "Playlist", "Episode", "File", "Status", "Size", "ETA"])
        .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .rip
        .jobs
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let ep_name = if job.episode.is_empty() {
                String::new()
            } else if job.episode.len() == 1 {
                format!("E{:02} {}", job.episode[0].episode_number, job.episode[0].name)
            } else {
                let first = &job.episode[0];
                let last = &job.episode[job.episode.len() - 1];
                format!("E{:02}-E{:02} {}", first.episode_number, last.episode_number, first.name)
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
                PlaylistStatus::Done(sz) => ("Completed".to_string(), format_size(*sz), String::new()),
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
                PlaylistStatus::Ripping(_) => {
                    row.style(Style::default().fg(Color::Cyan))
                }
                PlaylistStatus::Done(_) => {
                    row.style(Style::default().fg(Color::DarkGray))
                }
                PlaylistStatus::Failed(_) => {
                    row.style(Style::default().fg(Color::Red))
                }
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
    let hint = if app.rip.confirm_abort {
        Paragraph::new("Really abort? [y] Yes  [n] No")
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
    } else if app.rip.confirm_rescan {
        Paragraph::new("Rescan disc? This will abort the current rip. [y] Yes  [n] No").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Paragraph::new("[q] Abort  [Ctrl+R] Rescan  [Ctrl+S] Settings").style(Style::default().fg(Color::DarkGray))
    };
    f.render_widget(hint, chunks[2]);
}

pub fn render_done(f: &mut Frame, app: &App) {
    let completed: Vec<_> = app
        .rip
        .jobs
        .iter()
        .filter_map(|j| match &j.status {
            PlaylistStatus::Done(sz) => Some((j.filename.as_str(), *sz)),
            _ => None,
        })
        .collect();
    let failed_count = app
        .rip
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
        .split(f.area());

    // When showing an error with no rip jobs, put a short summary in the title
    // and the full message in the results body where it can wrap.
    let error_in_body = !app.status_message.is_empty()
        && app.rip.jobs.is_empty()
        && app.wizard.filenames.is_empty();

    let summary = if error_in_body {
        app.status_message
            .split(':')
            .next()
            .unwrap()
            .to_string()
    } else if !app.status_message.is_empty() {
        app.status_message.clone()
    } else if failed_count > 0 {
        format!(
            "Completed {} of {} playlist(s) ({} failed)",
            completed.len(),
            app.rip.jobs.len(),
            failed_count
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
            Line::from(format!("  {}", app.status_message))
                .style(Style::default().fg(Color::Red)),
        );
    } else if app.rip.jobs.is_empty() && !app.wizard.filenames.is_empty() {
        // Dry run: show what would have been ripped
        for name in &app.wizard.filenames {
            lines.push(Line::from(format!("  {}", name)));
        }
    } else {
        for (filename, sz) in &completed {
            lines.push(Line::from(format!("  {} ({})", filename, format_size(*sz))));
        }
        for job in &app.rip.jobs {
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

    let hint = Paragraph::new("[Enter/Ctrl+R] Rescan  [Ctrl+E] Eject  [Ctrl+S] Settings  [any other key] Exit")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[2]);

    if let Some(ref label) = app.disc_detected_label {
        let popup_area = centered_rect(60, 5, f.area());
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

pub fn handle_input(app: &mut App, key: KeyEvent) {
    if app.rip.confirm_abort {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(ref mut child) = app.rip.child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                app.rip.child = None;
                app.rip.progress_rx = None;
                app.quit = true;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.rip.confirm_abort = false;
            }
            _ => {}
        }
        return;
    }

    if key.code == KeyCode::Char('q') {
        app.rip.confirm_abort = true;
    }
}

pub fn tick(app: &mut App) -> anyhow::Result<()> {
    if check_all_done(app) {
        return Ok(());
    }

    if app.rip.child.is_none() {
        start_next_job(app);
        return Ok(());
    }

    poll_active_job(app);
    Ok(())
}

fn check_all_done(app: &mut App) -> bool {
    let all_done = app.rip.jobs.iter().all(|j| {
        matches!(
            j.status,
            PlaylistStatus::Done(_) | PlaylistStatus::Failed(_)
        )
    });

    if all_done && !app.rip.jobs.is_empty() {
        if let Some(ref mut child) = app.rip.child {
            let _ = child.wait();
        }
        app.rip.child = None;
        app.rip.progress_rx = None;
        app.screen = Screen::Done;
        if app.disc.did_mount {
            let _ = crate::disc::unmount_disc(&app.args.device().to_string_lossy());
            app.disc.did_mount = false;
        }
        super::start_disc_scan(app);
        app.screen = Screen::Done; // Override start_disc_scan's screen change
        true
    } else {
        false
    }
}

fn start_next_job(app: &mut App) {
    let next_idx = app
        .rip
        .jobs
        .iter()
        .position(|j| matches!(j.status, PlaylistStatus::Pending));

    let Some(idx) = next_idx else {
        return;
    };

    app.rip.current_rip = idx;
    let job = &app.rip.jobs[idx];
    let device = app.args.device().to_string_lossy().to_string();
    let playlist_num = job.playlist.num.clone();
    let outfile = app.args.output.join(&job.filename);
    if let Some(parent) = outfile.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Skip if output file already exists
    if outfile.exists() {
        let file_size = std::fs::metadata(&outfile).map(|m| m.len()).unwrap_or(0);
        app.rip.jobs[idx].status = PlaylistStatus::Done(file_size);
        return;
    }

    let streams = disc::probe_streams(&device, &playlist_num);
    let map_args = match streams {
        Some(ref s) => rip::build_map_args(s),
        None => vec!["-map".into(), "0".into()],
    };

    match rip::start_rip(&device, &playlist_num, &map_args, &outfile) {
        Ok(mut child) => {
            let stdout = child.stdout.take().expect("stdout piped");
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let reader = std::io::BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });

            let stderr = child.stderr.take().expect("stderr piped");
            let stderr_buf = Arc::new(Mutex::new(String::new()));
            let stderr_clone = stderr_buf.clone();
            thread::spawn(move || {
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    let mut buf = stderr_clone.lock().unwrap();
                    if !buf.is_empty() {
                        buf.push('\n');
                    }
                    buf.push_str(&line);
                }
            });

            app.rip.child = Some(child);
            app.rip.progress_rx = Some(rx);
            app.rip.stderr_buffer = Some(stderr_buf);
            app.rip.progress_state.clear();
            app.rip.jobs[idx].status =
                PlaylistStatus::Ripping(crate::types::RipProgress::default());
        }
        Err(e) => {
            app.rip.jobs[idx].status = PlaylistStatus::Failed(format!("Failed to start: {}", e));
        }
    }
}

fn poll_active_job(app: &mut App) {
    // Read progress from the channel
    if let Some(ref rx) = app.rip.progress_rx {
        while let Ok(line) = rx.try_recv() {
            if let Some(progress) = rip::parse_progress_line(&line, &mut app.rip.progress_state) {
                let idx = app.rip.current_rip;
                app.rip.jobs[idx].status = PlaylistStatus::Ripping(progress);
            }
        }
    }

    // Check if the child has exited
    if let Some(ref mut child) = app.rip.child {
        match child.try_wait() {
            Ok(Some(status)) => {
                let idx = app.rip.current_rip;
                if status.success() {
                    let outfile = app.args.output.join(&app.rip.jobs[idx].filename);
                    let file_size = std::fs::metadata(&outfile).map(|m| m.len()).unwrap_or(0);
                    app.rip.jobs[idx].status = PlaylistStatus::Done(file_size);

                    // Apply chapter markers
                    if app.has_mkvpropedit {
                        if let Some(ref mount) = app.disc.mount_point {
                            let playlist_num = &app.rip.jobs[idx].playlist.num;
                            if let Some(chapters) = crate::chapters::extract_chapters(
                                std::path::Path::new(mount.as_str()),
                                playlist_num,
                            ) {
                                let _ = crate::chapters::apply_chapters(&outfile, &chapters);
                            }
                        }
                    }
                } else {
                    let stderr_msg = app
                        .rip
                        .stderr_buffer
                        .as_ref()
                        .and_then(|b| b.lock().ok())
                        .map(|b| b.clone())
                        .unwrap_or_default();
                    let msg = if let Some(aacs_msg) = disc::check_aacs_error(&stderr_msg) {
                        aacs_msg
                    } else if stderr_msg.is_empty() {
                        format!("ffmpeg exited with code {}", status)
                    } else {
                        let last_line = stderr_msg.lines().last().unwrap_or("");
                        format!("ffmpeg: {}", last_line)
                    };
                    app.rip.jobs[idx].status = PlaylistStatus::Failed(msg);
                }
                app.rip.child = None;
                app.rip.progress_rx = None;
                app.rip.stderr_buffer = None;
            }
            Ok(None) => {} // still running
            Err(e) => {
                let idx = app.rip.current_rip;
                app.rip.jobs[idx].status = PlaylistStatus::Failed(format!("wait error: {}", e));
                app.rip.child = None;
                app.rip.progress_rx = None;
                app.rip.stderr_buffer = None;
            }
        }
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

fn active_rip_stats(app: &App) -> String {
    if let Some(job) = app.rip.jobs.get(app.rip.current_rip) {
        if let PlaylistStatus::Ripping(ref prog) = job.status {
            let time_str = rip::format_eta(prog.out_time_secs);
            let bitrate_str = if prog.bitrate.is_empty()
                || prog.bitrate == "N/A"
                || prog.bitrate == "0"
            {
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
