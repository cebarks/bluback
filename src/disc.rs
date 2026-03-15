use anyhow::{bail, Result};
use regex::Regex;
use std::process::Command;
use std::sync::LazyLock;

use crate::types::{LabelInfo, MediaInfo, Playlist, StreamInfo};
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

pub fn parse_media_info_json(json: &serde_json::Value) -> Option<MediaInfo> {
    let streams = json.get("streams")?.as_array()?;

    let video = streams.iter().find(|s| {
        s.get("codec_type").and_then(|v| v.as_str()) == Some("video")
    })?;

    let audio = streams.iter().find(|s| {
        s.get("codec_type").and_then(|v| v.as_str()) == Some("audio")
    });

    let width = video.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let height = video.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let resolution = if height > 0 { format!("{}p", height) } else { String::new() };

    let codec = video.get("codec_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let aspect_ratio = video.get("display_aspect_ratio").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let bit_depth = video.get("bits_per_raw_sample").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let profile_str = video.get("profile").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let framerate = video.get("r_frame_rate")
        .and_then(|v| v.as_str())
        .map(|fr| {
            if let Some((num, den)) = fr.split_once('/') {
                let n: f64 = num.parse().unwrap_or(0.0);
                let d: f64 = den.parse().unwrap_or(1.0);
                if d > 0.0 { format!("{:.3}", n / d) } else { fr.to_string() }
            } else {
                fr.to_string()
            }
        })
        .unwrap_or_default();

    // HDR detection
    let color_transfer = video.get("color_transfer").and_then(|v| v.as_str()).unwrap_or("");
    let side_data = video.get("side_data_list").and_then(|v| v.as_array());

    let has_dovi = side_data.map(|sd| {
        sd.iter().any(|entry| {
            entry.get("side_data_type").and_then(|v| v.as_str()) == Some("DOVI configuration record")
        })
    }).unwrap_or(false);

    let has_hdr10plus = side_data.map(|sd| {
        sd.iter().any(|entry| {
            entry.get("side_data_type").and_then(|v| v.as_str()) == Some("HDR Dynamic Metadata SMPTE2094-40")
        })
    }).unwrap_or(false);

    let hdr = if color_transfer == "smpte2084" {
        if has_dovi { "DV".to_string() }
        else if has_hdr10plus { "HDR10+".to_string() }
        else { "HDR10".to_string() }
    } else if color_transfer == "arib-std-b67" {
        "HLG".to_string()
    } else {
        "SDR".to_string()
    };

    // Audio info
    let (audio_codec, audio_channels, audio_lang) = if let Some(a) = audio {
        let codec_name = a.get("codec_name").and_then(|v| v.as_str()).unwrap_or("");
        let audio_profile = a.get("profile").and_then(|v| v.as_str()).unwrap_or("");

        let audio_str = if !audio_profile.is_empty() && codec_name == "dts" {
            audio_profile.to_lowercase()
        } else {
            codec_name.to_string()
        };

        let channels = a.get("channel_layout").and_then(|v| v.as_str()).unwrap_or("");
        let channel_count = a.get("channels").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let ch_str = if !channels.is_empty() {
            if channels.starts_with("stereo") {
                "2.0".to_string()
            } else if channels.starts_with("mono") {
                "1.0".to_string()
            } else {
                channels.split('(').next().unwrap_or(channels).to_string()
            }
        } else {
            match channel_count {
                1 => "1.0".to_string(),
                2 => "2.0".to_string(),
                6 => "5.1".to_string(),
                8 => "7.1".to_string(),
                n if n > 0 => format!("{}", n),
                _ => String::new(),
            }
        };

        let lang = a.get("tags")
            .and_then(|t| t.get("language"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        (audio_str, ch_str, lang)
    } else {
        (String::new(), String::new(), String::new())
    };

    Some(MediaInfo {
        resolution, width, height, codec, hdr, aspect_ratio,
        framerate, bit_depth, profile: profile_str,
        audio: audio_codec, channels: audio_channels, audio_lang,
    })
}

pub fn probe_media_info(device: &str, playlist_num: &str) -> Option<MediaInfo> {
    let output = Command::new("ffprobe")
        .args([
            "-playlist", playlist_num,
            "-print_format", "json",
            "-show_streams",
            "-loglevel", "quiet",
            "-i", &format!("bluray:{}", device),
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    parse_media_info_json(&json)
}

pub fn set_max_speed(device: &str) {
    let _ = Command::new("eject")
        .args(["-x", "0", device])
        .status();
}

pub fn eject_disc(device: &str) -> anyhow::Result<()> {
    let status = Command::new("eject")
        .arg(device)
        .status()?;

    if !status.success() {
        bail!(
            "eject exited with code {}",
            status.code().unwrap_or(-1)
        );
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

    #[test]
    fn test_parse_media_info_1080p_hevc_truehd() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "hevc",
                    "width": 1920,
                    "height": 1080,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "24000/1001",
                    "bits_per_raw_sample": "10",
                    "profile": "Main 10",
                    "color_transfer": "smpte2084",
                    "side_data_list": []
                },
                {
                    "codec_type": "audio",
                    "codec_name": "truehd",
                    "channel_layout": "7.1",
                    "channels": 8,
                    "tags": { "language": "eng" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.resolution, "1080p");
        assert_eq!(info.width, 1920);
        assert_eq!(info.height, 1080);
        assert_eq!(info.codec, "hevc");
        assert_eq!(info.hdr, "HDR10");
        assert_eq!(info.aspect_ratio, "16:9");
        assert_eq!(info.framerate, "23.976");
        assert_eq!(info.bit_depth, "10");
        assert_eq!(info.profile, "Main 10");
        assert_eq!(info.audio, "truehd");
        assert_eq!(info.channels, "7.1");
        assert_eq!(info.audio_lang, "eng");
    }

    #[test]
    fn test_parse_media_info_sdr() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": 1920, "height": 1080,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "24/1",
                    "bits_per_raw_sample": "8",
                    "profile": "High"
                },
                {
                    "codec_type": "audio",
                    "codec_name": "ac3",
                    "channel_layout": "5.1(side)",
                    "channels": 6,
                    "tags": { "language": "eng" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.codec, "h264");
        assert_eq!(info.hdr, "SDR");
        assert_eq!(info.channels, "5.1");
        assert_eq!(info.framerate, "24.000");
    }

    #[test]
    fn test_parse_media_info_dolby_vision() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "hevc",
                    "width": 3840, "height": 2160,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "24000/1001",
                    "bits_per_raw_sample": "10",
                    "profile": "Main 10",
                    "color_transfer": "smpte2084",
                    "side_data_list": [
                        { "side_data_type": "DOVI configuration record" }
                    ]
                },
                {
                    "codec_type": "audio", "codec_name": "truehd",
                    "channel_layout": "7.1", "channels": 8,
                    "tags": { "language": "eng" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.resolution, "2160p");
        assert_eq!(info.hdr, "DV");
    }

    #[test]
    fn test_parse_media_info_hlg() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "hevc",
                    "width": 3840, "height": 2160,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "50/1",
                    "bits_per_raw_sample": "10",
                    "profile": "Main 10",
                    "color_transfer": "arib-std-b67"
                },
                {
                    "codec_type": "audio", "codec_name": "aac",
                    "channel_layout": "stereo", "channels": 2,
                    "tags": { "language": "jpn" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.hdr, "HLG");
        assert_eq!(info.channels, "2.0");
        assert_eq!(info.audio_lang, "jpn");
    }

    #[test]
    fn test_parse_media_info_hdr10plus() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "hevc",
                    "width": 3840, "height": 2160,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "24000/1001",
                    "bits_per_raw_sample": "10",
                    "profile": "Main 10",
                    "color_transfer": "smpte2084",
                    "side_data_list": [
                        { "side_data_type": "HDR Dynamic Metadata SMPTE2094-40" }
                    ]
                },
                {
                    "codec_type": "audio", "codec_name": "eac3",
                    "channel_layout": "5.1(side)", "channels": 6,
                    "tags": { "language": "eng" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.hdr, "HDR10+");
    }

    #[test]
    fn test_parse_media_info_no_streams() {
        let json = serde_json::json!({ "streams": [] });
        assert!(parse_media_info_json(&json).is_none());
    }

    #[test]
    fn test_parse_media_info_dts_hd_ma() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video", "codec_name": "h264",
                    "width": 1920, "height": 1080,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "24/1",
                    "bits_per_raw_sample": "8",
                    "profile": "High"
                },
                {
                    "codec_type": "audio", "codec_name": "dts",
                    "profile": "DTS-HD MA",
                    "channel_layout": "5.1(side)", "channels": 6,
                    "tags": { "language": "eng" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.audio, "dts-hd ma");
    }
}
