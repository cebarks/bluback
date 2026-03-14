mod cli;
mod disc;
mod rip;
mod tmdb;
mod tui;
mod types;
mod util;

use clap::Parser;
use std::path::PathBuf;

const DEFAULT_DEVICE: &str = "/dev/sr0";

#[derive(Parser, Debug, Clone)]
#[command(name = "bluback", version, about = "Back up Blu-ray discs to MKV files using ffmpeg + libaacs")]
pub struct Args {
    /// Blu-ray device path
    #[arg(short, long, default_value = DEFAULT_DEVICE)]
    device: PathBuf,

    /// Output directory
    #[arg(short, long, default_value = ".")]
    output: PathBuf,

    /// Season number
    #[arg(short, long)]
    season: Option<u32>,

    /// Starting episode number
    #[arg(short = 'e', long)]
    start_episode: Option<u32>,

    /// Minimum seconds to consider a playlist an episode
    #[arg(long, default_value = "900")]
    min_duration: u32,

    /// Movie mode (skip episode assignment)
    #[arg(long)]
    movie: bool,

    /// Show what would be ripped without ripping
    #[arg(long)]
    dry_run: bool,

    /// Plain text mode (auto if not a TTY)
    #[arg(long)]
    no_tui: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    disc::check_dependencies()?;

    let use_tui = !args.no_tui && atty_stdout();

    if use_tui {
        tui::run(&args)
    } else {
        cli::run(&args)
    }
}

fn atty_stdout() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}
