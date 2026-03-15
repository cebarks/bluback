use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::LazyLock;

use crate::types::{RipProgress, StreamInfo};
use crate::util::duration_to_seconds;

static SURROUND_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"5\.1|7\.1|surround").unwrap());

pub fn build_map_args(streams: &StreamInfo) -> Vec<String> {
    let mut args = vec!["-map".into(), "0:v:0".into()];

    let mut surround_idx: Option<usize> = None;
    let mut stereo_idx: Option<usize> = None;

    for (i, line) in streams.audio_streams.iter().enumerate() {
        if SURROUND_RE.is_match(line) && surround_idx.is_none() {
            surround_idx = Some(i);
        }
        if line.contains("stereo") && stereo_idx.is_none() {
            stereo_idx = Some(i);
        }
    }

    if let Some(si) = surround_idx {
        args.extend(["-map".into(), format!("0:a:{}", si)]);
        if let Some(sti) = stereo_idx {
            args.extend(["-map".into(), format!("0:a:{}", sti)]);
        }
    } else if !streams.audio_streams.is_empty() {
        args.extend(["-map".into(), "0:a:0".into()]);
    }

    if streams.sub_count > 0 {
        args.extend(["-map".into(), "0:s?".into()]);
    }

    args
}

pub fn start_rip(
    device: &str,
    playlist_num: &str,
    map_args: &[String],
    outfile: &Path,
) -> anyhow::Result<Child> {
    let child = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-nostats",
            "-progress",
            "pipe:1",
        ])
        .args([
            "-playlist",
            playlist_num,
            "-i",
            &format!("bluray:{}", device),
        ])
        .args(map_args)
        .args(["-c", "copy"])
        .arg(outfile)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    Ok(child)
}

pub fn parse_progress_line(line: &str, state: &mut HashMap<String, String>) -> Option<RipProgress> {
    let line = line.trim();
    if let Some((key, val)) = line.split_once('=') {
        state.insert(key.to_string(), val.to_string());
    }

    if !line.starts_with("progress=") {
        return None;
    }

    let frame = state.get("frame").and_then(|v| v.parse().ok()).unwrap_or(0);
    let fps = state.get("fps").and_then(|v| v.parse().ok()).unwrap_or(0.0);

    let total_size = state
        .get("total_size")
        .and_then(|v| v.parse::<i64>().ok())
        .map(|v| v.max(0) as u64)
        .unwrap_or(0);

    let out_time_secs = state
        .get("out_time")
        .map(|v| {
            let truncated = v.split('.').next().unwrap_or("00:00:00");
            duration_to_seconds(truncated)
        })
        .unwrap_or(0);

    let bitrate = state.get("bitrate").cloned().unwrap_or_else(|| "0".into());

    let speed = state
        .get("speed")
        .and_then(|v| v.trim_end_matches('x').parse().ok())
        .unwrap_or(0.0);

    Some(RipProgress {
        frame,
        fps,
        total_size,
        out_time_secs,
        bitrate,
        speed,
    })
}

pub fn estimate_final_size(progress: &RipProgress, total_seconds: u32) -> Option<u64> {
    if progress.out_time_secs > 0 && total_seconds > 0 {
        Some(progress.total_size / progress.out_time_secs as u64 * total_seconds as u64)
    } else {
        None
    }
}

pub fn estimate_eta(progress: &RipProgress, total_seconds: u32) -> Option<u32> {
    if progress.speed > 0.0 && total_seconds > 0 && progress.out_time_secs < total_seconds {
        let remaining_content = total_seconds - progress.out_time_secs;
        Some((remaining_content as f64 / progress.speed) as u32)
    } else {
        None
    }
}

pub fn format_eta(seconds: u32) -> String {
    let hrs = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hrs > 0 {
        format!("{}:{:02}:{:02}", hrs, mins, secs)
    } else {
        format!("{}:{:02}", mins, secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_map_args_surround_and_stereo() {
        let streams = StreamInfo {
            audio_streams: vec![
                "Stream #0:1: Audio: pcm, 48000 Hz, 5.1, s16".into(),
                "Stream #0:2: Audio: pcm, 48000 Hz, stereo, s16".into(),
            ],
            sub_count: 2,
        };
        let args = build_map_args(&streams);
        assert_eq!(
            args,
            vec!["-map", "0:v:0", "-map", "0:a:0", "-map", "0:a:1", "-map", "0:s?",]
        );
    }

    #[test]
    fn test_build_map_args_no_surround() {
        let streams = StreamInfo {
            audio_streams: vec!["Stream #0:1: Audio: pcm, 48000 Hz, stereo, s16".into()],
            sub_count: 0,
        };
        let args = build_map_args(&streams);
        assert_eq!(args, vec!["-map", "0:v:0", "-map", "0:a:0"]);
    }

    #[test]
    fn test_build_map_args_no_audio() {
        let streams = StreamInfo {
            audio_streams: vec![],
            sub_count: 1,
        };
        let args = build_map_args(&streams);
        assert_eq!(args, vec!["-map", "0:v:0", "-map", "0:s?"]);
    }

    #[test]
    fn test_parse_progress_line_accumulates() {
        let mut state = HashMap::new();
        assert!(parse_progress_line("frame=100", &mut state).is_none());
        assert!(parse_progress_line("fps=24.0", &mut state).is_none());
        assert!(parse_progress_line("total_size=1048576", &mut state).is_none());
        assert!(parse_progress_line("out_time=00:01:30.000000", &mut state).is_none());
        assert!(parse_progress_line("bitrate=1234.5kbits/s", &mut state).is_none());
        assert!(parse_progress_line("speed=2.5x", &mut state).is_none());

        let progress = parse_progress_line("progress=continue", &mut state).unwrap();
        assert_eq!(progress.frame, 100);
        assert!((progress.fps - 24.0).abs() < 0.01);
        assert_eq!(progress.total_size, 1048576);
        assert_eq!(progress.out_time_secs, 90);
        assert_eq!(progress.bitrate, "1234.5kbits/s");
        assert!((progress.speed - 2.5).abs() < 0.01);
    }

    #[test]
    fn test_parse_progress_negative_size() {
        let mut state = HashMap::new();
        parse_progress_line("total_size=-1", &mut state);
        let progress = parse_progress_line("progress=continue", &mut state).unwrap();
        assert_eq!(progress.total_size, 0);
    }

    #[test]
    fn test_estimate_final_size() {
        let progress = RipProgress {
            total_size: 1_000_000,
            out_time_secs: 100,
            ..Default::default()
        };
        assert_eq!(estimate_final_size(&progress, 2600), Some(26_000_000));
    }

    #[test]
    fn test_estimate_eta() {
        let progress = RipProgress {
            out_time_secs: 1000,
            speed: 2.0,
            ..Default::default()
        };
        assert_eq!(estimate_eta(&progress, 2600), Some(800));
    }

    #[test]
    fn test_format_eta_with_hours() {
        assert_eq!(format_eta(3661), "1:01:01");
    }

    #[test]
    fn test_format_eta_minutes_only() {
        assert_eq!(format_eta(125), "2:05");
    }
}
