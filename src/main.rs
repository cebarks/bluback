mod chapters;
mod cli;
mod config;
mod disc;
mod rip;
mod tmdb;
mod tui;
mod types;
mod util;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "bluback",
    version,
    about = "Back up Blu-ray discs to MKV files using ffmpeg + libaacs"
)]
pub struct Args {
    /// Blu-ray device path [default: auto-detect]
    #[arg(short, long)]
    device: Option<PathBuf>,

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

    /// Custom filename template
    #[arg(long, group = "format_group")]
    format: Option<String>,

    /// Use a built-in filename preset (default, plex, jellyfin)
    #[arg(long, group = "format_group")]
    format_preset: Option<String>,

    /// Eject disc after successful rip
    #[arg(long, conflicts_with = "no_eject")]
    eject: bool,

    /// Don't eject disc after rip (overrides config)
    #[arg(long, conflicts_with = "eject")]
    no_eject: bool,

    /// Don't set drive to maximum read speed
    #[arg(long)]
    no_max_speed: bool,

    /// Open settings panel without starting a rip
    #[arg(long, conflicts_with_all = ["dry_run", "no_tui"])]
    settings: bool,

    /// Path to config file
    #[arg(long)]
    config: Option<PathBuf>,
}

impl Args {
    pub fn device(&self) -> &std::path::Path {
        self.device
            .as_deref()
            .expect("device resolved before use")
    }

    pub fn cli_eject(&self) -> Option<bool> {
        if self.eject {
            Some(true)
        } else if self.no_eject {
            Some(false)
        } else {
            None
        }
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = Args::parse();

    let config_path = config::resolve_config_path(args.config.clone());
    let config = config::load_from(&config_path);

    // --settings mode: open settings panel without disc/dependency checks
    if args.settings {
        if !atty_stdout() {
            anyhow::bail!("--settings requires a terminal (stdout is not a TTY)");
        }
        return tui::run_settings(&config, config_path);
    }

    // Apply config defaults to args
    if args.device.is_none() {
        if let Some(ref dev) = config.device {
            if dev != "auto-detect" {
                args.device = Some(PathBuf::from(dev));
            }
        }
    }
    if args.output.as_os_str() == "." {
        if let Some(ref dir) = config.output_dir {
            args.output = PathBuf::from(dir);
        }
    }

    if args.device.is_none() {
        let drives = disc::detect_optical_drives();
        args.device = Some(drives[0].clone());
    }

    disc::check_dependencies()?;

    let use_tui = !args.no_tui && atty_stdout();

    if use_tui {
        tui::run(&args, &config, config_path)
    } else {
        cli::run(&args, &config)
    }
}

fn atty_stdout() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}
