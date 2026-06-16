use std::path::Path;

use ffmpeg_the_third::media::Type as MediaType;

/// How thoroughly to verify output files.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerifyLevel {
    /// Quick structural checks: file exists, streams match, duration within tolerance.
    Quick,
    /// Quick checks plus sample frame decoding at multiple seek points.
    Full,
}

/// Expected properties of a ripped output file, derived from the source playlist.
pub struct VerifyExpected {
    pub duration_secs: u32,
    pub video_streams: u32,
    pub audio_streams: u32,
    pub subtitle_streams: u32,
    pub chapters: usize,
}

/// Aggregate result of all verification checks on a single output file.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub passed: bool,
    #[allow(dead_code)] // Used in Debug formatting; not directly accessed
    pub level: VerifyLevel,
    pub checks: Vec<VerifyCheck>,
}

/// A single pass/fail check with a human-readable detail string.
#[derive(Debug, Clone)]
pub struct VerifyCheck {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

/// Intermediate struct for probe results before comparison against expected values.
struct OutputInfo {
    duration_secs: f64,
    video_streams: u32,
    audio_streams: u32,
    subtitle_streams: u32,
    chapters: u32,
}

/// Verify an output MKV file against expected properties from the source playlist.
///
/// Runs quick structural checks (file existence, stream counts, duration tolerance,
/// chapter count). In `Full` mode, additionally decodes sample video frames at
/// multiple seek points to confirm bitstream validity.
pub fn verify_output(path: &Path, expected: &VerifyExpected, level: VerifyLevel) -> VerifyResult {
    let mut checks = Vec::new();

    // 1. File exists and size > 0
    let exists_ok = path.exists();
    let size_ok = exists_ok
        && std::fs::metadata(path)
            .map(|m| m.len() > 0)
            .unwrap_or(false);

    checks.push(VerifyCheck {
        name: "file_exists",
        passed: exists_ok && size_ok,
        detail: if !exists_ok {
            "file does not exist".to_string()
        } else if !size_ok {
            "file is empty (0 bytes)".to_string()
        } else {
            "ok".to_string()
        },
    });

    if !exists_ok || !size_ok {
        return VerifyResult {
            passed: false,
            level,
            checks,
        };
    }

    // 2. Probe the file with FFmpeg
    let info = match probe_output_file(path) {
        Ok(info) => {
            checks.push(VerifyCheck {
                name: "ffmpeg_open",
                passed: true,
                detail: "ok".to_string(),
            });
            info
        }
        Err(e) => {
            checks.push(VerifyCheck {
                name: "ffmpeg_open",
                passed: false,
                detail: e,
            });
            return VerifyResult {
                passed: false,
                level,
                checks,
            };
        }
    };

    // 3. Duration within 2%
    let (dur_ok, dur_detail) =
        duration_within_tolerance(info.duration_secs, expected.duration_secs);
    checks.push(VerifyCheck {
        name: "duration",
        passed: dur_ok,
        detail: dur_detail,
    });

    // 4-6. Stream counts
    checks.push(stream_count_matches(
        info.video_streams,
        expected.video_streams,
        "video_streams",
    ));
    checks.push(stream_count_matches(
        info.audio_streams,
        expected.audio_streams,
        "audio_streams",
    ));
    checks.push(stream_count_matches(
        info.subtitle_streams,
        expected.subtitle_streams,
        "subtitle_streams",
    ));

    // 7. Chapter count
    let chapters_ok = info.chapters as usize == expected.chapters;
    checks.push(VerifyCheck {
        name: "chapters",
        passed: chapters_ok,
        detail: if chapters_ok {
            format!("{} chapters", expected.chapters)
        } else {
            format!(
                "expected {} chapters, got {}",
                expected.chapters, info.chapters
            )
        },
    });

    // 8. Full mode: decode sample frames
    if level == VerifyLevel::Full {
        let decode_checks = decode_sample_frames(path, info.duration_secs);
        checks.extend(decode_checks);
    }

    let passed = checks.iter().all(|c| c.passed);
    VerifyResult {
        passed,
        level,
        checks,
    }
}

/// Open an output MKV file with FFmpeg and extract structural metadata.
fn probe_output_file(path: &Path) -> Result<OutputInfo, String> {
    crate::media::ensure_init();

    let path_str = path
        .to_str()
        .ok_or_else(|| "path contains invalid UTF-8".to_string())?;
    let ctx =
        ffmpeg_the_third::format::input(path_str).map_err(|e| format!("failed to open: {}", e))?;

    let duration_us = ctx.duration();
    let duration_secs = if duration_us > 0 {
        duration_us as f64 / f64::from(ffmpeg_the_third::ffi::AV_TIME_BASE)
    } else {
        0.0
    };

    let mut video = 0u32;
    let mut audio = 0u32;
    let mut subtitle = 0u32;
    for stream in ctx.streams() {
        match stream.parameters().medium() {
            MediaType::Video => video += 1,
            MediaType::Audio => audio += 1,
            MediaType::Subtitle => subtitle += 1,
            _ => {}
        }
    }

    let chapters = ctx.nb_chapters();

    Ok(OutputInfo {
        duration_secs,
        video_streams: video,
        audio_streams: audio,
        subtitle_streams: subtitle,
        chapters,
    })
}

/// Check whether actual duration is within 2% of expected.
///
/// If expected is 0, the check is skipped (passes unconditionally) since
/// the source didn't provide a duration to compare against.
const DURATION_TOLERANCE: f64 = 0.02;

fn duration_within_tolerance(actual_secs: f64, expected_secs: u32) -> (bool, String) {
    if expected_secs == 0 {
        return (true, "no expected duration (skipped)".to_string());
    }

    let expected = expected_secs as f64;
    let tolerance = expected * DURATION_TOLERANCE;
    let diff = actual_secs - expected;

    if diff.abs() <= tolerance {
        (
            true,
            format!("{:.1}s (expected {:.1}s)", actual_secs, expected),
        )
    } else if diff < 0.0 {
        (
            false,
            format!(
                "{:.1}s is {:.1}s short of expected {:.1}s (>{:.1}% tolerance)",
                actual_secs,
                diff.abs(),
                expected,
                (diff.abs() / expected) * 100.0
            ),
        )
    } else {
        (
            false,
            format!(
                "{:.1}s is {:.1}s over expected {:.1}s (>{:.1}% tolerance)",
                actual_secs,
                diff,
                expected,
                (diff / expected) * 100.0
            ),
        )
    }
}

/// Compare an actual stream count against expected, producing a VerifyCheck.
fn stream_count_matches(actual: u32, expected: u32, stream_type: &'static str) -> VerifyCheck {
    if actual == expected {
        VerifyCheck {
            name: stream_type,
            passed: true,
            detail: format!("{}", actual),
        }
    } else {
        VerifyCheck {
            name: stream_type,
            passed: false,
            detail: format!("expected {}, got {}", expected, actual),
        }
    }
}

/// Decode sample video frames at 5 seek points across the file duration.
///
/// Seeks to 0%, 25%, 50%, 75%, and 90% of the total duration and attempts
/// to decode one video frame at each point. This validates that the bitstream
/// is intact and decodable, catching corruption that structural checks miss.
fn decode_sample_frames(path: &Path, duration_secs: f64) -> Vec<VerifyCheck> {
    let path_str = match path.to_str() {
        Some(s) => s,
        None => {
            return vec![VerifyCheck {
                name: "decode_frames",
                passed: false,
                detail: "path contains invalid UTF-8".to_string(),
            }];
        }
    };

    const SEEK_PERCENTAGES: [f64; 5] = [0.0, 0.25, 0.50, 0.75, 0.90];
    let percentages = SEEK_PERCENTAGES;
    let mut checks = Vec::with_capacity(percentages.len());

    for &pct in &percentages {
        let seek_secs = (duration_secs * pct) as i64;
        let label = format!("{}%", (pct * 100.0) as u32);

        match decode_frame_at(path_str, seek_secs) {
            Ok(()) => {
                checks.push(VerifyCheck {
                    name: "decode_frame",
                    passed: true,
                    detail: format!("decoded frame at {} ({:.0}s)", label, seek_secs),
                });
            }
            Err(e) => {
                checks.push(VerifyCheck {
                    name: "decode_frame",
                    passed: false,
                    detail: format!("failed at {} ({:.0}s): {}", label, seek_secs, e),
                });
            }
        }
    }

    checks
}

/// Seek to a position in the file and decode one video frame.
///
/// Opens the file, finds the first video stream, sets up a decoder,
/// seeks to the target position, and attempts to decode a single frame.
/// Returns Ok(()) if a frame was successfully decoded, or an error string
/// describing what went wrong.
fn decode_frame_at(path: &str, seek_secs: i64) -> Result<(), String> {
    crate::media::ensure_init();

    let mut ictx =
        ffmpeg_the_third::format::input(path).map_err(|e| format!("failed to open: {}", e))?;

    // Find first video stream
    let video_stream_index = ictx
        .streams()
        .find(|s| s.parameters().medium() == MediaType::Video)
        .map(|s| s.index())
        .ok_or_else(|| "no video stream found".to_string())?;

    // Set up decoder from stream parameters
    let stream = ictx
        .stream(video_stream_index)
        .ok_or_else(|| "video stream disappeared".to_string())?;
    let codec_id = stream.parameters().id();
    let decoder_codec = ffmpeg_the_third::codec::decoder::find(codec_id)
        .ok_or_else(|| format!("no decoder for codec {:?}", codec_id))?;

    let mut decoder = ffmpeg_the_third::codec::Context::from_parameters(stream.parameters())
        .map_err(|e| format!("failed to create codec context: {}", e))?
        .decoder()
        .open_as(decoder_codec)
        .map_err(|e| format!("failed to open decoder: {}", e))?;

    // Seek to target position (timestamp in AV_TIME_BASE units = microseconds)
    // Use a wide seek range (0..=target) so FFmpeg can find the nearest keyframe
    let seek_ts = seek_secs * i64::from(ffmpeg_the_third::ffi::AV_TIME_BASE);
    if seek_secs > 0 {
        if let Err(e) = ictx.seek(seek_ts, 0..=seek_ts) {
            // Seek failure on short files is expected — fall through to
            // read from the current position, which still validates the bitstream
            log::debug!(
                "Seek to {}s failed ({}), reading from current position",
                seek_secs,
                e
            );
        }
    }

    // Read packets and try to decode one video frame
    let mut frame = ffmpeg_the_third::frame::Video::empty();
    let mut packet = ffmpeg_the_third::Packet::empty();
    const MAX_DECODE_ATTEMPTS: u32 = 500;
    let mut attempts = 0;

    loop {
        if attempts >= MAX_DECODE_ATTEMPTS {
            return Err("exceeded max packet read attempts without decoding a frame".to_string());
        }
        attempts += 1;

        match packet.read(&mut ictx) {
            Ok(()) => {}
            Err(ffmpeg_the_third::Error::Eof) => {
                // Flush the decoder
                let _ = decoder.send_eof();
                match decoder.receive_frame(&mut frame) {
                    Ok(()) => return Ok(()),
                    Err(_) => return Err("reached EOF without decoding a frame".to_string()),
                }
            }
            Err(e) => return Err(format!("packet read error: {}", e)),
        }

        if packet.stream() != video_stream_index {
            continue;
        }

        if let Err(e) = decoder.send_packet(&packet) {
            // EAGAIN means decoder is full — try receiving a frame first
            if e != (ffmpeg_the_third::Error::Other {
                errno: libc::EAGAIN,
            }) {
                return Err(format!("send_packet error: {}", e));
            }
        }

        match decoder.receive_frame(&mut frame) {
            Ok(()) => return Ok(()),
            Err(ffmpeg_the_third::Error::Other { errno }) if errno == libc::EAGAIN => {
                // Decoder needs more packets
                continue;
            }
            Err(e) => {
                // Some other error — keep trying with more packets
                log::debug!("receive_frame error at attempt {}: {}", attempts, e);
                continue;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_within_tolerance_exact() {
        let (ok, _detail) = duration_within_tolerance(3600.0, 3600);
        assert!(ok);
    }

    #[test]
    fn test_duration_within_tolerance_within_2pct() {
        // 3600 * 0.02 = 72, so 3528 is exactly at the 2% boundary
        let (ok, _detail) = duration_within_tolerance(3528.0, 3600);
        assert!(ok);
    }

    #[test]
    fn test_duration_within_tolerance_over_2pct() {
        // 3500 is 100s short of 3600 (2.78% off), beyond 2% tolerance
        let (ok, detail) = duration_within_tolerance(3500.0, 3600);
        assert!(!ok);
        assert!(
            detail.contains("short"),
            "detail should mention 'short': {}",
            detail
        );
    }

    #[test]
    fn test_duration_within_tolerance_zero_expected() {
        let (ok, _detail) = duration_within_tolerance(100.0, 0);
        assert!(ok);
    }

    #[test]
    fn test_stream_count_matches_pass() {
        let check = stream_count_matches(1, 1, "video_streams");
        assert!(check.passed);
        assert_eq!(check.name, "video_streams");
    }

    #[test]
    fn test_stream_count_matches_fail() {
        let check = stream_count_matches(2, 3, "audio_streams");
        assert!(!check.passed);
        assert_eq!(check.name, "audio_streams");
        assert!(
            check.detail.contains("expected 3"),
            "detail should contain 'expected 3': {}",
            check.detail
        );
    }

    #[test]
    fn test_verify_nonexistent_file() {
        let expected = VerifyExpected {
            duration_secs: 3600,
            video_streams: 1,
            audio_streams: 1,
            subtitle_streams: 0,
            chapters: 0,
        };
        let result = verify_output(
            Path::new("/nonexistent/path/to/file.mkv"),
            &expected,
            VerifyLevel::Quick,
        );
        assert!(!result.passed);
        let file_check = result.checks.iter().find(|c| c.name == "file_exists");
        assert!(file_check.is_some(), "should have a file_exists check");
        assert!(!file_check.unwrap().passed);
    }

    #[test]
    fn test_verify_quick_with_fixture() {
        let fixture = Path::new("tests/fixtures/media/test_video.mkv");
        if !fixture.exists() {
            return; // Skip if fixtures not generated
        }
        let expected = VerifyExpected {
            duration_secs: 0, // Skip duration check — synthetic fixture has unknown duration
            video_streams: 1,
            audio_streams: 1,
            subtitle_streams: 0,
            chapters: 0,
        };
        let result = verify_output(fixture, &expected, VerifyLevel::Quick);
        assert!(
            result
                .checks
                .iter()
                .any(|c| c.name == "file_exists" && c.passed),
            "file_exists check should pass"
        );
        assert!(
            result
                .checks
                .iter()
                .any(|c| c.name == "ffmpeg_open" && c.passed),
            "ffmpeg_open check should pass"
        );
        assert!(
            result
                .checks
                .iter()
                .any(|c| c.name == "video_streams" && c.passed),
            "video_streams check should pass"
        );
        assert!(
            result
                .checks
                .iter()
                .any(|c| c.name == "audio_streams" && c.passed),
            "audio_streams check should pass"
        );
        assert!(result.passed, "all checks should pass: {:?}", result.checks);
    }

    #[test]
    fn test_verify_quick_with_multi_audio_fixture() {
        let fixture = Path::new("tests/fixtures/media/test_multi_audio.mkv");
        if !fixture.exists() {
            return; // Skip if fixtures not generated
        }
        let expected = VerifyExpected {
            duration_secs: 0,
            video_streams: 1,
            audio_streams: 2,
            subtitle_streams: 0,
            chapters: 0,
        };
        let result = verify_output(fixture, &expected, VerifyLevel::Quick);
        assert!(result.passed, "all checks should pass: {:?}", result.checks);
    }

    #[test]
    fn test_verify_stream_count_mismatch_with_fixture() {
        let fixture = Path::new("tests/fixtures/media/test_video.mkv");
        if !fixture.exists() {
            return;
        }
        // Expect wrong stream counts to ensure mismatch is detected
        let expected = VerifyExpected {
            duration_secs: 0,
            video_streams: 2, // wrong — fixture has 1
            audio_streams: 3, // wrong — fixture has 1
            subtitle_streams: 0,
            chapters: 0,
        };
        let result = verify_output(fixture, &expected, VerifyLevel::Quick);
        assert!(!result.passed);
        assert!(
            result
                .checks
                .iter()
                .any(|c| c.name == "video_streams" && !c.passed),
            "video_streams mismatch should be detected"
        );
        assert!(
            result
                .checks
                .iter()
                .any(|c| c.name == "audio_streams" && !c.passed),
            "audio_streams mismatch should be detected"
        );
    }

    #[test]
    fn test_verify_result_summary() {
        let result = VerifyResult {
            passed: false,
            level: VerifyLevel::Quick,
            checks: vec![
                VerifyCheck {
                    name: "file_exists",
                    passed: true,
                    detail: "ok".to_string(),
                },
                VerifyCheck {
                    name: "duration",
                    passed: false,
                    detail: "too short".to_string(),
                },
                VerifyCheck {
                    name: "video_streams",
                    passed: true,
                    detail: "1".to_string(),
                },
            ],
        };

        let failed: Vec<&VerifyCheck> = result.checks.iter().filter(|c| !c.passed).collect();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].name, "duration");
    }

    /// Check if the platform's FFmpeg can decode H.264 from the test fixture.
    /// Fedora's ffmpeg-free may lack a working H.264 decoder.
    fn can_decode_fixture() -> bool {
        let fixture = Path::new("tests/fixtures/media/test_video.mkv");
        if !fixture.exists() {
            return false;
        }
        decode_frame_at(fixture.to_str().unwrap(), 0).is_ok()
    }

    #[test]
    fn test_verify_full_mode_with_fixture() {
        let fixture = Path::new("tests/fixtures/media/test_video.mkv");
        if !fixture.exists() {
            return;
        }
        if !can_decode_fixture() {
            // Platform lacks working H.264 decoder (e.g., Fedora ffmpeg-free)
            return;
        }
        let expected = VerifyExpected {
            duration_secs: 2, // fixture is ~2 seconds
            video_streams: 1,
            audio_streams: 1,
            subtitle_streams: 0,
            chapters: 0,
        };
        let result = verify_output(fixture, &expected, VerifyLevel::Full);
        // Full mode should decode frames at seek points
        let decode_checks: Vec<&VerifyCheck> = result
            .checks
            .iter()
            .filter(|c| c.name == "decode_frame")
            .collect();
        assert!(
            !decode_checks.is_empty(),
            "full mode should have decode_frame checks"
        );
        assert!(
            decode_checks.iter().all(|c| c.passed),
            "all decode_frame checks should pass: {:?}",
            decode_checks
        );
    }

    #[test]
    fn test_verify_duration_match_with_fixture() {
        let fixture = Path::new("tests/fixtures/media/test_video.mkv");
        if !fixture.exists() {
            return;
        }
        // Fixture is ~2 seconds; expect 2s — should be within 2% tolerance
        let expected = VerifyExpected {
            duration_secs: 2,
            video_streams: 1,
            audio_streams: 1,
            subtitle_streams: 0,
            chapters: 0,
        };
        let result = verify_output(fixture, &expected, VerifyLevel::Quick);
        let duration_check = result
            .checks
            .iter()
            .find(|c| c.name == "duration")
            .expect("should have duration check");
        assert!(
            duration_check.passed,
            "duration should match: {}",
            duration_check.detail
        );
    }

    #[test]
    fn test_verify_duration_mismatch_with_fixture() {
        let fixture = Path::new("tests/fixtures/media/test_video.mkv");
        if !fixture.exists() {
            return;
        }
        // Fixture is ~2s but expect 100s — way off, should fail
        let expected = VerifyExpected {
            duration_secs: 100,
            video_streams: 1,
            audio_streams: 1,
            subtitle_streams: 0,
            chapters: 0,
        };
        let result = verify_output(fixture, &expected, VerifyLevel::Quick);
        assert!(!result.passed);
        let duration_check = result
            .checks
            .iter()
            .find(|c| c.name == "duration")
            .expect("should have duration check");
        assert!(!duration_check.passed);
        assert!(duration_check.detail.contains("short"));
    }

    // -- End-to-end pipeline tests matching CLI --verify and --verify-level behavior --

    /// Simulates `--verify` (quick mode, default): exercises the full pipeline
    /// that the CLI/TUI would run after a successful remux. Verifies that the
    /// result matches what `println!("Verified ({:?}): all checks passed", level)`
    /// would print.
    #[test]
    fn test_end_to_end_quick_verify_pipeline() {
        let fixture = Path::new("tests/fixtures/media/test_video.mkv");
        if !fixture.exists() {
            return;
        }
        // Simulate what the CLI builds from Playlist fields after remux
        let expected = VerifyExpected {
            duration_secs: 2,
            video_streams: 1,
            audio_streams: 1,
            subtitle_streams: 0,
            chapters: 0,
        };
        let result = verify_output(fixture, &expected, VerifyLevel::Quick);

        // Verify the result matches CLI success output
        assert!(
            result.passed,
            "quick verify should pass: {:?}",
            result.checks
        );
        assert_eq!(result.level, VerifyLevel::Quick);

        // This is exactly what the CLI would print
        let output = format!("Verified ({:?}): all checks passed", result.level);
        assert_eq!(output, "Verified (Quick): all checks passed");

        // No decode_frame checks in quick mode
        assert!(
            !result.checks.iter().any(|c| c.name == "decode_frame"),
            "quick mode should not decode frames"
        );

        // All 7 quick checks should be present and passing
        let expected_checks = [
            "file_exists",
            "ffmpeg_open",
            "duration",
            "video_streams",
            "audio_streams",
            "subtitle_streams",
            "chapters",
        ];
        for name in &expected_checks {
            let check = result
                .checks
                .iter()
                .find(|c| c.name == *name)
                .unwrap_or_else(|| panic!("missing check: {}", name));
            assert!(
                check.passed,
                "check '{}' should pass: {}",
                name, check.detail
            );
        }
    }

    /// Simulates `--verify --verify-level full`: exercises the full pipeline
    /// including frame decode at seek points. Verifies decode_frame checks
    /// are present and pass on a real MKV file.
    #[test]
    fn test_end_to_end_full_verify_pipeline() {
        let fixture = Path::new("tests/fixtures/media/test_video.mkv");
        if !fixture.exists() {
            return;
        }
        if !can_decode_fixture() {
            // Platform lacks working H.264 decoder (e.g., Fedora ffmpeg-free)
            return;
        }
        let expected = VerifyExpected {
            duration_secs: 2,
            video_streams: 1,
            audio_streams: 1,
            subtitle_streams: 0,
            chapters: 0,
        };
        let result = verify_output(fixture, &expected, VerifyLevel::Full);

        // Verify the result matches CLI success output
        assert!(
            result.passed,
            "full verify should pass: {:?}",
            result.checks
        );
        assert_eq!(result.level, VerifyLevel::Full);

        let output = format!("Verified ({:?}): all checks passed", result.level);
        assert_eq!(output, "Verified (Full): all checks passed");

        // Full mode includes all quick checks plus decode_frame checks
        let decode_checks: Vec<&VerifyCheck> = result
            .checks
            .iter()
            .filter(|c| c.name == "decode_frame")
            .collect();
        assert!(
            decode_checks.len() >= 2,
            "full mode should have multiple decode_frame checks, got {}",
            decode_checks.len()
        );
        assert!(
            decode_checks.iter().all(|c| c.passed),
            "all frame decodes should pass: {:?}",
            decode_checks
        );
    }

    /// Simulates `--verify` with a verification failure: exercises the failure
    /// path and verifies the warning output format matches the CLI.
    #[test]
    fn test_end_to_end_verify_failure_output() {
        let fixture = Path::new("tests/fixtures/media/test_video.mkv");
        if !fixture.exists() {
            return;
        }
        // Wrong stream counts to force failure
        let expected = VerifyExpected {
            duration_secs: 2,
            video_streams: 3, // wrong
            audio_streams: 1,
            subtitle_streams: 0,
            chapters: 0,
        };
        let result = verify_output(fixture, &expected, VerifyLevel::Quick);

        assert!(!result.passed);

        // Simulate the CLI warning format
        let failed: Vec<&str> = result
            .checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.detail.as_str())
            .collect();
        let warning = format!("WARNING: verification failed: {}", failed.join("; "));
        assert!(
            warning.contains("WARNING: verification failed:"),
            "should format as warning"
        );
        assert!(
            warning.contains("expected 3, got 1"),
            "should describe the stream count mismatch: {}",
            warning
        );

        // The hook variable format uses check names (not details)
        let hook_detail: Vec<&str> = result
            .checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.name)
            .collect();
        assert!(
            hook_detail.contains(&"video_streams"),
            "hook detail should include video_streams: {:?}",
            hook_detail
        );
    }
}
