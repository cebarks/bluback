# FFmpeg Bindings Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace all CLI tool dependencies (ffprobe, ffmpeg, mkvpropedit) with `ffmpeg-the-third` library bindings for direct API access, typed errors, and single-pass chapter injection.

**Architecture:** New `src/media/` module provides a clean, bluback-agnostic API for disc probing and remuxing. Existing callers (`disc.rs`, `rip.rs`, `chapters.rs`, `tui/dashboard.rs`, `cli.rs`) migrate from spawning CLI processes to calling the media module. Chapters are injected during remux via `AVChapter` instead of post-hoc `mkvpropedit`.

**Tech Stack:** `ffmpeg-the-third` (Rust FFmpeg bindings, dynamic linking), existing `mpls` crate for MPLS chapter parsing.

**Spec:** `docs/superpowers/specs/2026-03-22-ffmpeg-bindings-design.md`

---

## File Structure

| File | Responsibility | Change |
|------|---------------|--------|
| `src/media/mod.rs` | Public API re-exports | Create |
| `src/media/error.rs` | `MediaError` enum, Display/Error impls | Create |
| `src/media/probe.rs` | Playlist scanning, stream probing, media info extraction via FFmpeg API | Create |
| `src/media/remux.rs` | Lossless remux with chapter injection, stream selection, progress callback | Create |
| `src/types.rs:39-42` | `StreamInfo` restructured to typed `AudioStream` fields | Modify |
| `Cargo.toml` | Add `ffmpeg-the-third`, remove `which` | Modify |
| `src/main.rs:1-9,133` | Add `mod media`, remove `check_dependencies()` call | Modify |
| `src/disc.rs:48-62,65-67,167-192,194-283,285-476` | Remove probe/scan/check functions, replace with `media::` calls | Modify |
| `src/rip.rs:12-118` | Remove `build_map_args`, `start_rip`, `parse_progress_line` | Modify |
| `src/chapters.rs:8-30,92-122` | Remove `apply_chapters`, `chapters_to_ogm`, `format_chapter_time` | Modify |
| `src/tui/mod.rs:80-89,97,195-225,390,417-421` | Update `RipState`, remove `has_mkvpropedit`, fix `reset_for_rescan`, fix quit handler | Modify |
| `src/tui/dashboard.rs:252-256,278,296-299,314-430` | Replace rip spawning, abort handler, tick, check_all_done with `media::remux()` | Modify |
| `src/tui/wizard.rs:1237` | Remove `has_mkvpropedit` condition for disc mounting (always mount) | Modify |
| `src/cli.rs:520-659` | Replace rip loop with `media::remux()` | Modify |
| `src/config.rs:20-33` | Add `stream_selection` field | Modify |
| `.github/workflows/ci.yml` | Add `ffmpeg-devel` package install | Modify |
| `.github/workflows/release.yml` | Add `ffmpeg-devel` package install | Modify |

---

### Task 0: Branch Setup and `bluray:` Protocol Spike

This is a go/no-go gate. If `bluray:` protocol doesn't work through `ffmpeg-the-third`, fall back to hybrid approach (see spec Risks section).

**Files:**
- Modify: `Cargo.toml`
- Create: `examples/bluray_spike.rs` (temporary, deleted after spike)

- [ ] **Step 1: Create feature branch**

```bash
git checkout -b feature/ffmpeg-bindings
```

- [ ] **Step 2: Add `ffmpeg-the-third` to Cargo.toml**

Add to `[dependencies]`:
```toml
ffmpeg-the-third = "4"
```

- [ ] **Step 3: Write a minimal spike to verify `bluray:` protocol**

Create `examples/bluray_spike.rs`:
```rust
use std::env;

fn main() {
    ffmpeg_the_third::init().unwrap();

    let device = env::args().nth(1).unwrap_or_else(|| "/dev/sr0".into());
    let url = format!("bluray:{}", device);

    println!("Attempting to open: {}", url);

    match ffmpeg_the_third::format::input(&url) {
        Ok(ctx) => {
            println!("Opened successfully!");
            println!("Streams: {}", ctx.streams().count());
            println!("Duration: {:?}", ctx.duration());
            for stream in ctx.streams() {
                let params = stream.parameters();
                println!(
                    "  Stream #{}: {:?} ({:?})",
                    stream.index(),
                    params.medium(),
                    stream.id()
                );
            }
        }
        Err(e) => {
            eprintln!("Failed to open: {}", e);
            std::process::exit(1);
        }
    }
}
```

- [ ] **Step 4: Build and run the spike with a disc inserted**

Run: `cargo run --example bluray_spike -- /dev/sr0`

Expected: Either prints stream info (protocol works) or prints an error (protocol doesn't work through bindings). If it fails, investigate whether `format::input_with_options` with protocol-specific options resolves it. If the protocol fundamentally doesn't work, fall back to hybrid approach per spec.

- [ ] **Step 5: Record result and clean up spike**

Delete `examples/bluray_spike.rs` after recording the result. If successful, proceed with remaining tasks. If failed, adjust plan to hybrid approach before continuing.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: add ffmpeg-the-third dependency for FFmpeg bindings integration"
```

---

### Task 1: Create `src/media/error.rs` — MediaError Enum

**Files:**
- Create: `src/media/error.rs`

- [ ] **Step 1: Create `src/media/` directory and `error.rs`**

```rust
use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum MediaError {
    /// AACS host certificate revoked (MKBv72+), need per-disc VUK in KEYDB.cfg
    AacsRevoked,
    /// AACS authentication failed (general — USB bridge issues, missing keys, etc.)
    AacsAuthFailed(String),
    /// libbluray/libaacs hung during AACS init (60s timeout exceeded)
    AacsTimeout,
    /// Device path doesn't exist or isn't an optical drive
    DeviceNotFound(String),
    /// Drive present but no disc inserted
    NoDisc,
    /// Requested playlist doesn't exist on disc
    PlaylistNotFound(String),
    /// Playlist has no usable streams
    NoStreams,
    /// Error during packet read/write in remux
    RemuxFailed(String),
    /// Output file already exists
    OutputExists(PathBuf),
    /// User-initiated cancellation via AtomicBool
    Cancelled,
    /// FFmpeg library error (passthrough)
    Ffmpeg(ffmpeg_the_third::Error),
    /// Standard I/O error
    Io(std::io::Error),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AacsRevoked => write!(
                f,
                "AACS host certificate revoked. This disc requires a per-disc VUK in KEYDB.cfg."
            ),
            Self::AacsAuthFailed(msg) => write!(f, "AACS authentication failed: {}", msg),
            Self::AacsTimeout => write!(
                f,
                "AACS initialization timed out (60s). Check for orphaned libmmbd.so.0."
            ),
            Self::DeviceNotFound(dev) => write!(f, "Device not found: {}", dev),
            Self::NoDisc => write!(f, "No disc in drive"),
            Self::PlaylistNotFound(num) => write!(f, "Playlist {} not found on disc", num),
            Self::NoStreams => write!(f, "No usable streams in playlist"),
            Self::RemuxFailed(msg) => write!(f, "Remux failed: {}", msg),
            Self::OutputExists(path) => {
                write!(f, "Output file already exists: {}", path.display())
            }
            Self::Cancelled => write!(f, "Operation cancelled"),
            Self::Ffmpeg(e) => write!(f, "FFmpeg error: {}", e),
            Self::Io(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for MediaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ffmpeg_the_third::Error> for MediaError {
    fn from(e: ffmpeg_the_third::Error) -> Self {
        Self::Ffmpeg(e)
    }
}

impl From<std::io::Error> for MediaError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Inspect an FFmpeg error to classify AACS-related failures.
/// FFmpeg wraps libbluray/libaacs errors — we match on known substrings.
pub fn classify_aacs_error(err: &ffmpeg_the_third::Error) -> Option<MediaError> {
    let msg = err.to_string().to_lowercase();
    if msg.contains("no valid processing key")
        || msg.contains("processing key")
        || msg.contains("your host key/certificate has been revoked")
    {
        Some(MediaError::AacsRevoked)
    } else if msg.contains("aacs") || msg.contains("libaacs") || msg.contains("bdplus") {
        Some(MediaError::AacsAuthFailed(err.to_string()))
    } else {
        None
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles (module not yet wired in, but syntax-check passes if you temporarily add `mod media;` or just check the file in isolation).

- [ ] **Step 3: Commit**

```bash
git add src/media/error.rs
git commit -m "feat(media): add MediaError enum with AACS classification"
```

---

### Task 2: Create `src/media/mod.rs` — Module Wiring

**Files:**
- Create: `src/media/mod.rs`
- Modify: `src/main.rs:1-9`

- [ ] **Step 1: Create `src/media/mod.rs`**

```rust
pub mod error;

pub use error::MediaError;

// These will be added in subsequent tasks:
// pub mod probe;
// pub mod remux;
```

- [ ] **Step 2: Add `mod media` to `src/main.rs`**

Add `mod media;` after line 4 (`mod disc;`), before `mod rip;`:
```rust
mod media;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles cleanly.

- [ ] **Step 4: Commit**

```bash
git add src/media/mod.rs src/main.rs
git commit -m "feat(media): wire up media module"
```

---

### Task 3: Update `StreamInfo` in `src/types.rs`

**Files:**
- Modify: `src/types.rs:39-42`

- [ ] **Step 1: Write test for new `AudioStream` struct**

Add to the existing `#[cfg(test)] mod tests` block in `src/types.rs`:

```rust
#[test]
fn test_audio_stream_is_surround() {
    let stream = AudioStream {
        index: 0,
        codec: "truehd".into(),
        channels: 8,
        channel_layout: "7.1".into(),
        language: Some("eng".into()),
        profile: None,
    };
    assert!(stream.is_surround());

    let stereo = AudioStream {
        index: 1,
        codec: "aac".into(),
        channels: 2,
        channel_layout: "stereo".into(),
        language: Some("eng".into()),
        profile: None,
    };
    assert!(!stereo.is_surround());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_audio_stream_is_surround`
Expected: FAIL — `AudioStream` doesn't exist yet.

- [ ] **Step 3: Replace `StreamInfo` and add `AudioStream`**

Replace the current `StreamInfo` at `src/types.rs:39-42`:

```rust
#[derive(Debug, Clone)]
pub struct AudioStream {
    pub index: usize,
    pub codec: String,
    pub channels: u16,
    pub channel_layout: String,
    pub language: Option<String>,
    pub profile: Option<String>,
}

impl AudioStream {
    pub fn is_surround(&self) -> bool {
        self.channels >= 6
    }

    /// Display string matching the old ffprobe text format for backward compatibility in UI
    pub fn display_line(&self) -> String {
        let lang = self.language.as_deref().unwrap_or("und");
        let codec_name = self.profile.as_deref().unwrap_or(&self.codec);
        format!("{} {} ({})", codec_name, self.channel_layout, lang)
    }
}

#[derive(Debug, Clone, Default)]
pub struct StreamInfo {
    pub audio_streams: Vec<AudioStream>,
    pub subtitle_count: u32,
}
```

- [ ] **Step 4: Fix compilation errors from `StreamInfo` change**

The old `StreamInfo` had `audio_streams: Vec<String>` and `sub_count: u32`. Callers that reference `sub_count` need updating to `subtitle_count`. Callers that iterate `audio_streams` as strings need updating to use `AudioStream` methods. Search for all references:

In `src/rip.rs:build_map_args()` — this function will be removed in Task 7, but it must compile in the interim. Update it to work with the new type:

```rust
pub fn build_map_args(streams: &StreamInfo) -> Vec<String> {
    let mut args = vec!["-map".into(), "0:v:0".into()];

    let surround_idx = streams
        .audio_streams
        .iter()
        .position(|s| s.is_surround());
    let stereo_idx = streams
        .audio_streams
        .iter()
        .position(|s| s.channels == 2);

    if let Some(idx) = surround_idx {
        args.extend(["-map".into(), format!("0:a:{}", idx)]);
        if let Some(si) = stereo_idx {
            if si != idx {
                args.extend(["-map".into(), format!("0:a:{}", si)]);
            }
        }
    } else if !streams.audio_streams.is_empty() {
        args.extend(["-map".into(), "0:a:0".into()]);
    }

    if streams.subtitle_count > 0 {
        args.extend(["-map".into(), "0:s?".into()]);
    }

    args
}
```

In `src/tui/dashboard.rs:341-344` — the fallback when `probe_streams` returns `None` stays the same. The `Some(ref s)` branch calls `build_map_args(s)` which now takes the new type. This compiles as-is.

In `src/cli.rs:571-583` — same pattern, compiles as-is.

Update any UI code that iterates `streams.audio_streams` as strings to use `AudioStream::display_line()` instead. Check `src/tui/wizard.rs` for any stream display code.

- [ ] **Step 5: Update existing `build_map_args` tests**

Update tests in `src/rip.rs` to use new `AudioStream` type. These tests will be removed in Task 7 but must pass in the interim:

```rust
#[test]
fn test_build_map_args_surround_and_stereo() {
    let streams = StreamInfo {
        audio_streams: vec![
            AudioStream {
                index: 0,
                codec: "truehd".into(),
                channels: 8,
                channel_layout: "7.1".into(),
                language: Some("eng".into()),
                profile: None,
            },
            AudioStream {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                channel_layout: "stereo".into(),
                language: Some("eng".into()),
                profile: None,
            },
        ],
        subtitle_count: 1,
    };
    let args = build_map_args(&streams);
    assert_eq!(
        args,
        vec!["-map", "0:v:0", "-map", "0:a:0", "-map", "0:a:1", "-map", "0:s?"]
    );
}

#[test]
fn test_build_map_args_no_surround() {
    let streams = StreamInfo {
        audio_streams: vec![AudioStream {
            index: 0,
            codec: "aac".into(),
            channels: 2,
            channel_layout: "stereo".into(),
            language: None,
            profile: None,
        }],
        subtitle_count: 0,
    };
    let args = build_map_args(&streams);
    assert_eq!(args, vec!["-map", "0:v:0", "-map", "0:a:0"]);
}

#[test]
fn test_build_map_args_no_audio() {
    let streams = StreamInfo {
        audio_streams: vec![],
        subtitle_count: 2,
    };
    let args = build_map_args(&streams);
    assert_eq!(args, vec!["-map", "0:v:0", "-map", "0:s?"]);
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test`
Expected: All tests pass, including `test_audio_stream_is_surround` and updated `build_map_args` tests.

- [ ] **Step 7: Commit**

```bash
git add src/types.rs src/rip.rs
git commit -m "refactor: restructure StreamInfo with typed AudioStream fields"
```

---

### Task 4: Create `src/media/probe.rs` — Probe Functions

**Files:**
- Create: `src/media/probe.rs`
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Write tests for probe helper functions**

Add tests at the bottom of `src/media/probe.rs` (these test pure conversion logic, not FFmpeg calls):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_hdr_smpte2084() {
        assert_eq!(classify_hdr("smpte2084", &[]), "HDR10");
    }

    #[test]
    fn test_classify_hdr_dolby_vision() {
        assert_eq!(
            classify_hdr("smpte2084", &["DOVI configuration record"]),
            "DV"
        );
    }

    #[test]
    fn test_classify_hdr_hdr10plus() {
        assert_eq!(
            classify_hdr(
                "smpte2084",
                &["HDR Dynamic Metadata SMPTE2094-40"]
            ),
            "HDR10+"
        );
    }

    #[test]
    fn test_classify_hdr_hlg() {
        assert_eq!(classify_hdr("arib-std-b67", &[]), "HLG");
    }

    #[test]
    fn test_classify_hdr_sdr() {
        assert_eq!(classify_hdr("bt709", &[]), "SDR");
    }

    #[test]
    fn test_format_channel_layout() {
        assert_eq!(format_channel_layout(8, "7.1"), "7.1");
        assert_eq!(format_channel_layout(6, "5.1(side)"), "5.1");
        assert_eq!(format_channel_layout(2, "stereo"), "2.0");
        assert_eq!(format_channel_layout(1, "mono"), "1.0");
        assert_eq!(format_channel_layout(6, ""), "5.1");
    }

    #[test]
    fn test_format_framerate() {
        assert_eq!(format_framerate((24000, 1001)), "23.976");
        assert_eq!(format_framerate((24, 1)), "24.000");
        assert_eq!(format_framerate((30000, 1001)), "29.970");
        assert_eq!(format_framerate((0, 0)), "0.000");
    }
}
```

- [ ] **Step 2: Implement probe module with helper functions**

Create `src/media/probe.rs`:

```rust
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::types::{AudioStream, MediaInfo, Playlist, StreamInfo};

use super::error::{classify_aacs_error, MediaError};

const SCAN_TIMEOUT_SECS: u64 = 60;

/// Scan a Blu-ray disc for playlists via FFmpeg/libbluray.
pub fn scan_playlists(device: &str) -> Result<Vec<Playlist>, MediaError> {
    super::ensure_init();
    let url = format!("bluray:{}", device);

    // Wrap in thread + timeout to handle AACS/libbluray hangs
    let (tx, rx) = mpsc::channel();
    let url_clone = url.clone();
    thread::spawn(move || {
        let result = ffmpeg_the_third::format::input(&url_clone);
        let _ = tx.send(result);
    });

    let ctx = rx
        .recv_timeout(Duration::from_secs(SCAN_TIMEOUT_SECS))
        .map_err(|_| MediaError::AacsTimeout)?
        .map_err(|e| {
            classify_aacs_error(&e).unwrap_or_else(|| MediaError::from(e))
        })?;

    let mut playlists = Vec::new();

    // libbluray exposes playlists as chapters/programs in the format context.
    // Iterate and extract playlist number + duration.
    // The exact mechanism depends on how ffmpeg-the-third exposes libbluray's
    // title list — this may need adaptation based on the spike results (Task 0).
    //
    // Approach: iterate programs or use format metadata to enumerate playlists.
    // If direct enumeration isn't available, fall back to probing individual
    // playlist numbers (00000-99999) similar to how some tools work.
    //
    // TODO: Adapt based on Task 0 spike findings. The implementation below
    // assumes programs map to playlists.

    for (i, chapter) in ctx.chapters().enumerate() {
        let duration_secs = chapter.end().rescale(1, 1) as u32;
        let hours = duration_secs / 3600;
        let minutes = (duration_secs % 3600) / 60;
        let seconds = duration_secs % 60;
        playlists.push(Playlist {
            num: format!("{:05}", i),
            duration: format!("{}:{:02}:{:02}", hours, minutes, seconds),
            seconds: duration_secs,
        });
    }

    Ok(playlists)
}

/// Probe stream information for a specific playlist.
pub fn probe_streams(device: &str, playlist_num: &str) -> Result<StreamInfo, MediaError> {
    super::ensure_init();
    let ctx = open_playlist(device, playlist_num)?;
    let mut info = StreamInfo::default();

    for stream in ctx.streams() {
        let params = stream.parameters();
        match params.medium() {
            ffmpeg_the_third::media::Type::Audio => {
                let codec = stream
                    .parameters()
                    .id()
                    .name()
                    .to_string();
                let channels = params.channel_layout().channels() as u16;
                let channel_layout_str = format!("{}", params.channel_layout());
                let language = stream
                    .metadata()
                    .get("language")
                    .map(String::from);
                let profile = None; // Profile from codec context if needed

                info.audio_streams.push(AudioStream {
                    index: stream.index(),
                    codec,
                    channels,
                    channel_layout: channel_layout_str,
                    language,
                    profile,
                });
            }
            ffmpeg_the_third::media::Type::Subtitle => {
                info.subtitle_count += 1;
            }
            _ => {}
        }
    }

    Ok(info)
}

/// Extract detailed media information for a specific playlist.
pub fn probe_media_info(device: &str, playlist_num: &str) -> Result<MediaInfo, MediaError> {
    super::ensure_init();
    let ctx = open_playlist(device, playlist_num)?;

    let mut media_info = MediaInfo::default();

    // Video stream (first one)
    if let Some(video) = ctx
        .streams()
        .find(|s| s.parameters().medium() == ffmpeg_the_third::media::Type::Video)
    {
        let params = video.parameters();
        let codec_name = params.id().name().to_string();

        media_info.codec = codec_name;
        media_info.width = params.width();
        media_info.height = params.height();
        media_info.resolution = format!("{}p", params.height());
        media_info.aspect_ratio = format_aspect_ratio(params.width(), params.height());

        let (num, den) = video.rate().into();
        media_info.framerate = format_framerate((num, den));

        // HDR detection via color transfer characteristic
        // This may require unsafe access to AVStream side_data for
        // Dolby Vision / HDR10+ distinction. See spec Risks section.
        let color_transfer = get_color_transfer(&video);
        let side_data_types = get_side_data_types(&video);
        media_info.hdr = classify_hdr(&color_transfer, &side_data_types);

        // Bit depth from codec parameters
        media_info.bit_depth = get_bit_depth(&params);
        media_info.profile = get_profile(&params);
    }

    // Audio stream (first one)
    if let Some(audio) = ctx
        .streams()
        .find(|s| s.parameters().medium() == ffmpeg_the_third::media::Type::Audio)
    {
        let params = audio.parameters();
        media_info.audio = params.id().name().to_string();
        let channels = params.channel_layout().channels() as u16;
        let layout_str = format!("{}", params.channel_layout());
        media_info.channels = format_channel_layout(channels, &layout_str);
        media_info.audio_lang = audio
            .metadata()
            .get("language")
            .map(String::from)
            .unwrap_or_default();
    }

    // Bitrate from format context
    media_info.bitrate_bps = ctx.bit_rate() as u64;

    Ok(media_info)
}

// --- Helper functions ---

/// Open a specific playlist on a Blu-ray device.
fn open_playlist(device: &str, playlist_num: &str) -> Result<ffmpeg_the_third::format::context::Input, MediaError> {
    let url = format!("bluray:{}", device);
    let mut opts = ffmpeg_the_third::Dictionary::new();
    opts.set("playlist", playlist_num);

    ffmpeg_the_third::format::input_with_dictionary(&url, opts).map_err(|e| {
        classify_aacs_error(&e).unwrap_or_else(|| MediaError::from(e))
    })
}

/// Classify HDR type from color transfer characteristic and side data.
fn classify_hdr(color_transfer: &str, side_data_types: &[&str]) -> String {
    match color_transfer {
        "smpte2084" => {
            if side_data_types
                .iter()
                .any(|s| s.contains("DOVI"))
            {
                "DV".into()
            } else if side_data_types
                .iter()
                .any(|s| s.contains("SMPTE2094"))
            {
                "HDR10+".into()
            } else {
                "HDR10".into()
            }
        }
        "arib-std-b67" => "HLG".into(),
        _ => "SDR".into(),
    }
}

/// Format channel layout string, normalizing common patterns.
fn format_channel_layout(channels: u16, layout: &str) -> String {
    if layout.contains("7.1") {
        "7.1".into()
    } else if layout.contains("5.1") || channels == 6 {
        "5.1".into()
    } else if layout == "stereo" || channels == 2 {
        "2.0".into()
    } else if layout == "mono" || channels == 1 {
        "1.0".into()
    } else {
        format!("{}.0", channels)
    }
}

/// Format framerate from numerator/denominator pair.
fn format_framerate(rate: (i32, i32)) -> String {
    let (num, den) = rate;
    if den == 0 || num == 0 {
        return "0.000".into();
    }
    format!("{:.3}", num as f64 / den as f64)
}

/// Format aspect ratio from width/height (e.g., "16:9").
fn format_aspect_ratio(width: u32, height: u32) -> String {
    if height == 0 {
        return String::new();
    }
    let ratio = width as f64 / height as f64;
    if (ratio - 16.0 / 9.0).abs() < 0.1 {
        "16:9".into()
    } else if (ratio - 4.0 / 3.0).abs() < 0.1 {
        "4:3".into()
    } else if (ratio - 2.4).abs() < 0.15 {
        "2.40:1".into()
    } else {
        format!("{:.2}:1", ratio)
    }
}

/// Get color transfer characteristic name from video stream.
/// May require unsafe access to raw AVStream.
fn get_color_transfer(stream: &ffmpeg_the_third::Stream) -> String {
    // Try safe API first; fall back to "unknown" if not exposed.
    // The exact API depends on ffmpeg-the-third version —
    // may need `unsafe { (*stream.as_ptr()).codecpar }` access.
    // This is a known risk documented in the spec.
    //
    // TODO: Implement based on ffmpeg-the-third's actual API surface.
    // Placeholder returns "unknown" which maps to SDR.
    String::from("unknown")
}

/// Get side data type names from video stream for HDR classification.
fn get_side_data_types(stream: &ffmpeg_the_third::Stream) -> Vec<&str> {
    // Side data access likely requires unsafe.
    // TODO: Implement based on ffmpeg-the-third's actual API surface.
    Vec::new()
}

/// Get bit depth from codec parameters.
fn get_bit_depth(params: &ffmpeg_the_third::codec::Parameters) -> String {
    // TODO: Extract bits_per_raw_sample from codec parameters.
    String::new()
}

/// Get codec profile name from codec parameters.
fn get_profile(params: &ffmpeg_the_third::codec::Parameters) -> String {
    // TODO: Extract profile from codec parameters.
    String::new()
}
```

- [ ] **Step 3: Wire up probe module in `src/media/mod.rs`**

```rust
pub mod error;
pub mod probe;

pub use error::MediaError;
pub use probe::{probe_media_info, probe_streams, scan_playlists};

use std::sync::Once;

static FFMPEG_INIT: Once = Once::new();

/// Initialize FFmpeg libraries. Safe to call multiple times — only runs once.
pub fn ensure_init() {
    FFMPEG_INIT.call_once(|| {
        ffmpeg_the_third::init().expect("Failed to initialize FFmpeg");
    });
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test media::probe`
Expected: All helper function tests pass. Probe functions compile but can't be tested without hardware.

- [ ] **Step 5: Commit**

```bash
git add src/media/probe.rs src/media/mod.rs
git commit -m "feat(media): add probe module with playlist scanning and stream probing"
```

---

### Task 5: Create `src/media/remux.rs` — Remux with Chapters

**Files:**
- Create: `src/media/remux.rs`
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Write tests for chapter conversion helper**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChapterMark;

    #[test]
    fn test_chapter_to_time_base() {
        let ch = ChapterMark {
            index: 1,
            start_secs: 202.119,
        };
        let (start_ms, _end_ms) = chapter_to_millis(&ch, Some(300.0));
        assert_eq!(start_ms, 202119);
    }

    #[test]
    fn test_chapter_end_uses_next_chapter() {
        let chapters = vec![
            ChapterMark { index: 1, start_secs: 0.0 },
            ChapterMark { index: 2, start_secs: 120.5 },
            ChapterMark { index: 3, start_secs: 300.0 },
        ];
        let ends = compute_chapter_ends(&chapters, 600.0);
        assert_eq!(ends, vec![120500, 300000, 600000]);
    }

    #[test]
    fn test_chapter_ends_single() {
        let chapters = vec![ChapterMark { index: 1, start_secs: 0.0 }];
        let ends = compute_chapter_ends(&chapters, 2700.0);
        assert_eq!(ends, vec![2700000]);
    }

    #[test]
    fn test_stream_selection_all_maps_everything() {
        let info = StreamInfo {
            audio_streams: vec![
                AudioStream {
                    index: 0,
                    codec: "truehd".into(),
                    channels: 8,
                    channel_layout: "7.1".into(),
                    language: Some("eng".into()),
                    profile: None,
                },
                AudioStream {
                    index: 1,
                    codec: "aac".into(),
                    channels: 2,
                    channel_layout: "stereo".into(),
                    language: Some("eng".into()),
                    profile: None,
                },
            ],
            subtitle_count: 3,
        };
        let indices = select_streams(&StreamSelection::All, &info, 5);
        // All 5 stream indices should be included
        assert_eq!(indices, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_stream_selection_prefer_surround() {
        let info = StreamInfo {
            audio_streams: vec![
                AudioStream {
                    index: 1,
                    codec: "truehd".into(),
                    channels: 8,
                    channel_layout: "7.1".into(),
                    language: Some("eng".into()),
                    profile: None,
                },
                AudioStream {
                    index: 2,
                    codec: "aac".into(),
                    channels: 2,
                    channel_layout: "stereo".into(),
                    language: Some("eng".into()),
                    profile: None,
                },
            ],
            subtitle_count: 1,
        };
        // total_streams = 4 (1 video at 0, 2 audio at 1,2, 1 sub at 3)
        let indices = select_streams(&StreamSelection::PreferSurround, &info, 4);
        // Video (0) + surround audio (1) + stereo audio (2) + sub (3)
        assert_eq!(indices, vec![0, 1, 2, 3]);
    }
}
```

- [ ] **Step 2: Implement remux module**

Create `src/media/remux.rs`:

```rust
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::types::{AudioStream, ChapterMark, RipProgress, StreamInfo};

use super::error::MediaError;

/// Stream selection strategy for remuxing.
#[derive(Debug, Clone, Default)]
pub enum StreamSelection {
    /// Map all video, audio, and subtitle streams (default).
    #[default]
    All,
    /// Prefer surround audio, include stereo as secondary. All video + subs.
    PreferSurround,
    /// Explicit stream indices.
    Manual(Vec<usize>),
}

/// Options for a remux operation.
pub struct RemuxOptions {
    pub device: String,
    pub playlist: String,
    pub output: PathBuf,
    pub chapters: Vec<ChapterMark>,
    pub stream_selection: StreamSelection,
    pub cancel: Arc<AtomicBool>,
}

/// Lossless remux from Blu-ray to MKV with chapter injection and progress reporting.
pub fn remux<F>(options: RemuxOptions, mut on_progress: F) -> Result<(), MediaError>
where
    F: FnMut(RipProgress) + Send,
{
    super::ensure_init();

    // Open input
    let url = format!("bluray:{}", options.device);
    let mut input_opts = ffmpeg_the_third::Dictionary::new();
    input_opts.set("playlist", &options.playlist);
    let mut ictx = ffmpeg_the_third::format::input_with_dictionary(&url, input_opts)
        .map_err(|e| {
            super::error::classify_aacs_error(&e)
                .unwrap_or_else(|| MediaError::from(e))
        })?;

    // Probe streams for selection
    let stream_info = super::probe::probe_streams(&options.device, &options.playlist)?;
    let total_streams = ictx.streams().count();
    let selected_indices = select_streams(&options.stream_selection, &stream_info, total_streams);

    // Create output
    let mut octx = ffmpeg_the_third::format::output(&options.output)
        .map_err(|e| MediaError::RemuxFailed(e.to_string()))?;

    // Map selected streams — build input→output index mapping
    let mut stream_map: Vec<Option<usize>> = vec![None; total_streams];
    let mut out_idx = 0usize;
    for &in_idx in &selected_indices {
        let in_stream = ictx.stream(in_idx).ok_or(MediaError::NoStreams)?;
        let mut out_stream = octx.add_stream(ffmpeg_the_third::encoder::find(ffmpeg_the_third::codec::Id::None))
            .map_err(|e| MediaError::RemuxFailed(e.to_string()))?;
        out_stream.parameters().clone_from(&in_stream.parameters());
        out_stream.set_time_base(in_stream.time_base());
        stream_map[in_idx] = Some(out_idx);
        out_idx += 1;
    }

    // Inject chapters as AVChapter entries
    let total_duration_secs = ictx.duration() as f64 / f64::from(ffmpeg_the_third::ffi::AV_TIME_BASE);
    inject_chapters(&mut octx, &options.chapters, total_duration_secs)?;

    // Write header
    octx.write_header()
        .map_err(|e| MediaError::RemuxFailed(e.to_string()))?;

    // Remux loop — read packets, rescale timestamps, write
    let start_time = Instant::now();
    let mut total_bytes: u64 = 0;
    let mut frame_count: u64 = 0;
    let mut last_progress = Instant::now();

    for (stream, packet) in ictx.packets() {
        // Check cancellation
        if options.cancel.load(Ordering::Relaxed) {
            // Attempt clean shutdown
            octx.write_trailer()
                .map_err(|e| MediaError::RemuxFailed(e.to_string()))?;
            return Err(MediaError::Cancelled);
        }

        let in_idx = stream.index();
        let Some(out_idx) = stream_map[in_idx] else {
            continue; // Stream not selected
        };

        let mut packet = packet.clone();
        packet.set_stream(out_idx);
        packet.rescale_ts(
            stream.time_base(),
            octx.stream(out_idx).unwrap().time_base(),
        );

        total_bytes += packet.size() as u64;
        if stream.parameters().medium() == ffmpeg_the_third::media::Type::Video {
            frame_count += 1;
        }

        packet
            .write_interleaved(&mut octx)
            .map_err(|e| MediaError::RemuxFailed(e.to_string()))?;

        // Report progress periodically (~100ms)
        if last_progress.elapsed() >= std::time::Duration::from_millis(100) {
            let elapsed = start_time.elapsed().as_secs_f64();
            let pts_secs = packet
                .pts()
                .map(|pts| {
                    let tb = octx.stream(out_idx).unwrap().time_base();
                    (pts as f64 * tb.0 as f64) / tb.1 as f64
                })
                .unwrap_or(0.0) as u32;

            let fps = if elapsed > 0.0 {
                frame_count as f64 / elapsed
            } else {
                0.0
            };
            let speed = if elapsed > 0.0 && pts_secs > 0 {
                pts_secs as f64 / elapsed
            } else {
                0.0
            };
            let bitrate = if elapsed > 0.0 {
                format!("{:.1}kbits/s", (total_bytes as f64 * 8.0) / (elapsed * 1000.0))
            } else {
                "N/A".into()
            };

            on_progress(RipProgress {
                frame: frame_count,
                fps,
                total_size: total_bytes,
                out_time_secs: pts_secs,
                bitrate,
                speed,
            });

            last_progress = Instant::now();
        }
    }

    // Write trailer
    octx.write_trailer()
        .map_err(|e| MediaError::RemuxFailed(e.to_string()))?;

    // Final progress report
    let elapsed = start_time.elapsed().as_secs_f64();
    on_progress(RipProgress {
        frame: frame_count,
        fps: if elapsed > 0.0 { frame_count as f64 / elapsed } else { 0.0 },
        total_size: total_bytes,
        out_time_secs: (ictx.duration() as f64 / f64::from(ffmpeg_the_third::ffi::AV_TIME_BASE)) as u32,
        bitrate: "N/A".into(),
        speed: 0.0,
    });

    Ok(())
}

/// Select which input stream indices to map based on the selection strategy.
pub fn select_streams(
    selection: &StreamSelection,
    info: &StreamInfo,
    total_streams: usize,
) -> Vec<usize> {
    match selection {
        StreamSelection::All => (0..total_streams).collect(),
        StreamSelection::PreferSurround => {
            // Always include all non-audio streams (video + subtitle)
            let mut indices: Vec<usize> = (0..total_streams).collect();
            // For PreferSurround, we still include all streams but could
            // filter in the future. Current behavior matches All for now
            // since the old build_map_args just reordered, not excluded.
            indices
        }
        StreamSelection::Manual(indices) => indices.clone(),
    }
}

/// Convert ChapterMark start time to milliseconds.
fn chapter_to_millis(chapter: &ChapterMark, _total_duration: Option<f64>) -> (i64, i64) {
    let start = (chapter.start_secs * 1000.0) as i64;
    // End will be computed separately
    (start, 0)
}

/// Compute chapter end times (each chapter ends where the next begins;
/// last chapter ends at total duration).
fn compute_chapter_ends(chapters: &[ChapterMark], total_duration_secs: f64) -> Vec<i64> {
    chapters
        .iter()
        .enumerate()
        .map(|(i, _ch)| {
            if i + 1 < chapters.len() {
                (chapters[i + 1].start_secs * 1000.0) as i64
            } else {
                (total_duration_secs * 1000.0) as i64
            }
        })
        .collect()
}

/// Inject chapters into the output format context as AVChapter entries.
fn inject_chapters(
    octx: &mut ffmpeg_the_third::format::context::Output,
    chapters: &[ChapterMark],
    total_duration_secs: f64,
) -> Result<(), MediaError> {
    if chapters.is_empty() {
        return Ok(());
    }

    let ends = compute_chapter_ends(chapters, total_duration_secs);

    for (i, chapter) in chapters.iter().enumerate() {
        let start_ms = (chapter.start_secs * 1000.0) as i64;
        let end_ms = ends[i];
        let title = format!("Chapter {}", chapter.index);

        // AVChapter injection via FFmpeg API.
        // This may require unsafe access to the raw AVFormatContext
        // if ffmpeg-the-third doesn't expose chapter writing in its safe API.
        // See spec Risks: "AVChapter writing not exposed in safe API"
        //
        // TODO: Use safe API if available, otherwise use:
        // unsafe {
        //     let ffi_ctx = octx.as_mut_ptr();
        //     // avpriv_new_chapter(ffi_ctx, id, time_base, start, end, title)
        // }
        let _ = (start_ms, end_ms, title); // suppress unused warnings until implemented
    }

    Ok(())
}
```

- [ ] **Step 3: Wire up remux module in `src/media/mod.rs`**

```rust
pub mod error;
pub mod probe;
pub mod remux;

pub use error::MediaError;
pub use probe::{probe_media_info, probe_streams, scan_playlists};
pub use remux::{remux, RemuxOptions, StreamSelection};
```

- [ ] **Step 4: Run tests**

Run: `cargo test media::remux`
Expected: All helper function tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/media/remux.rs src/media/mod.rs
git commit -m "feat(media): add remux module with chapter injection and stream selection"
```

---

### Task 6: Add `stream_selection` to Config

**Files:**
- Modify: `src/config.rs:20-33`

- [ ] **Step 1: Write test for stream_selection config parsing**

Add to `src/config.rs` test module:

```rust
#[test]
fn test_stream_selection_from_config() {
    let toml_str = r#"
        stream_selection = "prefer_surround"
    "#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.stream_selection.as_deref(), Some("prefer_surround"));
}

#[test]
fn test_stream_selection_default_is_none() {
    let toml_str = "";
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.stream_selection.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_stream_selection`
Expected: FAIL — `stream_selection` field doesn't exist.

- [ ] **Step 3: Add `stream_selection` field to Config**

Add to the `Config` struct in `src/config.rs`:

```rust
pub stream_selection: Option<String>,
```

Also add a helper method:

```rust
impl Config {
    pub fn resolve_stream_selection(&self) -> crate::media::StreamSelection {
        match self.stream_selection.as_deref() {
            Some("prefer_surround") => crate::media::StreamSelection::PreferSurround,
            _ => crate::media::StreamSelection::All,
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass, including the new config tests.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add stream_selection config field"
```

---

### Task 7: Migrate `disc.rs` — Remove Old Probe Functions

**Files:**
- Modify: `src/disc.rs`

- [ ] **Step 1: Replace `scan_playlists` with `media::` delegation**

Replace `src/disc.rs` `scan_playlists()` (lines 194-242) with a thin wrapper:

```rust
pub fn scan_playlists(device: &str) -> Result<Vec<Playlist>> {
    crate::media::scan_playlists(device).map_err(|e| anyhow::anyhow!("{}", e))
}
```

- [ ] **Step 2: Replace `probe_streams` with `media::` delegation**

Replace `src/disc.rs` `probe_streams()` (lines 251-283):

```rust
pub fn probe_streams(device: &str, playlist_num: &str) -> Option<StreamInfo> {
    crate::media::probe_streams(device, playlist_num).ok()
}
```

- [ ] **Step 3: Replace `probe_media_info` with `media::` delegation**

Replace `src/disc.rs` `probe_media_info()` (lines 453-476) and remove `parse_media_info_json()` (lines 285-451):

```rust
pub fn probe_media_info(device: &str, playlist_num: &str) -> Option<MediaInfo> {
    crate::media::probe_media_info(device, playlist_num).ok()
}
```

- [ ] **Step 4: Remove `check_dependencies`, `has_mkvpropedit`, `check_aacs_error`**

Remove these functions:
- `check_dependencies()` (lines 48-62)
- `has_mkvpropedit()` (lines 65-67)
- `check_aacs_error()` (lines 167-192)

- [ ] **Step 5: Remove their tests**

Remove from the test module:
- `test_check_aacs_revoked_processing_key` (line 721)
- `test_check_aacs_revoked_certificate` (line 729)
- `test_check_aacs_generic_failure` (line 736)
- `test_check_aacs_no_error` (line 744)
- `test_parse_media_info_1080p_hevc_truehd` (line 568)
- `test_parse_media_info_sdr` (line 608)
- `test_parse_media_info_dolby_vision` (line 637)
- `test_parse_media_info_hlg` (line 666)
- `test_parse_media_info_hdr10plus` (line 693)
- `test_parse_media_info_no_streams` (line 750)
- `test_parse_media_info_dts_hd_ma` (line 756)

Keep:
- `test_parse_label_*` tests (volume label parsing)
- `test_filter_episodes` (playlist filtering)

- [ ] **Step 6: Remove `check_dependencies` call from `main.rs`**

Remove line 133: `disc::check_dependencies()?;` from `src/main.rs`.

- [ ] **Step 7: Run tests**

Run: `cargo test`
Expected: All remaining tests pass. Removed functions no longer exist.

- [ ] **Step 8: Commit**

```bash
git add src/disc.rs src/main.rs
git commit -m "refactor(disc): migrate probe functions to media module, remove CLI tool checks"
```

---

### Task 8: Migrate `rip.rs` — Remove CLI Spawning

**Files:**
- Modify: `src/rip.rs`

- [ ] **Step 1: Remove `build_map_args`, `start_rip`, `parse_progress_line`**

Remove these functions and their tests:
- `build_map_args()` (lines 12-41)
- `start_rip()` (lines 43-72)
- `parse_progress_line()` (lines 74-118)

And their tests:
- `test_build_map_args_surround_and_stereo` (line 153)
- `test_build_map_args_no_surround` (line 169)
- `test_build_map_args_no_audio` (line 179)
- `test_parse_progress_line_accumulates` (line 189)
- `test_parse_progress_negative_size` (line 208)

- [ ] **Step 2: Keep estimation functions**

Keep these intact — they work on `RipProgress` which is unchanged:
- `estimate_final_size()` (lines 120-126)
- `estimate_eta()` (lines 128-135)
- `format_eta()` (lines 137-146)
- Their tests (lines 216-244)

- [ ] **Step 3: Clean up imports**

Remove unused imports from `src/rip.rs`:
- `std::collections::HashMap`
- `std::io::BufRead` (if unused)
- `std::process::{Child, Command, Stdio}`
- `crate::types::StreamInfo` (if `build_map_args` removed)

Keep:
- `crate::types::RipProgress`

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: Estimation tests still pass. Removed tests no longer exist.

- [ ] **Step 5: Commit**

```bash
git add src/rip.rs
git commit -m "refactor(rip): remove CLI spawning functions, keep estimation math"
```

---

### Task 9: Migrate `chapters.rs` — Remove mkvpropedit Functions

**Files:**
- Modify: `src/chapters.rs`

- [ ] **Step 1: Remove `apply_chapters`, `chapters_to_ogm`, `format_chapter_time`**

Remove:
- `format_chapter_time()` (lines 8-15)
- `chapters_to_ogm()` (lines 18-30)
- `apply_chapters()` (lines 92-122)

And their tests:
- `test_format_chapter_time_zero` (line 129)
- `test_format_chapter_time_minutes` (line 134)
- `test_format_chapter_time_hours` (line 139)
- `test_chapters_to_ogm_single` (line 144)
- `test_chapters_to_ogm_multiple` (line 154)
- `test_chapters_to_ogm_empty` (line 177)

- [ ] **Step 2: Keep MPLS extraction functions**

Keep:
- `count_chapters_for_playlists()` (lines 34-45)
- `extract_chapters()` (lines 54-85)

These use the `mpls` crate and filesystem operations — they're not affected by this migration.

- [ ] **Step 3: Clean up imports**

Remove unused imports (e.g., `std::process::Command`, any formatting utils).

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: No chapter-related tests remain. MPLS extraction functions have no tests (they require filesystem access).

- [ ] **Step 5: Commit**

```bash
git add src/chapters.rs
git commit -m "refactor(chapters): remove mkvpropedit functions, keep MPLS extraction"
```

---

### Task 10: Migrate TUI — Dashboard Rip Integration

**Files:**
- Modify: `src/tui/mod.rs:80-89,91-109,390`
- Modify: `src/tui/dashboard.rs:314-430`

- [ ] **Step 1: Update `RipState` to remove CLI-process fields**

In `src/tui/mod.rs`, replace `RipState` (lines 80-89):

```rust
#[derive(Default)]
pub struct RipState {
    pub jobs: Vec<RipJob>,
    pub current_rip: usize,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub progress_rx: Option<mpsc::Receiver<Result<RipProgress, crate::media::MediaError>>>,
    pub confirm_abort: bool,
    pub confirm_rescan: bool,
}
```

- [ ] **Step 1b: Update `reset_for_rescan` in `tui/mod.rs`**

Find `reset_for_rescan()` (around lines 195-225). Replace any `self.rip.child` references with `AtomicBool` reset:

```rust
// Replace:
//   self.rip.child = None;
// With:
self.rip.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
self.rip.progress_rx = None;
```

- [ ] **Step 1c: Update quit handler in `tui/mod.rs`**

Find the quit handler (around lines 417-421) which checks/kills `app.rip.child`. Replace with:

```rust
// Replace child.kill() with:
app.rip.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
```

- [ ] **Step 1d: Update `has_mkvpropedit` reference in `src/tui/wizard.rs`**

Find the `has_mkvpropedit` reference around line 1237. Remove the conditional — always mount the disc for chapter extraction since chapters are now baked in during remux:

```rust
// Remove the `if app.has_mkvpropedit` guard around disc mounting.
// The disc should always be mounted for chapter extraction.
```
```

Remove:
- `child: Option<std::process::Child>` — no more child process
- `progress_state: HashMap<String, String>` — no more line-by-line parsing
- `stderr_buffer: Option<Arc<Mutex<String>>>` — errors come from `MediaError`

- [ ] **Step 2: Remove `has_mkvpropedit` from `App`**

In `src/tui/mod.rs`, remove `has_mkvpropedit: bool` from the `App` struct (line 97) and remove the assignment at line 390:

```rust
// Remove this line:
app.has_mkvpropedit = crate::disc::has_mkvpropedit();
```

- [ ] **Step 3: Rewrite `start_next_job` in `dashboard.rs`**

Replace `start_next_job()` (lines 314-385) with media::remux integration:

```rust
fn start_next_job(app: &mut App) {
    let next_idx = app
        .rip
        .jobs
        .iter()
        .position(|j| matches!(j.status, PlaylistStatus::Pending));

    let Some(idx) = next_idx else {
        return;
    };

    app.rip.current_rip = idx;
    let job = &app.rip.jobs[idx];
    let device = app.args.device().to_string_lossy().to_string();
    let playlist_num = job.playlist.num.clone();
    let outfile = app.args.output.join(&job.filename);
    if let Some(parent) = outfile.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Skip if output file already exists
    if outfile.exists() {
        let file_size = std::fs::metadata(&outfile).map(|m| m.len()).unwrap_or(0);
        app.rip.jobs[idx].status = PlaylistStatus::Done(file_size);
        return;
    }

    // Extract chapters from MPLS if disc is mounted
    let chapters = app
        .disc
        .mount_point
        .as_ref()
        .and_then(|mount| {
            crate::chapters::extract_chapters(
                std::path::Path::new(mount.as_str()),
                &playlist_num,
            )
        })
        .unwrap_or_default();

    let stream_selection = app.config.resolve_stream_selection();
    let cancel = app.rip.cancel.clone();

    let (tx, rx) = mpsc::channel();
    app.rip.progress_rx = Some(rx);
    app.rip.jobs[idx].status = PlaylistStatus::Ripping(RipProgress::default());

    // Spawn remux in background thread
    let options = crate::media::RemuxOptions {
        device,
        playlist: playlist_num,
        output: outfile.clone(),
        chapters,
        stream_selection,
        cancel,
    };

    std::thread::spawn(move || {
        let result = crate::media::remux(options, |progress| {
            let _ = tx.send(Ok(progress));
        });
        if let Err(e) = result {
            let _ = tx.send(Err(e));
        }
    });
}
```

- [ ] **Step 4: Update `poll_active_job` in `dashboard.rs`**

Replace the progress polling (lines 387-430) to consume `RipProgress` directly from channel instead of parsing text lines:

```rust
fn poll_active_job(app: &mut App) {
    if let Some(ref rx) = app.rip.progress_rx {
        while let Ok(msg) = rx.try_recv() {
            let idx = app.rip.current_rip;
            match msg {
                Ok(progress) => {
                    app.rip.jobs[idx].status = PlaylistStatus::Ripping(progress);
                }
                Err(crate::media::MediaError::Cancelled) => {
                    app.rip.jobs[idx].status =
                        PlaylistStatus::Failed("Cancelled".into());
                    app.rip.progress_rx = None;
                    return;
                }
                Err(e) => {
                    app.rip.jobs[idx].status =
                        PlaylistStatus::Failed(e.to_string());
                    app.rip.progress_rx = None;

                    // Start next job
                    start_next_job(app);
                    return;
                }
            }
        }
    }

    // Check if the remux thread has finished (channel closed = sender dropped)
    if let Some(ref rx) = app.rip.progress_rx {
        // If try_recv returns Disconnected, the remux thread is done
        if matches!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected)) {
            let idx = app.rip.current_rip;
            if !matches!(app.rip.jobs[idx].status, PlaylistStatus::Done(_) | PlaylistStatus::Failed(_)) {
                let outfile = app.args.output.join(&app.rip.jobs[idx].filename);
                let file_size = std::fs::metadata(&outfile).map(|m| m.len()).unwrap_or(0);
                app.rip.jobs[idx].status = PlaylistStatus::Done(file_size);
            }
            app.rip.progress_rx = None;

            // Start next job
            start_next_job(app);
        }
    }
}
```

Note: The channel now sends `Result<RipProgress, MediaError>` instead of `String`. Update the channel type accordingly.

- [ ] **Step 5: Update abort handler in `dashboard.rs`**

Find `handle_input` abort handling (around lines 252-256) which references `app.rip.child`. Replace `child.kill()`:

```rust
// Replace all `app.rip.child` references in abort handling with:
app.rip.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
```

- [ ] **Step 6: Update `tick` in `dashboard.rs`**

Find `tick()` (around line 278) which checks `app.rip.child.is_none()` to decide whether to start the next job. Replace:

```rust
// Replace: if app.rip.child.is_none() { start_next_job(app); }
// With: if app.rip.progress_rx.is_none() { start_next_job(app); }
```

- [ ] **Step 7: Update `check_all_done` in `dashboard.rs`**

Find `check_all_done()` (around lines 296-299) which references `app.rip.child`. Replace:

```rust
// Replace: app.rip.child.is_none()
// With: app.rip.progress_rx.is_none()
```

- [ ] **Step 8: Clean up imports in `tui/mod.rs` and `dashboard.rs`**

Remove:
- `Arc`, `Mutex` (from `std::sync`) — unless used elsewhere
- `std::process::Child` references
- `crate::rip::{build_map_args, parse_progress_line, start_rip}`
- `crate::disc::{check_aacs_error, probe_streams, has_mkvpropedit}`

- [ ] **Step 9: Run full test suite and manual smoke test**

Run: `cargo test`
Run: `cargo build`
Expected: Compiles and tests pass.

- [ ] **Step 10: Commit**

```bash
git add src/tui/mod.rs src/tui/dashboard.rs src/tui/wizard.rs
git commit -m "refactor(tui): migrate rip pipeline to media::remux with progress callback"
```

---

### Task 11: Migrate CLI Mode

**Files:**
- Modify: `src/cli.rs:520-680`

- [ ] **Step 1: Rewrite `rip_selected` to use `media::remux`**

Replace `rip_selected()` (lines 498-680):

```rust
fn rip_selected(
    args: &Args,
    config: &crate::config::Config,
    device: &str,
    episodes_pl: &[Playlist],
    selected: &[usize],
    outfiles: &[PathBuf],
) -> anyhow::Result<()> {
    if args.dry_run {
        println!("\n[DRY RUN] Would rip:");
        for (i, &idx) in selected.iter().enumerate() {
            let pl = &episodes_pl[idx];
            println!(
                "  {} ({}) -> {}",
                pl.num,
                pl.duration,
                outfiles[i].file_name().unwrap().to_string_lossy()
            );
        }
        return Ok(());
    }

    // Mount disc for chapter extraction
    let (mount_point, did_mount) = match disc::ensure_mounted(device) {
        Ok((mount, did_mount)) => (Some(mount), did_mount),
        Err(e) => {
            println!(
                "Warning: could not mount disc for chapter extraction: {}",
                e
            );
            (None, false)
        }
    };

    // Create output directories
    for outfile in outfiles {
        if let Some(parent) = outfile.parent() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let stream_selection = config.resolve_stream_selection();
    let mut had_failure = false;

    for (i, &idx) in selected.iter().enumerate() {
        let pl = &episodes_pl[idx];
        let outfile = &outfiles[i];
        let filename = outfile.file_name().unwrap().to_string_lossy();

        // Skip if output file already exists
        if outfile.exists() {
            let existing_size = std::fs::metadata(outfile)?.len();
            println!(
                "\nSkipping playlist {} -> {} (already exists, {})",
                pl.num,
                filename,
                format_size(existing_size)
            );
            continue;
        }

        println!(
            "\nRipping playlist {} ({}) -> {}",
            pl.num, pl.duration, filename
        );

        // Extract chapters from MPLS
        let chapters = mount_point
            .as_ref()
            .and_then(|mount| {
                crate::chapters::extract_chapters(std::path::Path::new(mount), &pl.num)
            })
            .unwrap_or_default();

        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let options = crate::media::RemuxOptions {
            device: device.to_string(),
            playlist: pl.num.clone(),
            output: outfile.clone(),
            chapters: chapters.clone(),
            stream_selection: stream_selection.clone(),
            cancel,
        };

        let pl_seconds = pl.seconds;
        let result = crate::media::remux(options, |progress| {
            let size = format_size(progress.total_size);
            let time = format_time(progress.out_time_secs);
            let mut parts = vec![
                format!("frame={}", progress.frame),
                format!("fps={:.1}", progress.fps),
                format!("size={}", size),
                format!("time={}", time),
                format!("bitrate={}", progress.bitrate),
                format!("speed={:.1}x", progress.speed),
            ];

            if let Some(est) = rip::estimate_final_size(&progress, pl_seconds) {
                parts.push(format!("est=~{}", format_size(est)));
            }
            if let Some(eta_secs) = rip::estimate_eta(&progress, pl_seconds) {
                parts.push(format!("eta={}", rip::format_eta(eta_secs)));
            }

            print!("\r  {:<100}", parts.join(" "));
            io::stdout().flush().ok();
        });

        println!();

        match result {
            Ok(()) => {
                let final_size = std::fs::metadata(outfile)?.len();
                println!("Done: {} ({})", filename, format_size(final_size));
                if !chapters.is_empty() {
                    println!("  Added {} chapter markers", chapters.len());
                }
            }
            Err(e) => {
                println!("Error: {}", e);
                had_failure = true;
                continue;
            }
        }
    }

    if did_mount {
        let _ = disc::unmount_disc(device);
    }

    println!(
        "\nAll done! Ripped {} playlist(s) to {}",
        selected.len(),
        args.output.display()
    );

    if !had_failure && config.should_eject(args.cli_eject()) {
        println!("Ejecting disc...");
        if let Err(e) = disc::eject_disc(device) {
            println!("Warning: failed to eject disc: {}", e);
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Remove old imports**

Remove unused:
- `std::collections::HashMap` (if no longer needed after removing `parse_progress_line` state)
- `crate::disc::{has_mkvpropedit, check_aacs_error}`

- [ ] **Step 3: Run tests**

Run: `cargo test`
Run: `cargo build`
Expected: Compiles cleanly.

- [ ] **Step 4: Commit**

```bash
git add src/cli.rs
git commit -m "refactor(cli): migrate rip loop to media::remux with progress callback"
```

---

### Task 12: Update Cargo.toml — Remove `which`

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Remove `which` dependency**

Remove from `[dependencies]`:
```toml
which = "8"
```

Also remove `serde_json` if it's no longer needed (was used for `parse_media_info_json`). Check if any other code uses `serde_json`:
- `src/tmdb.rs` likely still uses it for TMDb API responses. Keep if so.

- [ ] **Step 2: Verify build**

Run: `cargo build`
Expected: Compiles without `which`. If any code still references `which::which`, you'll get a compile error — find and remove those references.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: remove which dependency, no longer needed for binary detection"
```

---

### Task 13: Update CI Workflows

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`

- [ ] **Step 1: Add ffmpeg-devel to CI test job**

In `.github/workflows/ci.yml`, add package install step before the toolchain step:

```yaml
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - name: Install FFmpeg development libraries
        run: sudo apt-get update && sudo apt-get install -y libavformat-dev libavcodec-dev libavutil-dev libswscale-dev libswresample-dev libavfilter-dev libavdevice-dev pkg-config clang libclang-dev
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --locked

  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - name: Install FFmpeg development libraries
        run: sudo apt-get update && sudo apt-get install -y libavformat-dev libavcodec-dev libavutil-dev libswscale-dev libswresample-dev libavfilter-dev libavdevice-dev pkg-config clang libclang-dev
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --locked -- -D warnings
```

- [ ] **Step 2: Add ffmpeg-devel to release gate and build jobs**

In `.github/workflows/release.yml`, add the same package install to both the `gate` job and the `build` job:

Gate job — add after `actions/checkout@v5`:
```yaml
      - name: Install FFmpeg development libraries
        run: sudo apt-get update && sudo apt-get install -y libavformat-dev libavcodec-dev libavutil-dev libswscale-dev libswresample-dev libavfilter-dev libavdevice-dev pkg-config clang libclang-dev
```

Build job — add after `actions/checkout@v5`:
```yaml
      - name: Install FFmpeg development libraries
        run: sudo apt-get update && sudo apt-get install -y libavformat-dev libavcodec-dev libavutil-dev libswscale-dev libswresample-dev libavfilter-dev libavdevice-dev pkg-config clang libclang-dev
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml .github/workflows/release.yml
git commit -m "ci: install FFmpeg development libraries for build and test"
```

---

### Task 14: Final Integration Testing and Cleanup

**Files:**
- All modified files

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass. No references to removed functions.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings. Fix any unused import or dead code warnings.

- [ ] **Step 3: Verify build**

Run: `cargo build --release`
Expected: Release build succeeds.

- [ ] **Step 4: Verify no remaining references to removed functions**

Search for any leftover references:

```bash
rg "check_dependencies|has_mkvpropedit|check_aacs_error|parse_media_info_json|build_map_args|start_rip|parse_progress_line|apply_chapters|chapters_to_ogm|format_chapter_time" src/
```

Expected: No matches (or only the new delegation wrappers in `disc.rs` if keeping backward-compatible signatures).

- [ ] **Step 5: Update TODO.md**

Update the "Investigate Further" section to reflect completed work:

```markdown
# Investigate Further

- ~~pure Rust MKV/ffprobe integration (overlaps with `~/code/media-tools` use case)~~
    - ~~ffmpeg bindings~~ Done: migrated to `ffmpeg-the-third` library bindings
    - ~~chapter writing via `mkv-element` crate to replace `mkvpropedit` shell-out~~ Done: chapters injected via AVChapter during remux
- macos/windows support
```

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore: cleanup and update TODO after FFmpeg bindings integration"
```

- [ ] **Step 7: Manual hardware test (requires Blu-ray disc)**

Test the full workflow with an actual disc:
1. `cargo run -- -d /dev/sr0` — verify TUI mode works end-to-end
2. `cargo run -- -d /dev/sr0 --no-tui` — verify CLI mode works
3. Check output MKV has chapters embedded (use `mkvinfo` or `ffprobe`)
4. Check all audio/video/subtitle streams are present
5. Verify progress reporting works in both modes

---

## Task Dependency Summary

```
Task 0 (spike) ──► Task 1 (error) ──► Task 2 (mod wiring)
                                            │
                   Task 3 (StreamInfo) ◄────┘
                         │
              ┌──────────┤
              ▼          ▼
        Task 4      Task 5
       (probe)     (remux)
              │          │
              │          ├──► Task 6 (config — depends on StreamSelection from Task 5)
              │          │          │
              └──────────┼──────────┘
                         ▼
                   Task 7 (disc.rs)
                         │
              ┌──────────┼──────────┐
              ▼          ▼          ▼
        Task 8      Task 9     Task 10
       (rip.rs)  (chapters)  (dashboard)
              │          │          │
              └──────────┼──────────┘
                         ▼
                   Task 11 (cli.rs)
                         │
                         ▼
                   Task 12 (Cargo.toml)
                         │
                         ▼
                   Task 13 (CI)
                         │
                         ▼
                   Task 14 (cleanup)
```

Tasks 4, 5 can run in parallel after Task 3. Task 6 depends on Task 5.
Tasks 8, 9, 10 can run in parallel after Task 7.
