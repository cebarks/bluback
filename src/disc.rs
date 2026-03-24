use anyhow::{bail, Result};
use regex::Regex;
use std::process::Command;
use std::sync::LazyLock;

use crate::types::{LabelInfo, MediaInfo, Playlist};

static LABEL_PATTERNS: LazyLock<[Regex; 2]> = LazyLock::new(|| {
    [
        Regex::new(r"(?i)^(?P<show>.+?)_?SEASON(?P<season>\d+)_?DISC(?P<disc>\d+)").expect("valid regex"),
        Regex::new(r"(?i)^(?P<show>.+?)_S(?P<season>\d+)_?D(?P<disc>\d+)").expect("valid regex"),
    ]
});

/// Return all optical drives (type "rom") found via lsblk.
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

    drives
}

/// Get the mount point of a device if it's already mounted.
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

/// Mount a disc using udisksctl. Returns the mount point on success.
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
    stdout
        .split(" at ")
        .nth(1)
        .map(|s| s.trim().trim_end_matches('.').to_string())
        .ok_or_else(|| anyhow::anyhow!("Could not parse mount point from udisksctl output"))
}

/// Unmount a disc using udisksctl.
pub fn unmount_disc(device: &str) -> Result<()> {
    let output = Command::new("udisksctl")
        .args(["unmount", "-b", device])
        .output()?;

    if !output.status.success() {
        bail!("Failed to unmount {}", device);
    }
    Ok(())
}

/// Ensure the disc is mounted, returning (mount_point, did_we_mount_it).
/// If it was already mounted, returns the existing mount point.
/// If we mounted it, the caller should unmount when done.
pub fn ensure_mounted(device: &str) -> Result<(String, bool)> {
    if let Some(mount) = get_mount_point(device) {
        Ok((mount, false))
    } else {
        let mount = mount_disc(device)?;
        Ok((mount, true))
    }
}

pub fn get_volume_label(device: &str) -> String {
    Command::new("lsblk")
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
        .unwrap_or_default()
}

pub fn parse_volume_label(label: &str) -> Option<LabelInfo> {
    if label.is_empty() {
        return None;
    }
    for re in LABEL_PATTERNS.iter() {
        if let Some(caps) = re.captures(label) {
            let show = caps["show"].trim_matches('_').replace('_', " ");
            let season: u32 = caps["season"].parse().expect("regex guarantees numeric capture");
            let disc: u32 = caps["disc"].parse().expect("regex guarantees numeric capture");
            return Some(LabelInfo { show, season, disc });
        }
    }
    None
}

pub fn scan_playlists(device: &str) -> Result<Vec<Playlist>> {
    crate::media::scan_playlists(device).map_err(|e| anyhow::anyhow!("{}", e))
}

pub fn filter_episodes(playlists: &[Playlist], min_duration: u32) -> Vec<&Playlist> {
    playlists
        .iter()
        .filter(|pl| pl.seconds >= min_duration)
        .collect()
}

pub fn probe_media_info(device: &str, playlist_num: &str) -> Option<MediaInfo> {
    crate::media::probe_media_info(device, playlist_num).ok()
}

pub fn set_max_speed(device: &str) {
    let _ = Command::new("eject").args(["-x", "0", device]).status();
}

pub fn eject_disc(device: &str) -> anyhow::Result<()> {
    let status = Command::new("eject").arg(device).status()?;

    if !status.success() {
        bail!("eject exited with code {}", status.code().unwrap_or(-1));
    }
    Ok(())
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
    fn test_filter_episodes() {
        let playlists = vec![
            Playlist {
                num: "00001".into(),
                duration: "0:00:30".into(),
                seconds: 30,
            },
            Playlist {
                num: "00002".into(),
                duration: "0:43:00".into(),
                seconds: 2580,
            },
            Playlist {
                num: "00003".into(),
                duration: "0:44:00".into(),
                seconds: 2640,
            },
            Playlist {
                num: "00004".into(),
                duration: "0:02:00".into(),
                seconds: 120,
            },
        ];
        let result = filter_episodes(&playlists, 900);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].num, "00002");
        assert_eq!(result[1].num, "00003");
    }

}
