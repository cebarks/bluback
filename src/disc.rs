use anyhow::{bail, Result};
use regex::Regex;
use std::process::Command;
use std::sync::LazyLock;

use crate::types::{LabelInfo, Playlist, StreamInfo};
use crate::util::duration_to_seconds;

static LABEL_PATTERNS: LazyLock<[Regex; 2]> = LazyLock::new(|| [
    Regex::new(r"(?i)^(?P<show>.+?)_?SEASON(?P<season>\d+)_?DISC(?P<disc>\d+)").unwrap(),
    Regex::new(r"(?i)^(?P<show>.+?)_S(?P<season>\d+)_?D(?P<disc>\d+)").unwrap(),
]);

static PLAYLIST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"playlist (\d+)\.mpls \((\d+:\d+:\d+)\)").unwrap());

pub fn check_dependencies() -> Result<()> {
    let mut missing = Vec::new();
    for cmd in &["ffmpeg", "ffprobe"] {
        if which::which(cmd).is_err() {
            missing.push(*cmd);
        }
    }
    if !missing.is_empty() {
        bail!(
            "Required commands not found: {}. Install ffmpeg with libbluray support.",
            missing.join(", ")
        );
    }
    Ok(())
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
            let season: u32 = caps["season"].parse().unwrap();
            let disc: u32 = caps["disc"].parse().unwrap();
            return Some(LabelInfo { show, season, disc });
        }
    }
    None
}

pub fn scan_playlists(device: &str) -> Result<Vec<Playlist>> {
    let output = Command::new("ffprobe")
        .args(["-i", &format!("bluray:{}", device)])
        .output()?;

    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);

    let mut playlists = Vec::new();
    for caps in PLAYLIST_RE.captures_iter(&text) {
        let num = caps[1].to_string();
        let duration = caps[2].to_string();
        let seconds = duration_to_seconds(&duration);
        playlists.push(Playlist { num, duration, seconds });
    }
    Ok(playlists)
}

pub fn filter_episodes(playlists: &[Playlist], min_duration: u32) -> Vec<&Playlist> {
    playlists.iter().filter(|pl| pl.seconds >= min_duration).collect()
}

pub fn probe_streams(device: &str, playlist_num: &str) -> Option<StreamInfo> {
    let output = Command::new("ffprobe")
        .args(["-playlist", playlist_num, "-i", &format!("bluray:{}", device)])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);

    let mut audio_streams = Vec::new();
    let mut sub_count = 0u32;
    for line in text.lines() {
        if line.contains("Stream") && line.contains("Audio") {
            audio_streams.push(line.to_string());
        }
        if line.contains("Stream") && line.contains("Subtitle") {
            sub_count += 1;
        }
    }
    Some(StreamInfo { audio_streams, sub_count })
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
            Playlist { num: "00001".into(), duration: "0:00:30".into(), seconds: 30 },
            Playlist { num: "00002".into(), duration: "0:43:00".into(), seconds: 2580 },
            Playlist { num: "00003".into(), duration: "0:44:00".into(), seconds: 2640 },
            Playlist { num: "00004".into(), duration: "0:02:00".into(), seconds: 120 },
        ];
        let result = filter_episodes(&playlists, 900);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].num, "00002");
        assert_eq!(result[1].num, "00003");
    }
}
