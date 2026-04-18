mod aacs;
mod chapters;
mod check;
mod cli;
mod config;
mod detection;
mod disc;
mod drive_monitor;
mod duration;
mod generate;
mod history;
mod history_cli;
mod hooks;
mod index;
mod logging;
mod media;
mod rip;
mod session;
mod streams;
mod tmdb;
mod tui;
mod types;
mod util;
mod verify;
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

    /// Output directory [default: .]
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Season number
    #[arg(short, long)]
    season: Option<u32>,

    /// Starting episode number
    #[arg(short = 'e', long)]
    start_episode: Option<u32>,

    /// Min seconds to probe playlist (filters menu clips) [default: 30]
    #[arg(long)]
    min_probe_duration: Option<u32>,

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

    /// Hide detected specials from ripping (skips all specials)
    #[arg(long, conflicts_with_all = ["specials", "movie"])]
    hide_specials: bool,

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

    /// Verify output files after ripping
    #[arg(long)]
    verify: bool,

    /// Verification level: quick (header probe) or full (+ frame decode)
    #[arg(long, value_parser = ["quick", "full"])]
    verify_level: Option<String>,

    /// Disable verification (overrides config)
    #[arg(long)]
    no_verify: bool,

    /// Enable batch mode (rip → eject → wait → repeat)
    #[arg(long, conflicts_with_all = ["no_batch", "dry_run", "list_playlists", "check", "settings", "no_eject"])]
    batch: bool,

    /// Disable batch mode (overrides config)
    #[arg(long, conflicts_with = "batch")]
    no_batch: bool,

    /// Enable automatic episode/special detection heuristics
    #[arg(long, conflicts_with_all = ["no_auto_detect", "movie"])]
    auto_detect: bool,

    /// Disable auto-detection (overrides config)
    #[arg(long, conflicts_with = "auto_detect")]
    no_auto_detect: bool,

    /// Filter audio streams by language (e.g. "eng,jpn")
    #[arg(long)]
    audio_lang: Option<String>,

    /// Filter subtitle streams by language (e.g. "eng")
    #[arg(long)]
    subtitle_lang: Option<String>,

    /// Prefer surround audio (select surround + one stereo)
    #[arg(long)]
    prefer_surround: bool,

    /// Include all streams, ignoring config filters
    #[arg(long, conflicts_with_all = ["audio_lang", "subtitle_lang", "prefer_surround"])]
    all_streams: bool,

    /// Select streams by type-local index (e.g. "a:0,2;s:0-1")
    #[arg(long, conflicts_with_all = ["audio_lang", "subtitle_lang", "prefer_surround", "all_streams"])]
    tracks: Option<String>,

    /// Disable history for this run
    #[arg(long)]
    no_history: bool,

    /// Ignore history (skip duplicate detection and episode continuation, still records)
    #[arg(long, conflicts_with = "no_history")]
    ignore_history: bool,
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

    pub fn cli_batch(&self) -> Option<bool> {
        if self.batch {
            Some(true)
        } else if self.no_batch {
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
    // Early intercept for history subcommand
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.get(1).map(|s| s.as_str()) == Some("history") {
        use clap::Parser;
        let history_args = history_cli::HistoryArgs::parse_from(&raw_args[1..]);
        if let Err(e) = history_cli::run_history(history_args) {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Early intercept for generate subcommand
    if raw_args.get(1).map(|s| s.as_str()) == Some("generate") {
        use clap::Parser;
        let gen_args = generate::GenerateArgs::parse_from(&raw_args[1..]);
        if let Err(e) = generate::run_generate(gen_args) {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Become a subreaper so that orphaned descendant processes (e.g.,
    // makemkvcon spawned by libmmbd via double-fork) get reparented to
    // us instead of PID 1. This ensures kill_makemkvcon_children() can
    // find and clean up ALL makemkvcon processes, not just direct children.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
    }

    // Ensure makemkvcon cleanup on ALL exit paths, including double-Ctrl+C
    // force exit and sub-thread races. atexit handlers run during
    // std::process::exit() before the process terminates.
    extern "C" fn cleanup_makemkvcon() {
        aacs::kill_makemkvcon_children();
        aacs::reap_children();
    }
    unsafe {
        libc::atexit(cleanup_makemkvcon);
    }

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
    aacs::kill_makemkvcon_children();
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
            // Force exit — clean up makemkvcon before terminating.
            // The atexit handler provides a second sweep, but explicit
            // cleanup here catches processes while threads are still alive.
            aacs::kill_makemkvcon_children();
            aacs::reap_children();
            std::process::exit(130);
        }
        FIRST_SIGNAL_MS.store(now, Ordering::Relaxed);
        CANCELLED.store(true, Ordering::Relaxed);
    })
    .expect("failed to set Ctrl+C handler");

    let config_path = config::resolve_config_path(args.config.clone());
    let config = config::load_from(&config_path).unwrap_or_else(|e| {
        eprintln!("Error: {:#}", e);
        std::process::exit(2);
    });

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
            &args
                .output
                .as_deref()
                .unwrap_or_else(|| std::path::Path::new("."))
                .display()
                .to_string(),
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

    // Resolve history DB path (TUI opens per-thread connections; CLI opens eagerly below)
    let history_db_path = if args.no_history || !config.history_enabled() {
        None
    } else {
        let path = crate::history::resolve_db_path(Some(&config));
        // Run retention auto-prune on startup if configured
        if let Some(retention) = config.history_retention() {
            if let Ok(parsed) = crate::duration::parse_duration(retention) {
                if let Ok(cutoff) = parsed.to_cutoff_date() {
                    if let Ok(db) = crate::history::HistoryDb::open(&path) {
                        let statuses = config
                            .history
                            .as_ref()
                            .and_then(|h| h.retention_statuses.as_ref())
                            .map(|v| {
                                v.iter()
                                    .filter_map(|s| crate::history::SessionStatus::from_str(s))
                                    .collect::<Vec<_>>()
                            });
                        let status_slice = statuses.as_deref();
                        match db.prune(&cutoff, status_slice) {
                            Ok(0) => {}
                            Ok(n) => log::info!("history: pruned {} old sessions", n),
                            Err(e) => log::warn!("history: prune failed: {}", e),
                        }
                    }
                }
            }
        }
        Some(path)
    };

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

    // Resolve output directory: CLI flag > config > current directory
    if args.output.is_none() {
        args.output = config.output_dir.as_ref().map(PathBuf::from);
    }

    // Device resolution: in TUI multi-drive auto mode, leave args.device as None
    // so the coordinator's DriveMonitor can detect and manage all drives.
    // In CLI mode or TUI manual mode, resolve a single device.
    let multi_drive_auto = use_tui && !args.list_playlists && config.multi_drive_mode() == "auto";
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

    // Acquire per-device lock to prevent multiple bluback processes from contending
    let _device_lock = if let Some(ref dev) = args.device {
        Some(disc::try_lock_device(&dev.to_string_lossy())?)
    } else {
        None
    };

    // Resolve stream filter: CLI flags > config
    let stream_filter = if args.all_streams {
        crate::streams::StreamFilter::default() // empty = all streams
    } else if args.audio_lang.is_some() || args.subtitle_lang.is_some() || args.prefer_surround {
        crate::streams::StreamFilter {
            audio_languages: args
                .audio_lang
                .as_deref()
                .map(|s| s.split(',').map(|l| l.trim().to_string()).collect())
                .unwrap_or_default(),
            subtitle_languages: args
                .subtitle_lang
                .as_deref()
                .map(|s| s.split(',').map(|l| l.trim().to_string()).collect())
                .unwrap_or_default(),
            prefer_surround: args.prefer_surround,
        }
    } else {
        config.resolve_stream_filter()
    };
    let tracks_spec = args.tracks.clone();

    if args.list_playlists {
        cli::list_playlists(&args, &config)?;
        return Ok(EXIT_SUCCESS);
    }

    let headless = args.yes || (!atty_stdin() && !use_tui);
    let batch = config.should_batch(args.cli_batch());

    if use_tui {
        tui::run(&args, &config, config_path, &stream_filter, history_db_path)?;
    } else {
        // CLI mode: open DB eagerly (single-threaded, one connection is fine)
        let history_db =
            history_db_path
                .as_ref()
                .and_then(|path| match crate::history::HistoryDb::open(path) {
                    Ok(db) => Some(db),
                    Err(e) => {
                        log::warn!("failed to open history DB: {}", e);
                        None
                    }
                });

        if batch {
            cli::run_batch(
                &args,
                &config,
                headless,
                &stream_filter,
                tracks_spec.as_deref(),
                history_db.as_ref(),
                args.ignore_history,
            )?;
        } else {
            let _ = cli::run(
                &args,
                &config,
                headless,
                &stream_filter,
                tracks_spec.as_deref(),
                None,  // no start_episode override
                false, // don't skip eject
                history_db.as_ref(),
                args.ignore_history,
                None, // no batch_id
            )?;
        }
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
