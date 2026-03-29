# Rip Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Post-remux verification that probes output MKV files, comparing duration/streams/chapters against source expectations. Two levels: quick (header probe) and full (+ sample frame decode).

**Architecture:** New `src/verify.rs` module with pure verification logic. Stream counts added to `Playlist` during scan. Config/CLI flags for opt-in. TUI dashboard prompts on failure; CLI logs warnings. Verification runs synchronously in the remux worker thread after remux completes.

**Tech Stack:** Rust, ffmpeg-the-third (avformat/avcodec APIs), existing config/types infrastructure.

---

### Task 1: Add stream counts to Playlist

**Files:**
- Modify: `src/types.rs:6-10`
- Modify: `src/media/probe.rs:355-365`

- [ ] **Step 1: Write failing tests for Playlist stream counts**

In `src/media/probe.rs`, update the `test_parse_playlist_log_line_valid` test and add a new test that expects stream counts on Playlist:

```rust
// In src/types.rs tests module, add:
#[test]
fn test_playlist_default_stream_counts() {
    let pl = Playlist {
        num: "00001".into(),
        duration: "1:00:00".into(),
        seconds: 3600,
        video_streams: 0,
        audio_streams: 0,
        subtitle_streams: 0,
    };
    assert_eq!(pl.video_streams, 0);
    assert_eq!(pl.audio_streams, 0);
    assert_eq!(pl.subtitle_streams, 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_playlist_default_stream_counts`
Expected: FAIL — `Playlist` doesn't have stream count fields yet.

- [ ] **Step 3: Add stream count fields to Playlist**

In `src/types.rs`, update the `Playlist` struct:

```rust
#[derive(Debug, Clone)]
pub struct Playlist {
    pub num: String,
    pub duration: String,
    pub seconds: u32,
    pub video_streams: u32,
    pub audio_streams: u32,
    pub subtitle_streams: u32,
}
```

- [ ] **Step 4: Fix all compilation errors from Playlist field addition**

Every place that constructs a `Playlist` needs the new fields. Key locations:

In `src/media/probe.rs` `parse_playlist_log_line`:
```rust
Some(Playlist {
    num,
    duration,
    seconds,
    video_streams: 0,
    audio_streams: 0,
    subtitle_streams: 0,
})
```

Fix all other `Playlist { .. }` constructions (tests in `src/disc.rs`, `src/session.rs`, `src/types.rs`, etc.) by adding the three new fields with value `0`.

- [ ] **Step 5: Run tests to verify compilation and tests pass**

Run: `cargo test`
Expected: All tests pass (stream counts are 0 everywhere for now).

- [ ] **Step 6: Populate stream counts during scan**

In `src/media/probe.rs`, after `scan_playlists_with_progress` builds the playlist list from log lines, add a second pass that probes each playlist's streams. Add a helper function:

```rust
/// Count streams by type for a single playlist.
/// Returns (video, audio, subtitle) counts. Returns (0, 0, 0) on error.
fn count_streams(device: &str, playlist_num: &str) -> (u32, u32, u32) {
    let ctx = match open_bluray(device, Some(playlist_num)) {
        Ok(ctx) => ctx,
        Err(_) => return (0, 0, 0),
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
    (video, audio, subtitle)
}
```

Then in `scan_playlists_with_progress`, after building the `playlists` Vec, iterate and fill in counts:

```rust
// Probe stream counts for each discovered playlist
for pl in &mut playlists {
    let (v, a, s) = count_streams(device, &pl.num);
    pl.video_streams = v;
    pl.audio_streams = a;
    pl.subtitle_streams = s;
}
```

**Note:** On Linux, the scan runs in a forked child process and playlists are built from pipe output in the parent. Stream counting must happen in the parent process after parsing the child's output (not inside the fork). The `count_streams` calls use `open_bluray` which runs FFmpeg in the current process — this is safe because the fork was only to isolate the initial libbluray D-state risk during the first `open_bluray(device, None)` call. Subsequent per-playlist opens don't have the D-state issue since AACS auth is already done.

- [ ] **Step 7: Run tests and verify**

Run: `cargo test`
Expected: All existing tests pass. Stream counts remain 0 in unit tests (no real disc), but the code compiles and runs.

- [ ] **Step 8: Commit**

Suggest commit: `feat: add stream counts to Playlist struct, populated during scan`

---

### Task 2: Add verify types and module skeleton

**Files:**
- Create: `src/verify.rs`
- Modify: `src/main.rs:1-17` (add `mod verify`)

- [ ] **Step 1: Write tests for VerifyExpected and VerifyResult**

Create `src/verify.rs` with types and tests:

```rust
use std::path::Path;

/// Verification intensity level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerifyLevel {
    Quick,
    Full,
}

/// Source expectations to compare against the output file.
pub struct VerifyExpected {
    pub duration_secs: u32,
    pub video_streams: u32,
    pub audio_streams: u32,
    pub subtitle_streams: u32,
    pub chapters: usize,
}

/// Result of verifying a single output file.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub passed: bool,
    pub level: VerifyLevel,
    pub checks: Vec<VerifyCheck>,
}

/// A single verification check (pass or fail with detail).
#[derive(Debug, Clone)]
pub struct VerifyCheck {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

const TOLERANCE_PCT: f64 = 2.0;

/// Check if a duration is within tolerance of the expected value.
fn duration_within_tolerance(actual_secs: f64, expected_secs: u32) -> (bool, String) {
    if expected_secs == 0 {
        return (true, "no expected duration".into());
    }
    let expected = expected_secs as f64;
    let diff_pct = ((actual_secs - expected) / expected * 100.0).abs();
    let passed = diff_pct <= TOLERANCE_PCT;
    let detail = format!(
        "expected {}s +/-{:.0}%, got {:.0}s ({:.1}% {})",
        expected_secs,
        TOLERANCE_PCT,
        actual_secs,
        diff_pct,
        if actual_secs < expected { "short" } else { "over" }
    );
    (passed, detail)
}

/// Check if a stream count matches the expected value.
fn stream_count_matches(actual: u32, expected: u32, stream_type: &str) -> VerifyCheck {
    let passed = actual == expected;
    let detail = if passed {
        format!("{} {} stream(s)", actual, stream_type)
    } else {
        format!("expected {} {} stream(s), got {}", expected, stream_type, actual)
    };
    VerifyCheck {
        name: match stream_type {
            "video" => "video_streams",
            "audio" => "audio_streams",
            "subtitle" => "subtitle_streams",
            _ => "streams",
        },
        passed,
        detail,
    }
}

/// Top-level verification entry point.
pub fn verify_output(
    path: &Path,
    expected: &VerifyExpected,
    level: VerifyLevel,
) -> VerifyResult {
    let mut checks = Vec::new();

    // Check 1: File exists and is non-zero
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > 0 => {
            checks.push(VerifyCheck {
                name: "file_exists",
                passed: true,
                detail: format!("{} bytes", meta.len()),
            });
        }
        Ok(_) => {
            checks.push(VerifyCheck {
                name: "file_exists",
                passed: false,
                detail: "file is empty (0 bytes)".into(),
            });
            return VerifyResult { passed: false, level, checks };
        }
        Err(e) => {
            checks.push(VerifyCheck {
                name: "file_exists",
                passed: false,
                detail: format!("cannot stat file: {}", e),
            });
            return VerifyResult { passed: false, level, checks };
        }
    }

    // Check 2-7: Probe output file
    match probe_output_file(path) {
        Ok(info) => {
            // Duration
            let (dur_ok, dur_detail) = duration_within_tolerance(info.duration_secs, expected.duration_secs);
            checks.push(VerifyCheck { name: "duration", passed: dur_ok, detail: dur_detail });

            // Stream counts
            checks.push(stream_count_matches(info.video_streams, expected.video_streams, "video"));
            checks.push(stream_count_matches(info.audio_streams, expected.audio_streams, "audio"));
            checks.push(stream_count_matches(info.subtitle_streams, expected.subtitle_streams, "subtitle"));

            // Chapters
            let ch_ok = info.chapters == expected.chapters;
            checks.push(VerifyCheck {
                name: "chapters",
                passed: ch_ok,
                detail: if ch_ok {
                    format!("{} chapter(s)", info.chapters)
                } else {
                    format!("expected {} chapter(s), got {}", expected.chapters, info.chapters)
                },
            });

            // Full mode: decode sample frames
            if level == VerifyLevel::Full {
                let decode_checks = decode_sample_frames(path, info.duration_secs);
                checks.extend(decode_checks);
            }
        }
        Err(e) => {
            checks.push(VerifyCheck {
                name: "probe",
                passed: false,
                detail: format!("failed to probe output: {}", e),
            });
        }
    }

    let passed = checks.iter().all(|c| c.passed);
    VerifyResult { passed, level, checks }
}

/// Info extracted from probing the output MKV file.
struct OutputInfo {
    duration_secs: f64,
    video_streams: u32,
    audio_streams: u32,
    subtitle_streams: u32,
    chapters: usize,
}

/// Probe an output MKV file for duration, stream counts, and chapter count.
fn probe_output_file(path: &Path) -> Result<OutputInfo, String> {
    crate::media::ensure_init();

    let path_str = path.to_str().ok_or("invalid path")?;
    let ctx = ffmpeg_the_third::format::input(path_str)
        .map_err(|e| format!("cannot open: {}", e))?;

    let duration_secs = {
        let dur = ctx.duration();
        if dur > 0 {
            dur as f64 / f64::from(ffmpeg_the_third::ffi::AV_TIME_BASE)
        } else {
            0.0
        }
    };

    let mut video = 0u32;
    let mut audio = 0u32;
    let mut subtitle = 0u32;
    for stream in ctx.streams() {
        match stream.parameters().medium() {
            ffmpeg_the_third::media::Type::Video => video += 1,
            ffmpeg_the_third::media::Type::Audio => audio += 1,
            ffmpeg_the_third::media::Type::Subtitle => subtitle += 1,
            _ => {}
        }
    }

    let chapters = ctx.nb_chapters() as usize;

    Ok(OutputInfo {
        duration_secs,
        video_streams: video,
        audio_streams: audio,
        subtitle_streams: subtitle,
        chapters,
    })
}

/// Decode sample video frames at evenly-spaced seek points (full mode).
/// Returns a VerifyCheck per seek point.
fn decode_sample_frames(path: &Path, duration_secs: f64) -> Vec<VerifyCheck> {
    let points = [0.0, 0.25, 0.50, 0.75, 0.90];
    let mut checks = Vec::new();

    let path_str = match path.to_str() {
        Some(s) => s,
        None => {
            checks.push(VerifyCheck {
                name: "decode_frames",
                passed: false,
                detail: "invalid path".into(),
            });
            return checks;
        }
    };

    for &pct in &points {
        let seek_secs = (duration_secs * pct) as i64;
        let label = format!("{:.0}%", pct * 100.0);
        match decode_frame_at(path_str, seek_secs) {
            Ok(()) => {
                checks.push(VerifyCheck {
                    name: "decode_frame",
                    passed: true,
                    detail: format!("decoded frame at {} ({}s)", label, seek_secs),
                });
            }
            Err(e) => {
                checks.push(VerifyCheck {
                    name: "decode_frame",
                    passed: false,
                    detail: format!("decode failed at {} ({}s): {}", label, seek_secs, e),
                });
            }
        }
    }

    checks
}

/// Seek to a position and decode one video frame.
fn decode_frame_at(path: &str, seek_secs: i64) -> Result<(), String> {
    use ffmpeg_the_third::{codec, format, media::Type, Packet};

    crate::media::ensure_init();

    let mut ictx = format::input(path).map_err(|e| e.to_string())?;

    // Find video stream index
    let video_idx = ictx
        .streams()
        .best(Type::Video)
        .ok_or("no video stream")?
        .index();

    // Seek
    let ts = seek_secs * ffmpeg_the_third::ffi::AV_TIME_BASE as i64;
    ictx.seek(ts, ..ts).map_err(|e| format!("seek failed: {}", e))?;

    // Open decoder
    let stream = ictx.stream(video_idx).ok_or("video stream not found")?;
    let decoder_codec = codec::decoder::find(stream.parameters().id())
        .ok_or("no decoder for video codec")?;
    let mut decoder = codec::Context::new()
        .decoder()
        .open_as(decoder_codec)
        .map_err(|e| format!("open decoder: {}", e))?;

    // Copy parameters from stream to decoder
    // Use unsafe to set parameters since the safe API requires specific setup
    unsafe {
        ffmpeg_the_third::ffi::avcodec_parameters_to_context(
            decoder.as_mut_ptr(),
            stream.parameters().as_ptr(),
        );
    }

    // Try to decode a frame from packets
    let mut frame = ffmpeg_the_third::frame::Video::empty();
    let mut packet = Packet::empty();
    let mut attempts = 0;

    loop {
        match packet.read(&mut ictx) {
            Ok(()) => {
                if packet.stream() != video_idx {
                    continue;
                }
                decoder.send_packet(&packet).map_err(|e| format!("send packet: {}", e))?;
                match decoder.receive_frame(&mut frame) {
                    Ok(()) => return Ok(()),
                    Err(ffmpeg_the_third::Error::Other { errno: ffmpeg_the_third::ffi::AVERROR(libc::EAGAIN) }) => {
                        // Need more packets
                    }
                    Err(e) => return Err(format!("receive frame: {}", e)),
                }
            }
            Err(ffmpeg_the_third::Error::Eof) => {
                return Err("reached EOF without decoding a frame".into());
            }
            Err(e) => {
                return Err(format!("read packet: {}", e));
            }
        }
        attempts += 1;
        if attempts > 100 {
            return Err("gave up after 100 packets without a decoded frame".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_within_tolerance_exact() {
        let (ok, _) = duration_within_tolerance(3600.0, 3600);
        assert!(ok);
    }

    #[test]
    fn test_duration_within_tolerance_within_2pct() {
        // 3600 * 0.02 = 72, so 3528 should pass
        let (ok, _) = duration_within_tolerance(3528.0, 3600);
        assert!(ok);
    }

    #[test]
    fn test_duration_within_tolerance_over_2pct() {
        // 3600 * 0.02 = 72, so 3500 should fail (2.78% short)
        let (ok, detail) = duration_within_tolerance(3500.0, 3600);
        assert!(!ok);
        assert!(detail.contains("short"));
    }

    #[test]
    fn test_duration_within_tolerance_zero_expected() {
        let (ok, _) = duration_within_tolerance(100.0, 0);
        assert!(ok);
    }

    #[test]
    fn test_stream_count_matches_pass() {
        let check = stream_count_matches(1, 1, "video");
        assert!(check.passed);
        assert_eq!(check.name, "video_streams");
    }

    #[test]
    fn test_stream_count_matches_fail() {
        let check = stream_count_matches(2, 3, "audio");
        assert!(!check.passed);
        assert_eq!(check.name, "audio_streams");
        assert!(check.detail.contains("expected 3"));
    }

    #[test]
    fn test_verify_nonexistent_file() {
        let expected = VerifyExpected {
            duration_secs: 3600,
            video_streams: 1,
            audio_streams: 2,
            subtitle_streams: 3,
            chapters: 10,
        };
        let result = verify_output(Path::new("/tmp/nonexistent_bluback_test.mkv"), &expected, VerifyLevel::Quick);
        assert!(!result.passed);
        assert_eq!(result.checks[0].name, "file_exists");
        assert!(!result.checks[0].passed);
    }

    #[test]
    fn test_verify_result_summary() {
        let result = VerifyResult {
            passed: false,
            level: VerifyLevel::Quick,
            checks: vec![
                VerifyCheck { name: "duration", passed: false, detail: "too short".into() },
                VerifyCheck { name: "video_streams", passed: true, detail: "1 video stream(s)".into() },
            ],
        };
        let failed: Vec<&str> = result.checks.iter().filter(|c| !c.passed).map(|c| c.name).collect();
        assert_eq!(failed, vec!["duration"]);
    }
}
```

- [ ] **Step 2: Register the module**

In `src/main.rs`, add `mod verify;` to the module list (after `mod util;`).

- [ ] **Step 3: Run tests to verify**

Run: `cargo test test_duration_within_tolerance test_stream_count_matches test_verify_nonexistent_file test_verify_result_summary`
Expected: All 6 new tests pass.

- [ ] **Step 4: Commit**

Suggest commit: `feat: add verify module with output probing and duration/stream checks`

---

### Task 3: Add config and CLI flags for verification

**Files:**
- Modify: `src/config.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write tests for verify config parsing**

In `src/config.rs` tests:

```rust
#[test]
fn test_parse_verify_config() {
    let toml_str = r#"
        verify = true
        verify_level = "full"
    "#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.verify, Some(true));
    assert_eq!(config.verify_level.as_deref(), Some("full"));
}

#[test]
fn test_verify_config_defaults() {
    let config = Config::default();
    assert!(!config.verify());
    assert_eq!(config.verify_level(), "quick");
}

#[test]
fn test_verify_config_serialization_roundtrip() {
    let config = Config {
        verify: Some(true),
        verify_level: Some("full".into()),
        ..Default::default()
    };
    let toml_str = config.to_toml_string();
    assert!(toml_str.contains("verify = true"));
    assert!(toml_str.contains(r#"verify_level = "full""#));
    let reparsed: Config = toml::from_str(&toml_str).unwrap();
    assert_eq!(reparsed.verify, Some(true));
    assert_eq!(reparsed.verify_level.as_deref(), Some("full"));
}

#[test]
fn test_validate_invalid_verify_level_warns() {
    let config = Config {
        verify_level: Some("deep".into()),
        ..Default::default()
    };
    let warnings = validate_config(&config);
    assert!(warnings.iter().any(|w| w.contains("verify_level")));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_parse_verify_config test_verify_config_defaults test_verify_config_serialization_roundtrip test_validate_invalid_verify_level`
Expected: FAIL — fields don't exist yet.

- [ ] **Step 3: Add verify fields to Config**

In `src/config.rs`, add to the `Config` struct:

```rust
pub verify: Option<bool>,
pub verify_level: Option<String>,
```

Add accessor methods to `impl Config`:

```rust
pub fn verify(&self) -> bool {
    self.verify.unwrap_or(false)
}

pub fn verify_level(&self) -> &str {
    self.verify_level.as_deref().unwrap_or("quick")
}
```

Add `"verify"` and `"verify_level"` to `KNOWN_KEYS`.

Add to `validate_config`:

```rust
if let Some(ref level) = config.verify_level {
    if !["quick", "full"].contains(&level.as_str()) {
        warnings.push(format!(
            "verify_level must be \"quick\" or \"full\", got \"{}\"",
            level
        ));
    }
}
```

Add to `to_toml_string` (after the `overwrite` line):

```rust
emit_bool(&mut out, "verify", self.verify, false);
emit_str(&mut out, "verify_level", &self.verify_level, "quick");
```

- [ ] **Step 4: Add CLI flags**

In `src/main.rs` `Args` struct:

```rust
/// Verify output files after ripping
#[arg(long)]
verify: bool,

/// Verification level: quick (header probe) or full (+ frame decode)
#[arg(long, value_parser = ["quick", "full"])]
verify_level: Option<String>,

/// Disable verification (overrides config)
#[arg(long)]
no_verify: bool,
```

- [ ] **Step 5: Run tests to verify**

Run: `cargo test`
Expected: All tests pass including the 4 new config tests.

- [ ] **Step 6: Update settings item count test**

The `test_settings_state_from_config_item_count` test in `src/types.rs` counts non-separator items. Adding verify settings will happen in Task 6, so skip this for now. Just make sure the test still passes with the current count.

- [ ] **Step 7: Commit**

Suggest commit: `feat: add verify and verify_level config options and CLI flags`

---

### Task 4: Integrate verification into TUI dashboard

**Files:**
- Modify: `src/types.rs:93-100` (PlaylistStatus)
- Modify: `src/tui/dashboard.rs`
- Modify: `src/session.rs`

- [ ] **Step 1: Extend PlaylistStatus**

In `src/types.rs`, update `PlaylistStatus`:

```rust
#[derive(Debug, Clone)]
pub enum PlaylistStatus {
    Pending,
    Ripping(RipProgress),
    Verifying,
    Done(u64),
    Verified(u64, crate::verify::VerifyResult),
    VerifyFailed(u64, crate::verify::VerifyResult),
    Skipped(u64),
    Failed(String),
}
```

- [ ] **Step 2: Fix all pattern matches on PlaylistStatus**

Adding new variants will cause exhaustiveness errors across the codebase. Fix each match:

In `src/tui/dashboard.rs` `render_dashboard_view`:
- `Verifying` renders as `("Verifying...", String::new(), String::new())` with `Style::default().fg(Color::Yellow)`
- `Verified(sz, _)` renders same as `Done(sz)` but status text is "Verified"
- `VerifyFailed(sz, ref result)` renders status as "Verify failed" with `Style::default().fg(Color::Yellow)`, size as `format_size(*sz)`

In `src/tui/dashboard.rs` `render_done_view`:
- Add `Verified(sz, _)` alongside `Done(sz)` in the `completed` filter
- Add `VerifyFailed(sz, ref result)` to the results display showing each failed check

In `src/tui/dashboard.rs` `check_all_done_session`:
- Add `Verified(..)` and `VerifyFailed(..)` to the `all_done` match
- Add `Verified(sz, _)` to the `succeeded` fold arm

In `src/tui/dashboard.rs` `build_post_rip_vars`:
- Add `Verified(size, _)` alongside `Done(size)` for file_size extraction

In `src/tui/dashboard.rs` `active_rip_stats_view`:
- No change needed (only matches `Ripping`)

In `src/session.rs` `tab_summary`:
- Add `Verified(..)` alongside `Done(..)` in `done_count`

In `src/tui/dashboard.rs` `render_dashboard_view` row styling:
- Add `Verified(..)` alongside `Done(..) | Skipped(..)` for `Color::DarkGray`
- Add `VerifyFailed(..)` with `Color::Yellow`

Search for all other `PlaylistStatus::Done` matches across the codebase and add `Verified` alongside them where appropriate.

- [ ] **Step 3: Add chapters_added tracking to RipState**

The TUI remux thread currently discards `chapters_added` (sender drops on success). Add an `Arc<AtomicUsize>` to `RipState` so the remux thread can write it back:

In `src/tui/mod.rs` (or wherever `RipState` is defined), add:

```rust
pub chapters_added: std::sync::Arc<std::sync::atomic::AtomicUsize>,
```

Initialize as `Arc::new(AtomicUsize::new(0))` in the default.

In `src/tui/dashboard.rs` `start_next_job_session`, reset it before spawning and capture it in the thread:

```rust
session.rip.chapters_added.store(0, Ordering::Relaxed);
let chapters_added = session.rip.chapters_added.clone();
```

In the spawned thread, store the result:

```rust
match result {
    Ok(added) => {
        chapters_added.store(added, Ordering::Relaxed);
    }
    Err(e) => {
        let _ = tx.send(Err(e));
    }
}
```

- [ ] **Step 4: Thread verify config into sessions**

In `src/session.rs`, add to `DriveSession`:

```rust
pub verify: bool,
pub verify_level: crate::verify::VerifyLevel,
```

Initialize in `DriveSession::new`:

```rust
verify: false,
verify_level: crate::verify::VerifyLevel::Quick,
```

- [ ] **Step 5: Wire verify flags from Args into session creation**

In `src/tui/coordinator.rs` (or wherever sessions are spawned from args), set:

```rust
session.verify = args.verify || (!args.no_verify && config.verify());
session.verify_level = match args.verify_level.as_deref().or_else(|| {
    if session.verify { Some(config.verify_level()) } else { None }
}) {
    Some("full") => crate::verify::VerifyLevel::Full,
    _ => crate::verify::VerifyLevel::Quick,
};
```

Also in `src/main.rs`, pass `args.verify`, `args.verify_level`, and `args.no_verify` to the TUI `run` call if needed (check how args flow to sessions).

- [ ] **Step 6: Add verification call in poll_active_job_session**

In `src/tui/dashboard.rs` `poll_active_job_session`, in the `Disconnected` arm (line 659-669), after getting `file_size`, add verification:

```rust
Err(mpsc::TryRecvError::Disconnected) => {
    let idx = session.rip.current_rip;
    if matches!(session.rip.jobs[idx].status, PlaylistStatus::Ripping(_)) {
        let outfile = session.output_dir.join(&session.rip.jobs[idx].filename);
        let file_size = std::fs::metadata(&outfile).map(|m| m.len()).unwrap_or(0);

        if session.verify {
            let playlist = &session.rip.jobs[idx].playlist;
            let expected = crate::verify::VerifyExpected {
                duration_secs: playlist.seconds,
                video_streams: playlist.video_streams,
                audio_streams: playlist.audio_streams,
                subtitle_streams: playlist.subtitle_streams,
                chapters: session.rip.chapters_added.load(std::sync::atomic::Ordering::Relaxed),
            };
            session.rip.jobs[idx].status = PlaylistStatus::Verifying;
            let result = crate::verify::verify_output(&outfile, &expected, session.verify_level);
            if result.passed {
                session.rip.jobs[idx].status = PlaylistStatus::Verified(file_size, result);
                let vars = build_post_rip_vars(session, idx, "success", "");
                crate::hooks::run_post_rip(&session.config, &vars, session.no_hooks);
            } else {
                let detail: String = result.checks.iter()
                    .filter(|c| !c.passed)
                    .map(|c| c.detail.clone())
                    .collect::<Vec<_>>()
                    .join("; ");
                log::warn!("Verification failed for {}: {}", session.rip.jobs[idx].filename, detail);
                session.rip.jobs[idx].status = PlaylistStatus::VerifyFailed(file_size, result);
                let vars = build_post_rip_vars(session, idx, "success", "");
                crate::hooks::run_post_rip(&session.config, &vars, session.no_hooks);
            }
        } else {
            session.rip.jobs[idx].status = PlaylistStatus::Done(file_size);
            let vars = build_post_rip_vars(session, idx, "success", "");
            crate::hooks::run_post_rip(&session.config, &vars, session.no_hooks);
        }
    }
    session.rip.progress_rx = None;
    return true;
}
```

- [ ] **Step 7: Run tests to verify compilation**

Run: `cargo test`
Expected: All tests pass. New `PlaylistStatus` variants are handled everywhere.

- [ ] **Step 8: Commit**

Suggest commit: `feat: integrate verification into TUI dashboard after remux`

---

### Task 5: Add TUI verify failure prompt

**Files:**
- Modify: `src/tui/dashboard.rs`
- Modify: `src/session.rs` (or `src/tui/mod.rs` for RipState)

- [ ] **Step 1: Add verify failure prompt state to RipState**

Find `RipState` (likely in `src/tui/mod.rs`) and add:

```rust
pub verify_failed_idx: Option<usize>,
```

Initialize as `None` in the default.

- [ ] **Step 2: Trigger prompt when verification fails**

In `poll_active_job_session`, after setting `VerifyFailed`, set:

```rust
session.rip.verify_failed_idx = Some(idx);
```

- [ ] **Step 3: Render the verify failure prompt**

In `render_dashboard_view`, when `view.verify_failed_idx.is_some()` (add this field to `DashboardView`), render a prompt overlay or replace the key hints line:

```rust
if let Some(fail_idx) = view.verify_failed_idx {
    if let Some(job) = view.jobs.get(fail_idx) {
        if let PlaylistStatus::VerifyFailed(_, ref result) = job.status {
            let failed_details: Vec<&str> = result.checks.iter()
                .filter(|c| !c.passed)
                .map(|c| c.detail.as_str())
                .collect();
            let msg = format!(
                "{} failed verification: {}  [D]elete & retry  [K]eep  [S]kip",
                job.filename,
                failed_details.join("; ")
            );
            let hint = Paragraph::new(msg)
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            f.render_widget(hint, chunks[2]);
            return; // Skip normal hints
        }
    }
}
```

- [ ] **Step 4: Handle verify failure prompt input**

In `handle_input_session`, add handling before the existing `confirm_abort` check:

```rust
if let Some(fail_idx) = session.rip.verify_failed_idx {
    match key.code {
        KeyCode::Char('d') | KeyCode::Char('D') => {
            // Delete and retry
            let outfile = session.output_dir.join(&session.rip.jobs[fail_idx].filename);
            let _ = std::fs::remove_file(&outfile);
            session.rip.jobs[fail_idx].status = PlaylistStatus::Pending;
            session.rip.verify_failed_idx = None;
        }
        KeyCode::Char('k') | KeyCode::Char('K') => {
            // Keep as-is
            if let PlaylistStatus::VerifyFailed(sz, _) = &session.rip.jobs[fail_idx].status {
                session.rip.jobs[fail_idx].status = PlaylistStatus::Done(*sz);
            }
            session.rip.verify_failed_idx = None;
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            // Skip (delete file)
            let outfile = session.output_dir.join(&session.rip.jobs[fail_idx].filename);
            let _ = std::fs::remove_file(&outfile);
            session.rip.jobs[fail_idx].status = PlaylistStatus::Skipped(0);
            session.rip.verify_failed_idx = None;
        }
        _ => {}
    }
    return;
}
```

- [ ] **Step 5: Add verify_failed_idx to DashboardView**

In `src/types.rs` `DashboardView`:

```rust
pub verify_failed_idx: Option<usize>,
```

In `src/session.rs` `build_dashboard_view`:

```rust
verify_failed_idx: self.rip.verify_failed_idx,
```

- [ ] **Step 6: Run tests to verify**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 7: Commit**

Suggest commit: `feat: add TUI prompt for verification failures (delete/keep/skip)`

---

### Task 6: Integrate verification into CLI path

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Add verification after remux success in CLI**

In `src/cli.rs`, in the `Ok(chapters_added)` arm of the remux result (around line 1074), add verification:

```rust
Ok(chapters_added) => {
    let final_size = std::fs::metadata(outfile)?.len();
    if !is_tty {
        println!("  [{}] 100% {} -- done", pl.num, format_size(final_size));
    }
    println!("Done: {} ({})", filename, format_size(final_size));
    if chapters_added > 0 {
        println!("  Added {} chapter markers", chapters_added);
    }

    // Verification
    let do_verify = args.verify || (!args.no_verify && config.verify());
    if do_verify {
        let level = match args.verify_level.as_deref().unwrap_or(config.verify_level()) {
            "full" => crate::verify::VerifyLevel::Full,
            _ => crate::verify::VerifyLevel::Quick,
        };
        let expected = crate::verify::VerifyExpected {
            duration_secs: pl.seconds,
            video_streams: pl.video_streams,
            audio_streams: pl.audio_streams,
            subtitle_streams: pl.subtitle_streams,
            chapters: chapters_added,
        };
        let result = crate::verify::verify_output(outfile, &expected, level);
        if result.passed {
            println!("  Verified ({:?}): all checks passed", level);
        } else {
            let failed: Vec<&str> = result.checks.iter()
                .filter(|c| !c.passed)
                .map(|c| c.detail.as_str())
                .collect();
            log::warn!("Verification failed for {}: {}", filename, failed.join("; "));
            println!("  WARNING: verification failed: {}", failed.join("; "));
        }
    }

    success_count += 1;
    ("success", String::new(), final_size, chapters_added)
}
```

- [ ] **Step 2: Run tests to verify compilation**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

Suggest commit: `feat: integrate verification into CLI path`

---

### Task 7: Add hook variables and settings panel items

**Files:**
- Modify: `src/hooks.rs` (documentation only — vars are built at call sites)
- Modify: `src/tui/dashboard.rs` (add verify vars to `build_post_rip_vars`)
- Modify: `src/cli.rs` (add verify vars to hook vars)
- Modify: `src/types.rs` (settings panel items)
- Modify: `src/config.rs` (settings to_config/from_config)

- [ ] **Step 1: Add {verify} and {verify_detail} to TUI post-rip vars**

In `src/tui/dashboard.rs` `build_post_rip_vars`, add:

```rust
// Determine verify status from job status
let (verify_status, verify_detail) = match &job.status {
    PlaylistStatus::Verified(_, ref result) => {
        ("passed".to_string(), String::new())
    }
    PlaylistStatus::VerifyFailed(_, ref result) => {
        let detail = result.checks.iter()
            .filter(|c| !c.passed)
            .map(|c| c.name)
            .collect::<Vec<_>>()
            .join(",");
        ("failed".to_string(), detail)
    }
    _ => ("skipped".to_string(), String::new()),
};
vars.insert("verify", verify_status);
vars.insert("verify_detail", verify_detail);
```

- [ ] **Step 2: Add {verify} and {verify_detail} to CLI post-rip vars**

In `src/cli.rs`, in the hook variables block (around line 1117), add:

```rust
vars.insert("verify", verify_status.to_string());
vars.insert("verify_detail", verify_detail_str);
```

Where `verify_status` and `verify_detail_str` are set based on whether verification ran and its result. Default to `"skipped"` and `""` when verify is off.

- [ ] **Step 3: Add settings panel items for verification**

In `src/types.rs` `from_config_with_drives`, add after the `overwrite` Toggle item:

```rust
SettingItem::Toggle {
    label: "Verify Rips".into(),
    key: "verify".into(),
    value: config.verify.unwrap_or(false),
},
SettingItem::Choice {
    label: "Verify Level".into(),
    key: "verify_level".into(),
    options: vec!["quick".into(), "full".into()],
    selected: match config.verify_level.as_deref() {
        Some("full") => 1,
        _ => 0,
    },
    custom_value: None,
},
```

- [ ] **Step 4: Add to_config handling for verify settings**

In `src/types.rs` `to_config`, add match arms:

```rust
"verify" if *value => config.verify = Some(true),
```

```rust
"verify_level" => {
    let val = &options[*selected];
    if val != "quick" {
        config.verify_level = Some(val.clone());
    }
}
```

- [ ] **Step 5: Add env var overrides for verify settings**

In `src/types.rs`, add to `ENV_MAPPINGS` in both `apply_env_overrides` and `active_env_var_warnings`:

```rust
("BLUBACK_VERIFY", "verify"),
("BLUBACK_VERIFY_LEVEL", "verify_level"),
```

- [ ] **Step 6: Update settings item count test**

Update `test_settings_state_from_config_item_count` to reflect the 2 new items (29 non-separator items instead of 27).

- [ ] **Step 7: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 8: Commit**

Suggest commit: `feat: add verify hook variables and settings panel items`

---

### Task 8: Update CLAUDE.md and roadmap

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/ROADMAP-1.0.md`

- [ ] **Step 1: Mark item 19 as complete in roadmap**

In `docs/ROADMAP-1.0.md`, update item 19:

```markdown
### 19. Rip verification ✓
- Post-remux: probe output file, compare expected vs actual duration, verify streams present
- **Two levels:** `quick` (header probe, milliseconds) and `full` (+ sample frame decode)
- **Off by default**, opt-in via `--verify` / config toggle / settings panel
- **Duration tolerance:** 2%
- **TUI:** prompt on failure (delete & retry / keep / skip)
- **CLI:** log warning, keep file
- **Hook variables:** `{verify}`, `{verify_detail}`
- **Files:** New `src/verify.rs`, `src/types.rs`, `src/config.rs`, `src/tui/dashboard.rs`, `src/cli.rs`, `src/session.rs`
```

- [ ] **Step 2: Update CLAUDE.md**

Add to the "Key Design Decisions" section:

```markdown
- **Rip verification** — Optional post-remux validation. `verify` config (default false) + `verify_level` ("quick" or "full"). Quick: probe output MKV headers — check duration (2% tolerance), stream counts, chapter count. Full: adds sample frame decode at 5 seek points. `--verify`, `--verify-level`, `--no-verify` CLI flags. TUI prompts on failure (delete & retry / keep / skip). CLI logs warning. Hook vars: `{verify}` (passed/failed/skipped), `{verify_detail}` (comma-separated failed check names).
```

Add to CLI flags section:

```
      --verify               Verify output files after ripping
      --verify-level <LEVEL> Verification level: quick or full
      --no-verify            Disable verification (overrides config)
```

Add `verify.rs` to Architecture section.

- [ ] **Step 3: Run fmt and clippy**

Run: `rustup run stable cargo fmt && cargo clippy -- -D warnings`
Expected: Clean.

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

Suggest commit: `docs: update CLAUDE.md and roadmap for rip verification`
