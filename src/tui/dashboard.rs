use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use std::io::BufRead;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use super::{App, Screen};
use crate::types::PlaylistStatus;
use crate::util::format_size;
use crate::{disc, rip};

pub fn render(f: &mut Frame, app: &App) {
    let done_count = app
        .rip_jobs
        .iter()
        .filter(|j| matches!(j.status, PlaylistStatus::Done(_)))
        .count();
    let total = app.rip_jobs.len();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // title + stats
            Constraint::Min(4),    // job table
            Constraint::Length(1), // key hints
        ])
        .split(f.area());

    // Title with stats
    let stats_text = active_rip_stats(app);
    let title_text = if stats_text.is_empty() {
        format!("Ripping: {}/{} complete", done_count, total)
    } else {
        format!("Ripping: {}/{} complete  │  {}", done_count, total, stats_text)
    };
    let title = Paragraph::new(title_text)
        .block(Block::default().borders(Borders::ALL).title("bluback"));
    f.render_widget(title, chunks[0]);

    // Job table
    let header = Row::new(["#", "Playlist", "Episode", "File", "Status", "Size", "ETA"])
        .style(Style::default().add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .rip_jobs
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let ep_name = job
                .episode
                .as_ref()
                .map(|e| format!("E{:02} {}", e.episode_number, e.name))
                .unwrap_or_default();

            let (status, size, eta) = match &job.status {
                PlaylistStatus::Pending => ("Pending".to_string(), String::new(), String::new()),
                PlaylistStatus::Ripping(prog) => {
                    let pct = if job.playlist.seconds > 0 {
                        (prog.out_time_secs as f64 / job.playlist.seconds as f64 * 100.0)
                            .min(100.0) as u32
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
                    ("Done".to_string(), format_size(*sz), String::new())
                }
                PlaylistStatus::Failed(msg) => {
                    (format!("Failed: {}", msg), String::new(), String::new())
                }
            };

            Row::new([
                format!("{}", i + 1),
                job.playlist.num.clone(),
                ep_name,
                job.filename.clone(),
                status,
                size,
                eta,
            ])
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
    let hint = if app.confirm_abort {
        Paragraph::new("Really abort? [y] Yes  [n] No")
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
    } else if app.confirm_rescan {
        Paragraph::new("Rescan disc? This will abort the current rip. [y] Yes  [n] No")
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Paragraph::new("[q] Abort  [Ctrl+R] Rescan")
            .style(Style::default().fg(Color::DarkGray))
    };
    f.render_widget(hint, chunks[2]);
}

pub fn render_done(f: &mut Frame, app: &App) {
    let completed: Vec<_> = app
        .rip_jobs
        .iter()
        .filter_map(|j| match &j.status {
            PlaylistStatus::Done(sz) => Some((j.filename.as_str(), *sz)),
            _ => None,
        })
        .collect();
    let failed_count = app
        .rip_jobs
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

    let summary = if !app.status_message.is_empty() {
        app.status_message.clone()
    } else if failed_count > 0 {
        format!(
            "Completed {} of {} playlist(s) ({} failed)",
            completed.len(),
            app.rip_jobs.len(),
            failed_count
        )
    } else {
        format!("All done! Backed up {} playlist(s)", completed.len())
    };

    let title = Paragraph::new(summary)
        .block(Block::default().borders(Borders::ALL).title("bluback"));
    f.render_widget(title, chunks[0]);

    let mut lines: Vec<Line> = Vec::new();
    if app.rip_jobs.is_empty() && !app.filenames.is_empty() {
        // Dry run: show what would have been ripped
        for name in &app.filenames {
            lines.push(Line::from(format!("  {}", name)));
        }
    } else {
        for (filename, sz) in &completed {
            lines.push(Line::from(format!("  {} ({})", filename, format_size(*sz))));
        }
        for job in &app.rip_jobs {
            if let PlaylistStatus::Failed(msg) = &job.status {
                lines.push(
                    Line::from(format!("  {} - FAILED: {}", job.filename, msg))
                        .style(Style::default().fg(Color::Red)),
                );
            }
        }
    }

    let body = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Results"));
    f.render_widget(body, chunks[1]);

    let hint = Paragraph::new("[Enter/Ctrl+R] Rescan  [any other key] Exit")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[2]);
}

pub fn handle_input(app: &mut App, key: KeyEvent) {
    if app.confirm_abort {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(ref mut child) = app.rip_child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                app.rip_child = None;
                app.progress_rx = None;
                app.quit = true;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.confirm_abort = false;
            }
            _ => {}
        }
        return;
    }

    if key.code == KeyCode::Char('q') {
        app.confirm_abort = true;
    }
}

pub fn tick(app: &mut App) -> anyhow::Result<()> {
    let all_done = app.rip_jobs.iter().all(|j| {
        matches!(j.status, PlaylistStatus::Done(_) | PlaylistStatus::Failed(_))
    });

    if all_done && !app.rip_jobs.is_empty() {
        // Clean up child if somehow still around
        if let Some(ref mut child) = app.rip_child {
            let _ = child.wait();
        }
        app.rip_child = None;
        app.progress_rx = None;

        // Poll for eject completion if already in progress
        if let Some(ref rx) = app.eject_rx {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    app.eject_rx = None;
                    app.status_message.clear();
                    app.screen = Screen::Done;
                }
                Ok(Err(e)) => {
                    app.eject_rx = None;
                    app.status_message = format!("Warning: failed to eject disc: {}", e);
                    app.screen = Screen::Done;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    // Still ejecting, keep waiting
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    app.eject_rx = None;
                    app.status_message = "Warning: eject thread terminated unexpectedly".into();
                    app.screen = Screen::Done;
                }
            }
            return Ok(());
        }

        let all_succeeded = app.rip_jobs.iter().all(|j| matches!(j.status, PlaylistStatus::Done(_)));

        // If eject enabled and all succeeded, spawn eject thread
        if app.eject && all_succeeded {
            let device = app.args.device.to_string_lossy().to_string();
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let _ = tx.send(crate::disc::eject_disc(&device));
            });
            app.eject_rx = Some(rx);
            app.status_message = "Ejecting disc...".into();
            return Ok(());
        }

        app.screen = Screen::Done;
        return Ok(());
    }

    // If no active rip, start the next pending job
    if app.rip_child.is_none() {
        let next_idx = app
            .rip_jobs
            .iter()
            .position(|j| matches!(j.status, PlaylistStatus::Pending));

        if let Some(idx) = next_idx {
            app.current_rip = idx;
            let job = &app.rip_jobs[idx];
            let device = app.args.device.to_string_lossy().to_string();
            let playlist_num = job.playlist.num.clone();
            let outfile = app.args.output.join(&job.filename);
            if let Some(parent) = outfile.parent() {
                std::fs::create_dir_all(parent).ok();
            }

            // Skip if output file already exists
            if outfile.exists() {
                let file_size = std::fs::metadata(&outfile)
                    .map(|m| m.len())
                    .unwrap_or(0);
                app.rip_jobs[idx].status = PlaylistStatus::Done(file_size);
                return Ok(());
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

                    app.rip_child = Some(child);
                    app.progress_rx = Some(rx);
                    app.stderr_buffer = Some(stderr_buf);
                    app.progress_state.clear();
                    app.rip_jobs[idx].status =
                        PlaylistStatus::Ripping(crate::types::RipProgress::default());
                }
                Err(e) => {
                    app.rip_jobs[idx].status =
                        PlaylistStatus::Failed(format!("Failed to start: {}", e));
                }
            }
        }
        return Ok(());
    }

    // Read progress from the channel
    if let Some(ref rx) = app.progress_rx {
        while let Ok(line) = rx.try_recv() {
            if let Some(progress) = rip::parse_progress_line(&line, &mut app.progress_state) {
                let idx = app.current_rip;
                app.rip_jobs[idx].status = PlaylistStatus::Ripping(progress);
            }
        }
    }

    // Check if the child has exited
    if let Some(ref mut child) = app.rip_child {
        match child.try_wait() {
            Ok(Some(status)) => {
                let idx = app.current_rip;
                if status.success() {
                    let outfile = app.args.output.join(&app.rip_jobs[idx].filename);
                    let file_size = std::fs::metadata(&outfile)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    app.rip_jobs[idx].status = PlaylistStatus::Done(file_size);
                } else {
                    let stderr_msg = app.stderr_buffer.as_ref()
                        .and_then(|b| b.lock().ok())
                        .map(|b| b.clone())
                        .unwrap_or_default();
                    let msg = if stderr_msg.is_empty() {
                        format!("ffmpeg exited with code {}", status)
                    } else {
                        let last_line = stderr_msg.lines().last().unwrap_or("");
                        format!("ffmpeg: {}", last_line)
                    };
                    app.rip_jobs[idx].status = PlaylistStatus::Failed(msg);
                }
                app.rip_child = None;
                app.progress_rx = None;
                app.stderr_buffer = None;
            }
            Ok(None) => {} // still running
            Err(e) => {
                let idx = app.current_rip;
                app.rip_jobs[idx].status = PlaylistStatus::Failed(format!("wait error: {}", e));
                app.rip_child = None;
                app.progress_rx = None;
                app.stderr_buffer = None;
            }
        }
    }

    Ok(())
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

fn active_rip_stats(app: &App) -> String {
    if let Some(job) = app.rip_jobs.get(app.current_rip) {
        if let PlaylistStatus::Ripping(ref prog) = job.status {
            let mins = prog.out_time_secs / 60;
            let secs = prog.out_time_secs % 60;
            return format!(
                "fps: {:.1}  size: {}  time: {:02}:{:02}  bitrate: {}  speed: {:.2}x",
                prog.fps,
                format_size(prog.total_size),
                mins,
                secs,
                prog.bitrate,
                prog.speed,
            );
        }
    }
    String::new()
}
