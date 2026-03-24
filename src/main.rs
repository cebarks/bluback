mod aacs;
mod chapters;
mod cli;
mod config;
mod disc;
mod media;
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

    /// Accept all defaults without prompting (auto if stdin is not a TTY)
    #[arg(short = 'y', long)]
    yes: bool,

    /// Set show (TV) or movie title directly, skipping TMDb lookup
    #[arg(long)]
    title: Option<String>,

    /// Movie release year for filename templates (used with --title in --movie mode)
    #[arg(long)]
    year: Option<String>,

    /// Select specific playlists (e.g. "1,2,3", "1-3", "all")
    #[arg(long)]
    playlists: Option<String>,

    /// Scan disc and print playlist info, then exit
    #[arg(long)]
    list_playlists: bool,

    /// AACS decryption backend: auto, libaacs, or libmmbd
    #[arg(long, value_parser = ["auto", "libaacs", "libmmbd"])]
    aacs_backend: Option<String>,
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

    let aacs_backend = args.aacs_backend
        .as_deref()
        .map(|s| match s {
            "libaacs" => config::AacsBackend::Libaacs,
            "libmmbd" => config::AacsBackend::Libmmbd,
            _ => config::AacsBackend::Auto,
        })
        .unwrap_or_else(|| config.aacs_backend());

    aacs::preflight(aacs_backend)?;

    // Suppress libbluray's BD_DEBUG stderr output unless verbose mode is on.
    // Must be set before any ffmpeg/libbluray calls.
    if !config.verbose_libbluray() {
        std::env::set_var("BD_DEBUG_MASK", "0");
    }

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
        if let Some(drive) = drives.first() {
            args.device = Some(drive.clone());
        } else {
            anyhow::bail!("No optical drives detected");
        }
    }

    if args.list_playlists {
        return cli::list_playlists(&args, &config);
    }

    let use_tui = !args.no_tui && atty_stdout();
    let headless = args.yes || (!atty_stdin() && !use_tui);

    if use_tui {
        tui::run(&args, &config, config_path)
    } else {
        cli::run(&args, &config, headless)
    }
}

fn atty_stdout() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

fn atty_stdin() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}
