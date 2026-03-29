mod aacs;
mod chapters;
mod check;
mod cli;
mod config;
mod disc;
mod drive_monitor;
mod hooks;
mod logging;
mod media;
mod rip;
mod session;
mod tmdb;
mod tui;
mod types;
mod util;
mod workflow;

use clap::Parser;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static CANCELLED: AtomicBool = AtomicBool::new(false);
static FIRST_SIGNAL_MS: AtomicU64 = AtomicU64::new(0);

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

    /// Mark playlists as specials (S{season}SP{episode}), e.g. "4,5" or "4-5"
    #[arg(long, conflicts_with = "movie")]
    specials: Option<String>,

    /// Overwrite existing output files instead of skipping
    #[arg(long)]
    overwrite: bool,

    /// Scan disc and print playlist info, then exit
    #[arg(long)]
    list_playlists: bool,

    /// Show detailed info (e.g. stream info with --list-playlists)
    #[arg(short = 'v', long)]
    verbose: bool,

    /// AACS decryption backend: auto, libaacs, or libmmbd
    #[arg(long, value_parser = ["auto", "libaacs", "libmmbd"])]
    aacs_backend: Option<String>,

    /// Validate environment and configuration, then exit
    #[arg(long)]
    check: bool,

    /// Stderr log verbosity: error, warn, info, debug, trace [default: warn]
    #[arg(long, value_parser = ["error", "warn", "info", "debug", "trace"])]
    log_level: Option<String>,

    /// Disable log file output
    #[arg(long)]
    no_log: bool,

    /// Don't embed metadata tags in output MKV files
    #[arg(long)]
    no_metadata: bool,

    /// Custom log file path (overrides default location)
    #[arg(long, conflicts_with = "no_log")]
    log_file: Option<PathBuf>,

    /// Disable post-rip and post-session hooks for this run
    #[arg(long)]
    no_hooks: bool,
}

impl Args {
    pub fn device(&self) -> &std::path::Path {
        self.device.as_deref().expect("device resolved before use")
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

const EXIT_SUCCESS: i32 = 0;
const EXIT_RUNTIME_ERROR: i32 = 1;
#[allow(dead_code)]
const EXIT_USAGE_ERROR: i32 = 2;
const EXIT_NO_DEVICE: i32 = 3;
const EXIT_CANCELLED: i32 = 4;

fn main() {
    let code = run();
    std::process::exit(code);
}

fn run() -> i32 {
    let code = match run_inner() {
        Ok(code) => code,
        Err(e) => {
            // Fallback for pre-logging errors (before logging::init runs)
            if log::max_level() == log::LevelFilter::Off {
                eprintln!("Error: {:#}", e);
            }
            log::error!("{:#}", e);
            classify_exit_code(&e)
        }
    };
    aacs::reap_children();
    code
}

fn classify_exit_code(err: &anyhow::Error) -> i32 {
    let msg = format!("{:#}", err);
    if msg.contains("No optical drives")
        || msg.contains("Device not found")
        || msg.contains("No disc")
    {
        return EXIT_NO_DEVICE;
    }
    if msg.contains("cancelled") || msg.contains("Cancelled") {
        return EXIT_CANCELLED;
    }
    if let Some(me) = err.downcast_ref::<crate::media::MediaError>() {
        return match me {
            crate::media::MediaError::DeviceNotFound(_) | crate::media::MediaError::NoDisc => {
                EXIT_NO_DEVICE
            }
            crate::media::MediaError::Cancelled => EXIT_CANCELLED,
            _ => EXIT_RUNTIME_ERROR,
        };
    }
    EXIT_RUNTIME_ERROR
}

fn run_inner() -> anyhow::Result<i32> {
    let mut args = Args::parse();

    ctrlc::set_handler(|| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let first = FIRST_SIGNAL_MS.load(Ordering::Relaxed);
        if first > 0 && now.saturating_sub(first) < 2000 {
            std::process::exit(130);
        }
        FIRST_SIGNAL_MS.store(now, Ordering::Relaxed);
        CANCELLED.store(true, Ordering::Relaxed);
    })
    .expect("failed to set Ctrl+C handler");

    let config_path = config::resolve_config_path(args.config.clone());
    let config = config::load_from(&config_path);

    // Initialize logging before config validation so warnings are captured
    let use_tui = !args.no_tui && atty_stdout();
    let stderr_level = logging::parse_level(
        args.log_level
            .as_deref()
            .unwrap_or_else(|| config.log_level()),
    );
    let log_path = logging::init(
        &config,
        stderr_level,
        args.log_file.clone(),
        args.no_log,
        use_tui,
    )?;

    if config_path.exists() {
        if let Ok(raw) = std::fs::read_to_string(&config_path) {
            for w in config::validate_raw_toml(&raw) {
                log::warn!("{} in {}", w, config_path.display());
            }
            for w in config::validate_config(&config) {
                log::warn!("{}", w);
            }
        }
    }

    if let Some(ref path) = log_path {
        let header = logging::session_header(
            env!("CARGO_PKG_VERSION"),
            args.device
                .as_deref()
                .map(|d| d.to_string_lossy())
                .as_deref(),
            &args.output.display().to_string(),
            &config_path,
            args.aacs_backend.as_deref().unwrap_or("auto"),
        );
        // Write header directly to log file (before log macros, which add timestamps)
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(path) {
            let _ = f.write_all(header.as_bytes());
        }
        log::info!("Log file: {}", path.display());
    }

    if args.check {
        return Ok(check::run_check(&config, &config_path));
    }

    let aacs_backend = args
        .aacs_backend
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
        tui::run_settings(&config, config_path)?;
        return Ok(EXIT_SUCCESS);
    }

    // Apply config defaults to args
    if args.output.as_os_str() == "." {
        if let Some(ref dir) = config.output_dir {
            args.output = PathBuf::from(dir);
        }
    }

    // Device resolution: in TUI multi-drive auto mode, leave args.device as None
    // so the coordinator's DriveMonitor can detect and manage all drives.
    // In CLI mode or TUI manual mode, resolve a single device.
    let multi_drive_auto = use_tui && config.multi_drive_mode() == "auto";
    if args.device.is_none() && !multi_drive_auto {
        if let Some(ref dev) = config.device {
            if dev != "auto-detect" {
                args.device = Some(PathBuf::from(dev));
            }
        }
    }
    if args.device.is_none() && !multi_drive_auto {
        let drives = disc::detect_optical_drives();
        if let Some(drive) = drives.first() {
            args.device = Some(drive.clone());
        } else {
            anyhow::bail!("No optical drives detected");
        }
    }

    if args.list_playlists {
        cli::list_playlists(&args, &config)?;
        return Ok(EXIT_SUCCESS);
    }

    let headless = args.yes || (!atty_stdin() && !use_tui);

    if use_tui {
        tui::run(&args, &config, config_path)?;
    } else {
        cli::run(&args, &config, headless)?;
    }

    Ok(EXIT_SUCCESS)
}

fn atty_stdout() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

fn atty_stdin() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}
