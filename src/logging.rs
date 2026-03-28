use log::LevelFilter;
use std::path::{Path, PathBuf};

use crate::config::Config;

pub fn parse_level(s: &str) -> LevelFilter {
    match s {
        "error" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "info" => LevelFilter::Info,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Warn,
    }
}

pub fn rotate_logs(log_dir: &Path, max_files: usize) -> anyhow::Result<()> {
    if !log_dir.is_dir() {
        return Ok(());
    }

    let mut log_files: Vec<String> = std::fs::read_dir(log_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".log") {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    log_files.sort();

    if log_files.len() >= max_files {
        let to_delete = log_files.len() - max_files + 1;
        for name in &log_files[..to_delete] {
            let _ = std::fs::remove_file(log_dir.join(name));
        }
    }

    Ok(())
}

pub fn session_header(
    version: &str,
    device: Option<&str>,
    output_dir: &str,
    config_path: &Path,
    aacs_backend: &str,
) -> String {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let device_str = device.unwrap_or("auto-detect");

    format!(
        "=== bluback v{version} ===\n\
         Timestamp:    {timestamp}\n\
         Platform:     {os}/{arch}\n\
         Device:       {device_str}\n\
         Output dir:   {output_dir}\n\
         Config:       {config_path}\n\
         AACS backend: {aacs_backend}\n\
         ===",
        config_path = config_path.display(),
    )
}

pub fn init(
    config: &Config,
    stderr_level: LevelFilter,
    log_file_path: Option<PathBuf>,
    no_log: bool,
    is_tui: bool,
) -> anyhow::Result<Option<PathBuf>> {
    let file_logging = !no_log && config.log_file_enabled();

    let resolved_path = if file_logging {
        let custom = log_file_path.is_some();
        let path = match log_file_path {
            Some(p) => p,
            None => {
                let log_dir = config.log_dir();
                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                log_dir.join(format!("bluback_{timestamp}.log"))
            }
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            if !custom {
                rotate_logs(parent, config.max_log_files() as usize)?;
            }
        }

        Some(path)
    } else {
        None
    };

    let mut dispatch = fern::Dispatch::new();

    if let Some(ref path) = resolved_path {
        let file_dispatch = fern::Dispatch::new()
            .level(LevelFilter::Debug)
            .format(|out, message, record| {
                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                let level = record.level();
                let target = record.target();
                if target.starts_with("bluback") {
                    out.finish(format_args!("{timestamp} [{level}] {message}"));
                } else {
                    out.finish(format_args!("{timestamp} [{level}] [{target}] {message}"));
                }
            })
            .chain(fern::log_file(path)?);
        dispatch = dispatch.chain(file_dispatch);
    }

    if !is_tui && stderr_level != LevelFilter::Off {
        let stderr_dispatch = fern::Dispatch::new()
            .level(stderr_level)
            .format(|out, message, record| {
                match record.level() {
                    log::Level::Error => out.finish(format_args!("Error: {message}")),
                    log::Level::Warn => out.finish(format_args!("Warning: {message}")),
                    _ => out.finish(format_args!("{message}")),
                }
            })
            .chain(std::io::stderr());
        dispatch = dispatch.chain(stderr_dispatch);
    }

    dispatch.apply().map_err(|e| anyhow::anyhow!("failed to initialize logging: {e}"))?;

    Ok(resolved_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_level() {
        assert_eq!(parse_level("error"), LevelFilter::Error);
        assert_eq!(parse_level("warn"), LevelFilter::Warn);
        assert_eq!(parse_level("info"), LevelFilter::Info);
        assert_eq!(parse_level("debug"), LevelFilter::Debug);
        assert_eq!(parse_level("trace"), LevelFilter::Trace);
        assert_eq!(parse_level("garbage"), LevelFilter::Warn);
        assert_eq!(parse_level(""), LevelFilter::Warn);
    }

    #[test]
    fn test_rotate_logs_empty_dir() {
        let dir = TempDir::new().unwrap();
        rotate_logs(dir.path(), 10).unwrap();
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_rotate_logs_under_limit() {
        let dir = TempDir::new().unwrap();
        for i in 0..5 {
            std::fs::write(dir.path().join(format!("bluback_{i:02}.log")), "").unwrap();
        }
        rotate_logs(dir.path(), 10).unwrap();
        let count: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(count.len(), 5);
    }

    #[test]
    fn test_rotate_logs_at_limit_deletes_oldest() {
        let dir = TempDir::new().unwrap();
        for i in 0..10 {
            std::fs::write(dir.path().join(format!("bluback_{i:02}.log")), "").unwrap();
        }
        rotate_logs(dir.path(), 10).unwrap();
        let remaining: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        // Should have deleted the oldest to make room for a new one
        assert_eq!(remaining.len(), 9);
        assert!(!remaining.contains(&"bluback_00.log".to_string()));
    }

    #[test]
    fn test_rotate_logs_nonexistent_dir() {
        let path = Path::new("/tmp/bluback_test_nonexistent_dir_xyz");
        assert!(!path.exists());
        let result = rotate_logs(path, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_rotate_logs_ignores_non_log_files() {
        let dir = TempDir::new().unwrap();
        for i in 0..10 {
            std::fs::write(dir.path().join(format!("bluback_{i:02}.log")), "").unwrap();
        }
        std::fs::write(dir.path().join("notes.txt"), "keep me").unwrap();
        std::fs::write(dir.path().join("data.json"), "{}").unwrap();

        rotate_logs(dir.path(), 10).unwrap();

        let remaining: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();

        assert!(remaining.contains(&"notes.txt".to_string()));
        assert!(remaining.contains(&"data.json".to_string()));
        // 9 log files remain + 2 non-log files
        assert_eq!(remaining.len(), 11);
    }

    #[test]
    fn test_session_header_format() {
        let header = session_header(
            "0.9.2",
            Some("/dev/sr0"),
            "/home/user/rips",
            Path::new("/home/user/.config/bluback/config.toml"),
            "libaacs",
        );

        assert!(header.contains("bluback v0.9.2"));
        assert!(header.contains("/dev/sr0"));
        assert!(header.contains("/home/user/rips"));
        assert!(header.contains("config.toml"));
        assert!(header.contains("libaacs"));
        assert!(header.contains(std::env::consts::OS));
        assert!(header.contains(std::env::consts::ARCH));
        assert!(header.contains("Timestamp:"));

        // Test with no device
        let header_no_dev = session_header(
            "0.9.2",
            None,
            ".",
            Path::new("/tmp/config.toml"),
            "auto",
        );
        assert!(header_no_dev.contains("auto-detect"));
    }
}
