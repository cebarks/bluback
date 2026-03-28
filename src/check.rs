#[derive(Debug)]
enum CheckStatus {
    Pass,
    Fail,
    Warn,
    Miss,
}

struct CheckResult {
    label: String,
    status: CheckStatus,
    detail: String,
}

impl CheckResult {
    fn tag(&self) -> &str {
        match self.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Fail => "FAIL",
            CheckStatus::Warn => "WARN",
            CheckStatus::Miss => "MISS",
        }
    }
}

fn print_results(results: &[CheckResult]) {
    for r in results {
        if r.detail.is_empty() {
            println!("[{}] {}", r.tag(), r.label);
        } else {
            println!("[{}] {} ({})", r.tag(), r.label, r.detail);
        }
    }
}

fn check_library(
    results: &mut Vec<CheckResult>,
    name: &str,
    known_paths: &[&str],
    required: bool,
    any_failed: &mut bool,
) {
    if let Some(path) = crate::aacs::find_library(name, known_paths) {
        results.push(CheckResult {
            label: name.to_string(),
            status: CheckStatus::Pass,
            detail: path.display().to_string(),
        });
    } else if required {
        results.push(CheckResult {
            label: name.to_string(),
            status: CheckStatus::Fail,
            detail: "not found".into(),
        });
        *any_failed = true;
    } else {
        results.push(CheckResult {
            label: name.to_string(),
            status: CheckStatus::Miss,
            detail: format!(
                "optional — needed for aacs_backend={}",
                name.trim_start_matches("lib")
            ),
        });
    }
}

fn check_command(
    results: &mut Vec<CheckResult>,
    name: &str,
    required: bool,
    hint: &str,
    any_failed: &mut bool,
) {
    if crate::aacs::command_exists(name) {
        results.push(CheckResult {
            label: name.to_string(),
            status: CheckStatus::Pass,
            detail: String::new(),
        });
    } else if required {
        results.push(CheckResult {
            label: name.to_string(),
            status: CheckStatus::Fail,
            detail: format!("not found — {}", hint),
        });
        *any_failed = true;
    } else {
        results.push(CheckResult {
            label: name.to_string(),
            status: CheckStatus::Miss,
            detail: format!("optional — {}", hint),
        });
    }
}

pub fn run_check(config: &crate::config::Config, config_path: &std::path::Path) -> i32 {
    let mut results = Vec::new();
    let mut any_required_failed = false;

    // Check 1: FFmpeg libraries (required)
    match ffmpeg_the_third::init() {
        Ok(()) => results.push(CheckResult {
            label: "FFmpeg libraries".into(),
            status: CheckStatus::Pass,
            detail: String::new(),
        }),
        Err(e) => {
            results.push(CheckResult {
                label: "FFmpeg libraries".into(),
                status: CheckStatus::Fail,
                detail: format!("{}", e),
            });
            any_required_failed = true;
        }
    }

    // Check 2: libbluray (required)
    check_library(
        &mut results,
        "libbluray",
        crate::aacs::LIBBLURAY_PATHS,
        true,
        &mut any_required_failed,
    );

    // Check 3: libaacs (required)
    check_library(
        &mut results,
        "libaacs",
        crate::aacs::LIBAACS_PATHS,
        true,
        &mut any_required_failed,
    );

    // Check 4: KEYDB.cfg (required)
    let keydb = crate::aacs::dirs_keydb_path();
    if keydb.exists() {
        results.push(CheckResult {
            label: "KEYDB.cfg".into(),
            status: CheckStatus::Pass,
            detail: keydb.display().to_string(),
        });
    } else {
        results.push(CheckResult {
            label: "KEYDB.cfg".into(),
            status: CheckStatus::Fail,
            detail: format!("not found at {}", keydb.display()),
        });
        any_required_failed = true;
    }

    // Check 5: libmmbd (optional)
    check_library(
        &mut results,
        "libmmbd",
        crate::aacs::LIBMMBD_PATHS,
        false,
        &mut any_required_failed,
    );

    // Check 6: makemkvcon (optional)
    check_command(
        &mut results,
        "makemkvcon",
        false,
        "needed for aacs_backend=libmmbd",
        &mut any_required_failed,
    );

    // Check 7: Mount utility (required, platform-specific)
    #[cfg(target_os = "linux")]
    check_command(
        &mut results,
        "udisksctl",
        true,
        "needed for disc mount/chapter extraction",
        &mut any_required_failed,
    );
    #[cfg(target_os = "macos")]
    check_command(
        &mut results,
        "diskutil",
        true,
        "needed for disc mount/volume info",
        &mut any_required_failed,
    );

    // Check 8: Optical drives (optional)
    let drives = crate::disc::detect_optical_drives();
    if drives.is_empty() {
        results.push(CheckResult {
            label: "Optical drives".into(),
            status: CheckStatus::Miss,
            detail: "no drives detected".into(),
        });
    } else {
        let drive_list = drives
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        results.push(CheckResult {
            label: "Optical drives".into(),
            status: CheckStatus::Pass,
            detail: drive_list,
        });
    }

    // Check 9: Drive permissions (skip if no drives)
    for drive in &drives {
        match std::fs::File::open(drive) {
            Ok(_) => {
                results.push(CheckResult {
                    label: "Drive permissions".into(),
                    status: CheckStatus::Pass,
                    detail: format!("{} — readable", drive.display()),
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                results.push(CheckResult {
                    label: "Drive permissions".into(),
                    status: CheckStatus::Fail,
                    detail: format!(
                        "{} — permission denied, check cdrom/optical group",
                        drive.display()
                    ),
                });
            }
            Err(_) => {
                results.push(CheckResult {
                    label: "Drive permissions".into(),
                    status: CheckStatus::Pass,
                    detail: format!("{} — accessible", drive.display()),
                });
            }
        }
    }

    // Check 10: Output directory (required)
    let output_dir = config.output_dir.as_deref().unwrap_or(".");
    let output_path = std::path::Path::new(output_dir);
    if output_path.exists() && output_path.is_dir() {
        let test_file = output_path.join(".bluback_check_tmp");
        match std::fs::File::create(&test_file) {
            Ok(_) => {
                let _ = std::fs::remove_file(&test_file);
                results.push(CheckResult {
                    label: "Output directory".into(),
                    status: CheckStatus::Pass,
                    detail: format!("{} — writable", output_dir),
                });
            }
            Err(_) => {
                results.push(CheckResult {
                    label: "Output directory".into(),
                    status: CheckStatus::Fail,
                    detail: format!("{} — not writable", output_dir),
                });
                any_required_failed = true;
            }
        }
    } else if !output_path.exists() {
        results.push(CheckResult {
            label: "Output directory".into(),
            status: CheckStatus::Warn,
            detail: format!("{} — does not exist (will be created on rip)", output_dir),
        });
    } else {
        results.push(CheckResult {
            label: "Output directory".into(),
            status: CheckStatus::Fail,
            detail: format!("{} — not a directory", output_dir),
        });
        any_required_failed = true;
    }

    // Check 11: TMDb API key (optional)
    if config.tmdb_api_key().is_some() {
        results.push(CheckResult {
            label: "TMDb API key".into(),
            status: CheckStatus::Pass,
            detail: "configured".into(),
        });
    } else {
        results.push(CheckResult {
            label: "TMDb API key".into(),
            status: CheckStatus::Warn,
            detail: "not configured — TMDb lookup will be unavailable".into(),
        });
    }

    // Check 12: Config file (optional)
    if config_path.exists() {
        match std::fs::read_to_string(config_path) {
            Ok(raw) => {
                let mut config_warnings = crate::config::validate_raw_toml(&raw);
                config_warnings.extend(crate::config::validate_config(config));
                if config_warnings.is_empty() {
                    results.push(CheckResult {
                        label: "Config file".into(),
                        status: CheckStatus::Pass,
                        detail: format!("{} — valid", config_path.display()),
                    });
                } else {
                    results.push(CheckResult {
                        label: "Config file".into(),
                        status: CheckStatus::Warn,
                        detail: config_warnings.join("; "),
                    });
                }
            }
            Err(e) => {
                results.push(CheckResult {
                    label: "Config file".into(),
                    status: CheckStatus::Fail,
                    detail: format!("cannot read: {}", e),
                });
            }
        }
    } else {
        results.push(CheckResult {
            label: "Config file".into(),
            status: CheckStatus::Pass,
            detail: "no config file — using defaults".into(),
        });
    }

    // Log directory
    let log_dir = config.log_dir();
    if log_dir.exists() {
        let log_count = std::fs::read_dir(&log_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
                    .count()
            })
            .unwrap_or(0);
        results.push(CheckResult {
            label: "Log directory".into(),
            status: CheckStatus::Pass,
            detail: format!("{} ({} logs)", log_dir.display(), log_count),
        });
    } else {
        results.push(CheckResult {
            label: "Log directory".into(),
            status: CheckStatus::Warn,
            detail: format!("{} (will be created on first run)", log_dir.display()),
        });
    }

    print_results(&results);
    if any_required_failed {
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_result_tags() {
        assert_eq!(
            CheckResult {
                label: "t".into(),
                status: CheckStatus::Pass,
                detail: String::new()
            }
            .tag(),
            "PASS"
        );
        assert_eq!(
            CheckResult {
                label: "t".into(),
                status: CheckStatus::Fail,
                detail: String::new()
            }
            .tag(),
            "FAIL"
        );
        assert_eq!(
            CheckResult {
                label: "t".into(),
                status: CheckStatus::Warn,
                detail: String::new()
            }
            .tag(),
            "WARN"
        );
        assert_eq!(
            CheckResult {
                label: "t".into(),
                status: CheckStatus::Miss,
                detail: String::new()
            }
            .tag(),
            "MISS"
        );
    }
}
