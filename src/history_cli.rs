//! CLI subcommand for history management.

use crate::history::{resolve_db_path, HistoryDb, SessionFilter, SessionStatus};
use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "bluback history", about = "Manage rip history")]
pub struct HistoryArgs {
    #[command(subcommand)]
    pub command: Option<HistoryCommand>,
}

#[derive(Subcommand, Debug)]
pub enum HistoryCommand {
    /// List past sessions
    List {
        #[arg(long, default_value = "20")]
        limit: i64,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        season: Option<i32>,
        #[arg(long)]
        batch_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show full details for a session
    Show { id: i64 },
    /// Show aggregate statistics
    Stats,
    /// Delete specific sessions
    Delete { ids: Vec<i64> },
    /// Clear history
    Clear {
        #[arg(long)]
        older_than: Option<String>,
        #[arg(long)]
        status: Option<String>,
        /// Skip confirmation (must be explicit — does NOT auto-enable on non-TTY)
        #[arg(long, short)]
        yes: bool,
    },
    /// Export history as JSON
    Export,
}

pub fn run_history(args: HistoryArgs) -> Result<()> {
    let db_path = resolve_db_path(None);
    let db = HistoryDb::open(&db_path)?;

    match args.command.unwrap_or(HistoryCommand::List {
        limit: 20,
        status: None,
        title: None,
        since: None,
        season: None,
        batch_id: None,
        json: false,
    }) {
        HistoryCommand::List {
            limit,
            status,
            title,
            since,
            season,
            batch_id,
            json,
        } => run_list(&db, limit, status, title, since, season, batch_id, json),
        HistoryCommand::Show { id } => run_show(&db, id),
        HistoryCommand::Stats => run_stats(&db),
        HistoryCommand::Delete { ids } => run_delete(&db, ids),
        HistoryCommand::Clear {
            older_than,
            status,
            yes,
        } => run_clear(&db, older_than, status, yes),
        HistoryCommand::Export => run_export(&db),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_list(
    db: &HistoryDb,
    limit: i64,
    status: Option<String>,
    title: Option<String>,
    since: Option<String>,
    season: Option<i32>,
    batch_id: Option<String>,
    json: bool,
) -> Result<()> {
    let parsed_status = if let Some(ref s) = status {
        let st =
            SessionStatus::from_str(s).ok_or_else(|| anyhow::anyhow!("invalid status: {}", s))?;
        Some(st)
    } else {
        None
    };

    let since_date = if let Some(ref since_str) = since {
        let parsed = crate::duration::parse_duration(since_str)?;
        Some(parsed.to_cutoff_date()?)
    } else {
        None
    };

    let filter = SessionFilter {
        limit: Some(limit),
        status: parsed_status,
        title,
        since: since_date,
        season,
        batch_id,
    };

    let sessions = db.list_sessions(&filter)?;

    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
        return Ok(());
    }

    // Table output
    println!(
        "{:>3}  {:<11} {:<24} {:<7} {:<5} {:<10} {:<6} {:<8}",
        "ID", "Date", "Title", "Season", "Disc", "Status", "Files", "Size"
    );

    for s in sessions {
        let date = s.started_at.split('T').next().unwrap_or("");
        let season_str = s
            .season
            .map(|n| format!("S{:02}", n))
            .unwrap_or_else(|| "—".to_string());
        let disc_str = s
            .disc_number
            .map(|n| format!("D{}", n))
            .unwrap_or_else(|| "—".to_string());
        let files_str = format!("{}/{}", s.files_completed, s.files_total);
        let size_str = format_size(s.total_size);

        println!(
            "{:>3}  {:<11} {:<24} {:<7} {:<5} {:<10} {:<6} {:<8}",
            s.id,
            date,
            truncate(&s.title, 24),
            season_str,
            disc_str,
            s.display_status(),
            files_str,
            size_str
        );
    }

    Ok(())
}

fn run_show(db: &HistoryDb, id: i64) -> Result<()> {
    let detail = db
        .get_session(id)?
        .ok_or_else(|| anyhow::anyhow!("session #{} not found", id))?;

    let s = &detail.summary;

    // Header
    let season_str = s
        .season
        .map(|n| format!("S{:02}", n))
        .unwrap_or_else(|| "".to_string());
    let disc_str = s
        .disc_number
        .map(|n| format!("D{}", n))
        .unwrap_or_else(|| "".to_string());
    let mut parts = vec![s.title.as_str()];
    if !season_str.is_empty() {
        parts.push(&season_str);
    }
    if !disc_str.is_empty() {
        parts.push(&disc_str);
    }

    println!("Session #{} — {}", s.id, parts.join(" "));

    // Details
    println!("  Disc:     {}", s.volume_label);
    if let Some(ref dev) = detail.device {
        println!("  Device:   {}", dev);
    }

    let started = s.started_at.split('T').next().unwrap_or("");
    let started_time = s
        .started_at
        .split('T')
        .nth(1)
        .and_then(|t| t.split('.').next())
        .unwrap_or("");
    let finished = detail
        .finished_at
        .as_ref()
        .and_then(|f| f.split('T').nth(1))
        .and_then(|t| t.split('.').next())
        .unwrap_or("");

    if !finished.is_empty() {
        println!("  Date:     {} {} -> {}", started, started_time, finished);
    } else {
        println!("  Date:     {} {}", started, started_time);
    }

    if let (Some(tmdb_id), Some(ref tmdb_type)) = (detail.tmdb_id, detail.tmdb_type.as_ref()) {
        println!("  TMDb:     {}/{}", tmdb_type, tmdb_id);
    }

    let status_msg = match s.display_status() {
        "partial" => format!(
            "partial ({}/{} playlists ripped)",
            s.files_completed, s.files_total
        ),
        other => other.to_string(),
    };
    println!("  Status:   {}", status_msg);
    println!();

    // Files
    if !detail.files.is_empty() {
        println!("  Files:");
        for f in &detail.files {
            let symbol = match f.status {
                crate::history::FileStatus::Completed => "+",
                crate::history::FileStatus::Failed => "x",
                crate::history::FileStatus::Skipped => ".",
                crate::history::FileStatus::InProgress => "…",
            };

            let size_str = if let Some(sz) = f.file_size {
                format_size(sz)
            } else {
                "—".to_string()
            };

            let duration_str = format_duration_ms(f.duration_ms);

            let verified_str = f.verified.as_deref().unwrap_or("");

            let mut line = format!(
                "    {} {:<30} {:>8}  {:>5}",
                symbol,
                truncate(&f.playlist, 30),
                size_str,
                duration_str
            );

            if f.status == crate::history::FileStatus::Failed {
                if let Some(ref err) = f.error {
                    line.push_str(&format!("  error: {}", err));
                }
            } else if !verified_str.is_empty() {
                line.push_str(&format!("  {}", verified_str));
            }

            println!("{}", line);
        }
    }

    Ok(())
}

fn run_stats(db: &HistoryDb) -> Result<()> {
    let stats = db.stats()?;

    let display_completed = stats.completed_sessions - stats.partial_sessions;

    println!("History Summary");
    println!(
        "  Sessions:  {} ({} completed, {} partial, {} failed, {} scanned)",
        stats.total_sessions,
        display_completed,
        stats.partial_sessions,
        stats.failed_sessions,
        stats.scanned_sessions
    );
    println!(
        "  Files:     {} ripped, {} failed, {} skipped",
        stats.total_files - stats.failed_files - stats.skipped_files,
        stats.failed_files,
        stats.skipped_files
    );
    println!(
        "  Total:     {} across {} discs",
        format_size(stats.total_size),
        stats.total_sessions
    );

    if let Some(ref first) = stats.first_session {
        let first_date = first.split('T').next().unwrap_or(first);
        println!("  First:     {}", first_date);
    }
    if let Some(ref last) = stats.last_session {
        let last_date = last.split('T').next().unwrap_or(last);
        println!("  Last:      {}", last_date);
    }

    if stats.batch_count > 0 {
        println!("  Batches:   {} batch runs", stats.batch_count);
    }

    Ok(())
}

fn run_delete(db: &HistoryDb, ids: Vec<i64>) -> Result<()> {
    use std::io::IsTerminal;

    if ids.is_empty() {
        bail!("no session IDs provided");
    }

    if !std::io::stdin().is_terminal() {
        eprintln!("warning: stdin is not a TTY; proceeding without confirmation");
    } else {
        print!("Delete {} session(s)? [y/N] ", ids.len());
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    for id in ids {
        if db.delete_session(id)? {
            println!("Deleted session #{}", id);
        } else {
            eprintln!("warning: session #{} not found", id);
        }
    }

    Ok(())
}

fn run_clear(
    db: &HistoryDb,
    older_than: Option<String>,
    status: Option<String>,
    yes: bool,
) -> Result<()> {
    let parsed_status = if let Some(ref s) = status {
        let st =
            SessionStatus::from_str(s).ok_or_else(|| anyhow::anyhow!("invalid status: {}", s))?;
        Some(vec![st])
    } else {
        None
    };

    let count = if let Some(ref cutoff_str) = older_than {
        let parsed = crate::duration::parse_duration(cutoff_str)?;
        let cutoff_date = parsed.to_cutoff_date()?;

        if !yes {
            print!("Clear all sessions older than {}? [y/N] ", cutoff_str);
            std::io::Write::flush(&mut std::io::stdout())?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Cancelled.");
                return Ok(());
            }
        }

        db.prune(&cutoff_date, parsed_status.as_deref())?
    } else {
        if !yes {
            print!("Clear ALL history? [y/N] ");
            std::io::Write::flush(&mut std::io::stdout())?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Cancelled.");
                return Ok(());
            }
        }

        db.clear_all()?
    };

    println!("Deleted {} session(s).", count);
    Ok(())
}

fn run_export(db: &HistoryDb) -> Result<()> {
    db.export_json(&mut std::io::stdout())?;
    Ok(())
}

fn format_size(bytes: i64) -> String {
    if bytes == 0 {
        return "—".to_string();
    }
    let gb = bytes as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        return format!("{:.1} GB", gb);
    }
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1.0 {
        return format!("{:.1} MB", mb);
    }
    let kb = bytes as f64 / 1024.0;
    format!("{:.1} KB", kb)
}

fn format_duration_ms(ms: Option<i64>) -> String {
    match ms {
        None | Some(0) => "—".to_string(),
        Some(ms) => {
            let total_secs = ms / 1000;
            let hours = total_secs / 3600;
            let minutes = (total_secs % 3600) / 60;
            let secs = total_secs % 60;
            if hours > 0 {
                format!("{}:{:02}:{:02}", hours, minutes, secs)
            } else {
                format!("{:02}:{:02}", minutes, secs)
            }
        }
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}
