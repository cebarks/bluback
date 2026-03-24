use crate::config::AacsBackend;
use anyhow::{bail, Result};
use std::path::PathBuf;
use std::process::Command;

/// Check if a command exists in PATH.
fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Search for a shared library using ldconfig, falling back to known paths.
fn find_library(name: &str, known_paths: &[&str]) -> Option<PathBuf> {
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
fn is_libmmbd(path: &std::path::Path) -> bool {
    match std::fs::canonicalize(path) {
        Ok(real) => real
            .file_name()
            .map(|f| f.to_string_lossy().contains("mmbd"))
            .unwrap_or(false),
        Err(_) => false,
    }
}

const LIBMMBD_PATHS: &[&str] = &[
    "/usr/lib64/libmmbd.so.0",
    "/usr/lib/x86_64-linux-gnu/libmmbd.so.0",
    "/usr/lib/libmmbd.so.0",
];

const LIBAACS_PATHS: &[&str] = &[
    "/usr/lib64/libaacs.so.0",
    "/usr/lib/x86_64-linux-gnu/libaacs.so.0",
    "/usr/lib/libaacs.so.0",
];

/// Run AACS backend preflight checks. Call before any FFmpeg/libbluray init.
pub fn preflight(backend: AacsBackend) -> Result<()> {
    match backend {
        AacsBackend::Libmmbd => {
            if !command_exists("makemkvcon") {
                bail!(
                    "aacs_backend is set to libmmbd but makemkvcon was not found in PATH. \
                     Install MakeMKV or set aacs_backend = \"auto\"."
                );
            }
            if let Some(path) = find_library("libmmbd", LIBMMBD_PATHS) {
                let path_str = path.to_string_lossy().to_string();
                std::env::set_var("LIBAACS_PATH", &path_str);
                std::env::set_var("LIBBDPLUS_PATH", &path_str);
            }
            Ok(())
        }
        AacsBackend::Libaacs => {
            if let Some(path) = find_library("libaacs", LIBAACS_PATHS) {
                if is_libmmbd(&path) {
                    eprintln!(
                        "Warning: system libaacs.so is a symlink to libmmbd. \
                         Searching for real libaacs..."
                    );
                    // Try to find the real libaacs by checking each known path
                    for candidate in LIBAACS_PATHS {
                        let p = PathBuf::from(candidate);
                        if p.exists() && !is_libmmbd(&p) {
                            std::env::set_var("LIBAACS_PATH", candidate);
                            break;
                        }
                    }
                } else {
                    std::env::set_var("LIBAACS_PATH", path.to_string_lossy().as_ref());
                }
            }
            let keydb = dirs_keydb_path();
            if !keydb.exists() {
                eprintln!(
                    "Warning: KEYDB.cfg not found at {} — AACS decryption may fail.",
                    keydb.display()
                );
            }
            Ok(())
        }
        AacsBackend::Auto => {
            // Detect if libmmbd is masquerading as libaacs
            if let Some(path) = find_library("libaacs", LIBAACS_PATHS) {
                if is_libmmbd(&path) && !command_exists("makemkvcon") {
                    eprintln!(
                        "Warning: libmmbd.so is installed as libaacs but makemkvcon was not found. \
                         AACS initialization may hang. Consider setting aacs_backend = \"libaacs\" in config."
                    );
                }
            }
            Ok(())
        }
    }
}

fn dirs_keydb_path() -> PathBuf {
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
