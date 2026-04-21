use anyhow::{bail, Result};
use regex::Regex;
use std::process::Command;
use std::sync::LazyLock;

use std::fs::File;
use std::io::{Read as _, Seek, SeekFrom, Write as _};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

use crate::types::{InputSource, LabelInfo};

static LABEL_PATTERNS: LazyLock<[Regex; 2]> = LazyLock::new(|| {
    [
        Regex::new(r"(?i)^(?P<show>.+?)_?SEASON(?P<season>\d+)_?DISC(?P<disc>\d+)")
            .expect("valid regex"),
        Regex::new(r"(?i)^(?P<show>.+?)_S(?P<season>\d+)_?D(?P<disc>\d+)").expect("valid regex"),
    ]
});

/// Return all optical drives found on the system.
#[cfg(target_os = "linux")]
pub fn detect_optical_drives() -> Vec<std::path::PathBuf> {
    let output = Command::new("lsblk")
        .args(["-rno", "NAME,TYPE"])
        .output()
        .ok();

    let mut drives = Vec::new();
    if let Some(out) = output {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let mut parts = line.split_whitespace();
                if let (Some(name), Some(typ)) = (parts.next(), parts.next()) {
                    if typ == "rom" {
                        drives.push(std::path::PathBuf::from(format!("/dev/{}", name)));
                    }
                }
            }
        }
    }

    log::debug!("Detected {} optical drives", drives.len());
    drives
}

#[cfg(target_os = "macos")]
pub fn detect_optical_drives() -> Vec<std::path::PathBuf> {
    let mut drives = Vec::new();

    // "drutil list" enumerates all optical drives with 1-based indices:
    //   1  MATSHITA BD-MLT  UJ272    ST04 External
    //   2  HL-DT-ST DVDRAM GP65NS60 YP00 External
    // Then "drutil status -drive N" gives each drive's device path.
    let list_output = match Command::new("drutil").arg("list").output() {
        Ok(o) if o.status.success() => o,
        _ => {
            // Fallback: try plain "drutil status" for single-drive systems
            return detect_optical_drives_single();
        }
    };

    let list_text = String::from_utf8_lossy(&list_output.stdout);
    let indices: Vec<u32> = list_text
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            // Lines starting with a digit are drive entries
            trimmed.split_whitespace().next()?.parse::<u32>().ok()
        })
        .collect();

    if indices.is_empty() {
        return drives;
    }

    for idx in &indices {
        if let Ok(status) = Command::new("drutil")
            .args(["status", "-drive", &idx.to_string()])
            .output()
        {
            if status.status.success() {
                let text = String::from_utf8_lossy(&status.stdout);
                for line in text.lines() {
                    if let Some(pos) = line.find("Name:") {
                        let after = line[pos + 5..].trim();
                        if after.starts_with("/dev/") {
                            drives.push(std::path::PathBuf::from(after));
                        }
                    }
                }
            }
        }
    }

    log::debug!("Detected {} optical drives", drives.len());
    drives
}

/// Fallback: single-drive detection via plain "drutil status"
#[cfg(target_os = "macos")]
fn detect_optical_drives_single() -> Vec<std::path::PathBuf> {
    if let Ok(output) = Command::new("drutil").arg("status").output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if let Some(idx) = line.find("Name:") {
                    let after = line[idx + 5..].trim();
                    if after.starts_with("/dev/") {
                        return vec![std::path::PathBuf::from(after)];
                    }
                }
            }
        }
    }
    Vec::new()
}

/// Get the mount point of a device if it's already mounted.
#[cfg(target_os = "linux")]
pub fn get_mount_point(device: &str) -> Option<String> {
    let output = Command::new("findmnt")
        .args(["-n", "-o", "TARGET", device])
        .output()
        .ok()?;

    if output.status.success() {
        let mount = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if mount.is_empty() {
            None
        } else {
            Some(mount)
        }
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
pub fn get_mount_point(device: &str) -> Option<String> {
    let output = Command::new("diskutil")
        .args(["info", device])
        .output()
        .ok()?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.trim().starts_with("Mount Point:") {
                let mount = line.split(':').nth(1)?.trim().to_string();
                if mount.is_empty() || mount == "(not mounted)" {
                    return None;
                }
                return Some(mount);
            }
        }
    }
    None
}

/// Mount a disc. Returns the mount point on success.
#[cfg(target_os = "linux")]
pub fn mount_disc(device: &str) -> Result<String> {
    let output = Command::new("udisksctl")
        .args(["mount", "-b", device])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to mount {}: {}", device, stderr.trim());
    }

    // udisksctl output: "Mounted /dev/sr0 at /run/media/user/LABEL."
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mount_path = stdout
        .split(" at ")
        .nth(1)
        .map(|s| s.trim().trim_end_matches('.').to_string())
        .ok_or_else(|| anyhow::anyhow!("Could not parse mount point from udisksctl output"))?;
    log::info!("Disc mounted at {}", mount_path);
    Ok(mount_path)
}

#[cfg(target_os = "macos")]
pub fn mount_disc(device: &str) -> Result<String> {
    // Check if already mounted (macOS auto-mounts optical media)
    if let Some(mount) = get_mount_point(device) {
        log::info!("Disc mounted at {}", mount);
        return Ok(mount);
    }

    // Try to mount it manually
    let output = Command::new("diskutil").args(["mount", device]).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to mount {}: {}", device, stderr.trim());
    }

    // diskutil output: "Volume <LABEL> on <device> mounted"
    // Get the mount point via diskutil info
    let mount_path = get_mount_point(device)
        .ok_or_else(|| anyhow::anyhow!("Mounted {} but could not find mount point", device))?;
    log::info!("Disc mounted at {}", mount_path);
    Ok(mount_path)
}

/// Unmount a disc.
#[cfg(target_os = "linux")]
pub fn unmount_disc(device: &str) -> Result<()> {
    let output = Command::new("udisksctl")
        .args(["unmount", "-b", device])
        .output()?;

    if !output.status.success() {
        bail!("Failed to unmount {}", device);
    }
    log::debug!("Disc unmounted");
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn unmount_disc(device: &str) -> Result<()> {
    let output = Command::new("diskutil")
        .args(["unmount", device])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to unmount {}: {}", device, stderr.trim());
    }
    log::debug!("Disc unmounted");
    Ok(())
}

/// Ensure the disc is mounted, returning (mount_point, did_we_mount_it).
/// If it was already mounted, returns the existing mount point.
/// If we mounted it, the caller should unmount when done.
/// If `device` is a directory with a BDMV/ subdirectory, treats it as
/// already mounted (useful for testing with directory paths instead of
/// block devices).
pub fn ensure_mounted(device: &str) -> Result<(String, bool)> {
    // Directory path with BDMV structure: use directly as mount point
    let path = std::path::Path::new(device);
    if path.is_dir() && path.join("BDMV").is_dir() {
        return Ok((device.to_string(), false));
    }

    if let Some(mount) = get_mount_point(device) {
        Ok((mount, false))
    } else {
        let mount = mount_disc(device)?;
        Ok((mount, true))
    }
}

#[cfg(target_os = "linux")]
pub fn get_volume_label(device: &str) -> String {
    let label = Command::new("lsblk")
        .args(["-no", "LABEL", device])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    if !label.is_empty() {
        log::info!("Disc detected: {}", label);
    }
    label
}

#[cfg(target_os = "macos")]
pub fn get_volume_label(device: &str) -> String {
    let label = Command::new("diskutil")
        .args(["info", device])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines() {
                    if line.trim().starts_with("Volume Name:") {
                        return line.split_once(':').map(|(_, v)| v.trim().to_string());
                    }
                }
            }
            None
        })
        .unwrap_or_default();
    if !label.is_empty() {
        log::info!("Disc detected: {}", label);
    }
    label
}

pub fn parse_volume_label(label: &str) -> Option<LabelInfo> {
    if label.is_empty() {
        return None;
    }
    for re in LABEL_PATTERNS.iter() {
        if let Some(caps) = re.captures(label) {
            let show = caps["show"].trim_matches('_').replace('_', " ");
            let season: u32 = caps["season"]
                .parse()
                .expect("regex guarantees numeric capture");
            let disc: u32 = caps["disc"]
                .parse()
                .expect("regex guarantees numeric capture");
            return Some(LabelInfo { show, season, disc });
        }
    }
    None
}

/// Resolve an input path to either a disc device or a folder containing BDMV structure.
///
/// For directories: validates that a `BDMV/` subdirectory exists.
/// For non-directories (e.g., `/dev/sr0`): assumes a block device.
#[allow(dead_code)] // Wired in Task 5 (main.rs)
pub fn resolve_input_source(path: &std::path::Path) -> anyhow::Result<InputSource> {
    if path.is_dir() {
        if path.join("BDMV").is_dir() {
            Ok(InputSource::Folder {
                path: path.to_path_buf(),
            })
        } else {
            bail!(
                "Directory '{}' does not contain a BDMV structure. \
                 Expected a 'BDMV/' subdirectory.",
                path.display()
            )
        }
    } else {
        Ok(InputSource::Disc {
            device: path.to_path_buf(),
        })
    }
}

/// Parse disc title from BDMV/META/DL/bdmt_*.xml metadata.
///
/// Prefers `bdmt_eng.xml` if present; otherwise uses the first bdmt file found.
/// Returns the content of the `<di:name>` element.
#[allow(dead_code)] // Used in later tasks
pub fn parse_bdmt_title(bdmv_root: &std::path::Path) -> Option<String> {
    let meta_dir = bdmv_root.join("BDMV").join("META").join("DL");
    if !meta_dir.is_dir() {
        return None;
    }

    let entries: Vec<_> = std::fs::read_dir(&meta_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("bdmt_") && name.ends_with(".xml")
        })
        .collect();

    if entries.is_empty() {
        return None;
    }

    // Prefer English, fall back to first found
    let target = entries
        .iter()
        .find(|e| e.file_name().to_string_lossy() == "bdmt_eng.xml")
        .or_else(|| entries.first())?;

    parse_bdmt_xml(&target.path())
}

#[allow(dead_code)] // Used by parse_bdmt_title
fn parse_bdmt_xml(path: &std::path::Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;

    // Strip UTF-8 BOM if present
    let data = if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &data[3..]
    } else {
        &data
    };

    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_reader(data);
    let mut in_name = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let local = e.local_name();
                if local.as_ref() == b"name" {
                    in_name = true;
                }
            }
            Ok(Event::Text(e)) if in_name => {
                let text = e.unescape().ok()?.trim().to_string();
                if !text.is_empty() {
                    return Some(text);
                }
                in_name = false;
            }
            Ok(Event::End(_)) if in_name => {
                in_name = false;
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }

    None
}

#[cfg(target_os = "linux")]
pub fn set_max_speed(device: &str) {
    let _ = Command::new("eject").args(["-x", "0", device]).status();
}

#[cfg(target_os = "macos")]
pub fn set_max_speed(_device: &str) {
    // No direct equivalent on macOS — drive speed is auto-negotiated.
}

pub struct MountGuard {
    device: String,
    mounted_by_us: bool,
}

impl MountGuard {
    pub fn new(device: &str, mounted_by_us: bool) -> Self {
        Self {
            device: device.to_string(),
            mounted_by_us,
        }
    }

    pub fn cleanup(&mut self) {
        if self.mounted_by_us {
            let _ = unmount_disc(&self.device);
            self.mounted_by_us = false;
        }
    }
}

impl Drop for MountGuard {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(target_os = "linux")]
pub fn eject_disc(device: &str) -> anyhow::Result<()> {
    let status = Command::new("eject").arg(device).status()?;

    if !status.success() {
        bail!("eject exited with code {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn eject_disc(device: &str) -> anyhow::Result<()> {
    let status = Command::new("diskutil").args(["eject", device]).status()?;

    if !status.success() {
        bail!(
            "diskutil eject exited with code {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

/// Acquire an exclusive per-device lock so only one bluback process accesses a drive at a time.
/// Returns a File guard — the lock is held until the guard is dropped (or the process exits).
pub fn try_lock_device(device: &str) -> anyhow::Result<File> {
    let device_name = std::path::Path::new(device)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let lock_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());

    let lock_path = lock_dir.join(format!("bluback-{}.lock", device_name));

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| anyhow::anyhow!("Failed to open lock file {}: {}", lock_path.display(), e))?;

    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 {
        let mut contents = String::new();
        let _ = file.read_to_string(&mut contents);
        let detail = contents
            .trim()
            .parse::<u32>()
            .ok()
            .map(|pid| format!(" (PID {pid})"))
            .unwrap_or_default();
        bail!(
            "Another bluback process{detail} is already using {device}. \
             Only one instance can access a device at a time."
        );
    }

    // Write our PID so other instances can report it
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    let _ = writeln!(file, "{}", std::process::id());

    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_label_sxdy() {
        let info = parse_volume_label("SGU_BR_S1D2").unwrap();
        assert_eq!(info.show, "SGU BR");
        assert_eq!(info.season, 1);
        assert_eq!(info.disc, 2);
    }

    #[test]
    fn test_parse_label_underscore_separated() {
        let info = parse_volume_label("SHOW_S1_D2").unwrap();
        assert_eq!(info.show, "SHOW");
        assert_eq!(info.season, 1);
        assert_eq!(info.disc, 2);
    }

    #[test]
    fn test_parse_label_long_form() {
        let info = parse_volume_label("SHOW_SEASON1_DISC2").unwrap();
        assert_eq!(info.show, "SHOW");
        assert_eq!(info.season, 1);
        assert_eq!(info.disc, 2);
    }

    #[test]
    fn test_parse_label_no_match() {
        assert!(parse_volume_label("RANDOM_DISC").is_none());
    }

    #[test]
    fn test_parse_label_empty() {
        assert!(parse_volume_label("").is_none());
    }

    #[test]
    fn test_parse_label_show_with_underscores() {
        let info = parse_volume_label("THE_WIRE_S3D1").unwrap();
        assert_eq!(info.show, "THE WIRE");
        assert_eq!(info.season, 3);
        assert_eq!(info.disc, 1);
    }

    #[test]
    fn parse_bdmt_title_valid_xml() {
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join("BDMV/META/DL");
        std::fs::create_dir_all(&meta_dir).unwrap();
        std::fs::write(
            meta_dir.join("bdmt_eng.xml"),
            r#"<?xml version="1.0" encoding="utf-8"?>
<disclib xmlns="urn:BDA:bdmv;disclib">
  <di:discinfo xmlns:di="urn:BDA:bdmv;discinfo">
    <di:title>
      <di:name>My Great Movie</di:name>
    </di:title>
  </di:discinfo>
</disclib>"#,
        )
        .unwrap();
        assert_eq!(
            parse_bdmt_title(dir.path()),
            Some("My Great Movie".to_string())
        );
    }

    #[test]
    fn parse_bdmt_title_with_bom() {
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join("BDMV/META/DL");
        std::fs::create_dir_all(&meta_dir).unwrap();
        let mut content = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
        content.extend_from_slice(
            br#"<?xml version="1.0" encoding="utf-8"?>
<disclib xmlns="urn:BDA:bdmv;disclib">
  <di:discinfo xmlns:di="urn:BDA:bdmv;discinfo">
    <di:title>
      <di:name>BOM Test Title</di:name>
    </di:title>
  </di:discinfo>
</disclib>"#,
        );
        std::fs::write(meta_dir.join("bdmt_jpn.xml"), content).unwrap();
        assert_eq!(
            parse_bdmt_title(dir.path()),
            Some("BOM Test Title".to_string())
        );
    }

    #[test]
    fn parse_bdmt_title_prefers_eng() {
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join("BDMV/META/DL");
        std::fs::create_dir_all(&meta_dir).unwrap();
        let make_xml = |name: &str| {
            format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
<disclib xmlns="urn:BDA:bdmv;disclib">
  <di:discinfo xmlns:di="urn:BDA:bdmv;discinfo">
    <di:title>
      <di:name>{}</di:name>
    </di:title>
  </di:discinfo>
</disclib>"#,
                name
            )
        };
        std::fs::write(meta_dir.join("bdmt_jpn.xml"), make_xml("日本語タイトル")).unwrap();
        std::fs::write(meta_dir.join("bdmt_eng.xml"), make_xml("English Title")).unwrap();
        assert_eq!(
            parse_bdmt_title(dir.path()),
            Some("English Title".to_string())
        );
    }

    #[test]
    fn parse_bdmt_title_no_meta_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("BDMV")).unwrap();
        assert_eq!(parse_bdmt_title(dir.path()), None);
    }

    #[test]
    fn parse_bdmt_title_malformed_xml() {
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join("BDMV/META/DL");
        std::fs::create_dir_all(&meta_dir).unwrap();
        std::fs::write(meta_dir.join("bdmt_eng.xml"), "not xml at all").unwrap();
        assert_eq!(parse_bdmt_title(dir.path()), None);
    }

    #[test]
    fn parse_bdmt_title_missing_name_element() {
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join("BDMV/META/DL");
        std::fs::create_dir_all(&meta_dir).unwrap();
        std::fs::write(
            meta_dir.join("bdmt_eng.xml"),
            r#"<?xml version="1.0" encoding="utf-8"?>
<disclib xmlns="urn:BDA:bdmv;disclib">
  <di:discinfo xmlns:di="urn:BDA:bdmv;discinfo">
    <di:title></di:title>
  </di:discinfo>
</disclib>"#,
        )
        .unwrap();
        assert_eq!(parse_bdmt_title(dir.path()), None);
    }

    // Task 3: resolve_input_source() tests
    #[test]
    fn resolve_input_source_folder_with_bdmv() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("BDMV")).unwrap();
        let src = resolve_input_source(dir.path()).unwrap();
        assert!(src.is_folder());
        assert_eq!(src.bluray_path(), dir.path());
    }

    #[test]
    fn resolve_input_source_folder_without_bdmv() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_input_source(dir.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("BDMV"), "error should mention BDMV: {}", msg);
    }

    #[test]
    fn resolve_input_source_block_device_path() {
        let src = resolve_input_source(std::path::Path::new("/dev/sr0")).unwrap();
        assert!(!src.is_folder());
        assert_eq!(src.bluray_path(), std::path::Path::new("/dev/sr0"));
    }
}
