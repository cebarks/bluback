use crate::config::AacsBackend;
use anyhow::{bail, Result};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

/// Process group IDs from forked scan children. When the scan child exits,
/// makemkvcon is orphaned (PPid → init) but remains in the child's process group.
/// We track these PGIDs so `kill_makemkvcon_children` can SIGKILL any stragglers
/// that survived the initial process group kill.
static SCAN_PGIDS: Mutex<Vec<i32>> = Mutex::new(Vec::new());

/// Register a scan child's PGID for later cleanup.
pub fn register_scan_pgid(pgid: i32) {
    if let Ok(mut list) = SCAN_PGIDS.try_lock() {
        list.push(pgid);
    }
}

/// Check if a command exists in PATH.
pub fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Search for a shared library using ldconfig, falling back to known paths.
pub fn find_library(name: &str, known_paths: &[&str]) -> Option<PathBuf> {
    // Try ldconfig -p first
    if let Ok(output) = Command::new("ldconfig").arg("-p").output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if line.contains(name) {
                    // ldconfig output format: "    libname.so.0 (libc6,x86-64) => /usr/lib64/libname.so.0"
                    if let Some(path) = line.split("=>").nth(1) {
                        let path = PathBuf::from(path.trim());
                        if path.exists() {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }
    // Fallback to known paths
    for path in known_paths {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Check if a library path is actually libmmbd (via symlink resolution).
pub fn is_libmmbd(path: &std::path::Path) -> bool {
    match std::fs::canonicalize(path) {
        Ok(real) => real
            .file_name()
            .map(|f| f.to_string_lossy().contains("mmbd"))
            .unwrap_or(false),
        Err(_) => false,
    }
}

pub const LIBMMBD_PATHS: &[&str] = &[
    // Linux
    "/usr/lib64/libmmbd.so.0",
    "/usr/lib/x86_64-linux-gnu/libmmbd.so.0",
    "/usr/lib/libmmbd.so.0",
    // macOS (Homebrew)
    "/opt/homebrew/lib/libmmbd.dylib",
    "/usr/local/lib/libmmbd.dylib",
];

pub const LIBAACS_PATHS: &[&str] = &[
    // Linux
    "/usr/lib64/libaacs.so.0",
    "/usr/lib/x86_64-linux-gnu/libaacs.so.0",
    "/usr/lib/libaacs.so.0",
    // macOS (Homebrew)
    "/opt/homebrew/lib/libaacs.dylib",
    "/usr/local/lib/libaacs.dylib",
];

pub const LIBBLURAY_PATHS: &[&str] = &[
    // Linux
    "/usr/lib64/libbluray.so.2",
    "/usr/lib/x86_64-linux-gnu/libbluray.so.2",
    "/usr/lib/libbluray.so.2",
    // macOS (Homebrew)
    "/opt/homebrew/lib/libbluray.dylib",
    "/usr/local/lib/libbluray.dylib",
];

/// Run AACS backend preflight checks. Call before any FFmpeg/libbluray init.
pub fn preflight(backend: AacsBackend) -> Result<()> {
    // On macOS, libbluray uses dlopen() to load libaacs/libmmbd by name, but
    // Homebrew's /opt/homebrew/lib/ isn't in the default dyld search path.
    // Extend DYLD_LIBRARY_PATH so dlopen("libmmbd.dylib") finds Homebrew libs.
    #[cfg(target_os = "macos")]
    {
        let mut dyld_path = std::env::var("DYLD_LIBRARY_PATH").unwrap_or_default();
        for dir in ["/opt/homebrew/lib", "/usr/local/lib"] {
            if !dyld_path.contains(dir) {
                if !dyld_path.is_empty() {
                    dyld_path.push(':');
                }
                dyld_path.push_str(dir);
            }
        }
        std::env::set_var("DYLD_LIBRARY_PATH", &dyld_path);
    }

    match backend {
        AacsBackend::Libmmbd => {
            if !command_exists("makemkvcon") {
                bail!(
                    "aacs_backend is set to libmmbd but makemkvcon was not found in PATH. \
                     Install MakeMKV or set aacs_backend = \"auto\"."
                );
            }
            // libbluray's dl_dlopen appends ".so.{version}" to the name,
            // so LIBAACS_PATH must be a library NAME (e.g. "libmmbd"),
            // NOT a full path. A full path like "/lib64/libmmbd.so.0"
            // becomes "/lib64/libmmbd.so.0.so.0" and silently fails.
            if find_library("libmmbd", LIBMMBD_PATHS).is_some() {
                std::env::set_var("LIBAACS_PATH", "libmmbd");
                std::env::set_var("LIBBDPLUS_PATH", "libmmbd");
            }
            Ok(())
        }
        AacsBackend::Libaacs => {
            if let Some(path) = find_library("libaacs", LIBAACS_PATHS) {
                if is_libmmbd(&path) {
                    log::warn!(
                        "system libaacs.so is a symlink to libmmbd — searching for real libaacs"
                    );
                    // Try to force real libaacs by name — libbluray's dl_dlopen
                    // appends ".so.{version}", so we pass a library name, not path.
                    std::env::set_var("LIBAACS_PATH", "libaacs");
                } else {
                    // Real libaacs found, set by name (not path)
                    std::env::set_var("LIBAACS_PATH", "libaacs");
                }
            }
            let keydb = dirs_keydb_path();
            if !keydb.exists() {
                log::warn!(
                    "KEYDB.cfg not found at {} — AACS decryption may fail",
                    keydb.display()
                );
            }
            Ok(())
        }
        AacsBackend::Auto => {
            // Detect if libmmbd is masquerading as libaacs
            if let Some(path) = find_library("libaacs", LIBAACS_PATHS) {
                if is_libmmbd(&path) && !command_exists("makemkvcon") {
                    log::warn!(
                        "libmmbd.so is installed as libaacs but makemkvcon was not found — AACS initialization may hang. Consider setting aacs_backend = \"libaacs\" in config"
                    );
                }
            }
            Ok(())
        }
    }
}

pub fn dirs_keydb_path() -> PathBuf {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    home.join(".config").join("aacs").join("KEYDB.cfg")
}

/// Reap zombie child processes (makemkvcon cleanup).
pub fn reap_children() {
    use std::ptr;
    unsafe {
        // Loop waitpid(-1, WNOHANG) until no more children to reap
        loop {
            let ret = libc::waitpid(-1, ptr::null_mut(), libc::WNOHANG);
            if ret <= 0 {
                break;
            }
        }
    }
}

/// Kill any orphaned makemkvcon child processes of the current process,
/// plus any remaining in scan-forked process groups.
///
/// When using the libmmbd AACS backend, libbluray spawns makemkvcon via IPC
/// each time a bluray device is opened (count_streams, probe_playlist, remux).
/// These processes aren't always cleaned up when the FFmpeg context is dropped,
/// leaving orphans that can interfere with subsequent device opens and survive
/// past bluback's exit.
///
/// Two cleanup strategies:
/// 1. Direct children: scan /proc for makemkvcon with PPid == our PID
/// 2. Scan fork orphans: SIGKILL process groups from forked scan children
///    (makemkvcon is orphaned to init when the scan child exits, but remains
///    in the child's process group)
#[cfg(target_os = "linux")]
pub fn kill_makemkvcon_children() {
    let our_pid = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Ok(pid) = name.to_string_lossy().parse::<i32>() else {
            continue;
        };

        let status_path = format!("/proc/{}/status", pid);
        let Ok(status) = std::fs::read_to_string(&status_path) else {
            continue;
        };

        let is_our_child = status.lines().any(|line| {
            line.strip_prefix("PPid:")
                .and_then(|rest| rest.trim().parse::<u32>().ok())
                == Some(our_pid)
        });

        if !is_our_child {
            continue;
        }

        let comm_path = format!("/proc/{}/comm", pid);
        let Ok(comm) = std::fs::read_to_string(&comm_path) else {
            continue;
        };

        if comm.trim() == "makemkvcon" {
            log::debug!("Killing orphaned makemkvcon child (pid {})", pid);
            unsafe {
                libc::kill(pid, libc::SIGKILL);
                libc::waitpid(pid, std::ptr::null_mut(), 0);
            }
        }
    }

    // Kill any makemkvcon that survived in scan-forked process groups.
    // These are orphaned (PPid == 1) and invisible to the PPid check above.
    // Uses try_lock to avoid deadlocking when called from atexit handlers
    // or concurrent signal contexts.
    if let Ok(mut pgids) = SCAN_PGIDS.try_lock() {
        for pgid in pgids.drain(..) {
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn kill_makemkvcon_children() {
    // No /proc filesystem on macOS; makemkvcon orphan cleanup is not
    // needed since macOS doesn't use the fork-based scan path and
    // libbluray/libmmbd behavior differs.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_libmmbd_non_mmbd_path() {
        // A path that doesn't contain "mmbd" after canonicalization
        assert!(!is_libmmbd(std::path::Path::new("/usr/lib64/libaacs.so.0")));
    }

    #[test]
    fn test_command_exists_true() {
        // "ls" should always exist
        assert!(command_exists("ls"));
    }

    #[test]
    fn test_command_exists_false() {
        assert!(!command_exists("definitely_not_a_real_command_xyz"));
    }
}
