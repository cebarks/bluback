# Per-Stream Track Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users select which video, audio, and subtitle streams to include per playlist, via config defaults, CLI flags, and an inline TUI track picker.

**Architecture:** Two-layer filtering: config/CLI provides a `StreamFilter` (language + format rules) that auto-selects streams; TUI allows per-playlist manual overrides. Both resolve to `StreamSelection::Manual(Vec<usize>)` before remux. A unified `probe_playlist()` replaces separate `probe_media_info()`/`probe_streams()` calls. `PreferSurround` variant is removed; its behavior is subsumed by `StreamFilter { prefer_surround: true }`.

**Tech Stack:** Rust, ratatui, clap, toml/serde, ffmpeg-the-third

**Spec:** `docs/superpowers/specs/2026-03-29-per-stream-track-selection-design.md`

---

### Task 1: New Stream Types

**Files:**
- Modify: `src/types.rs`

- [ ] **Step 1: Add `VideoStream` struct**

Add after the `AudioStream` impl block (~line 63):

```rust
#[derive(Debug, Clone)]
pub struct VideoStream {
    pub index: usize,
    pub codec: String,
    pub resolution: String,
    pub hdr: String,
    pub framerate: String,
    pub bit_depth: String,
}

impl VideoStream {
    pub fn display_line(&self) -> String {
        let hdr_part = if self.hdr.is_empty() || self.hdr == "SDR" {
            String::new()
        } else {
            format!("  {}", self.hdr)
        };
        format!(
            "{} {}  {}fps{}",
            self.codec.to_uppercase(),
            self.resolution,
            self.framerate,
            hdr_part
        )
    }
}
```

- [ ] **Step 2: Add `SubtitleStream` struct**

Add after `VideoStream`:

```rust
#[derive(Debug, Clone)]
pub struct SubtitleStream {
    pub index: usize,
    pub codec: String,
    pub language: Option<String>,
    pub forced: bool,
}

impl SubtitleStream {
    pub fn display_line(&self) -> String {
        let lang = self.language.as_deref().unwrap_or("und");
        let forced_tag = if self.forced { " FORCED" } else { "" };
        format!("{} ({}){}", self.codec_display_name(), lang, forced_tag)
    }

    fn codec_display_name(&self) -> &str {
        match self.codec.as_str() {
            "hdmv_pgs_subtitle" => "PGS",
            "subrip" | "srt" => "SRT",
            "dvd_subtitle" => "VobSub",
            "ass" => "ASS",
            other => other,
        }
    }
}
```

- [ ] **Step 3: Expand `StreamInfo` — add video/subtitle streams, keep `subtitle_count` temporarily**

Replace the existing `StreamInfo` struct (~lines 65-70). Keep `subtitle_count` alongside `subtitle_streams` to avoid breaking `build_stream_info()` and `probe_streams()` — those are removed in Tasks 4 and 5.

```rust
#[derive(Debug, Clone, Default)]
pub struct StreamInfo {
    pub video_streams: Vec<VideoStream>,
    pub audio_streams: Vec<AudioStream>,
    pub subtitle_streams: Vec<SubtitleStream>,
    #[deprecated(note = "Use subtitle_streams.len() instead — will be removed after probe/remux migration")]
    pub subtitle_count: u32,
}
```

- [ ] **Step 4: Suppress deprecation warnings temporarily**

Add `#[allow(deprecated)]` at usages in `remux.rs` (`build_stream_info()`) and `probe.rs` (`probe_streams()`). These functions are removed in Tasks 4 and 5, at which point the deprecated field itself is removed.

- [ ] **Step 5: Write tests for display methods**

Add to the `#[cfg(test)] mod tests` block in `types.rs`:

```rust
#[test]
fn test_video_stream_display() {
    let v = VideoStream {
        index: 0,
        codec: "hevc".into(),
        resolution: "1920x1080".into(),
        hdr: "HDR10".into(),
        framerate: "23.976".into(),
        bit_depth: "10".into(),
    };
    assert_eq!(v.display_line(), "HEVC 1920x1080  23.976fps  HDR10");

    let sdr = VideoStream {
        index: 0,
        codec: "h264".into(),
        resolution: "1920x1080".into(),
        hdr: "SDR".into(),
        framerate: "24".into(),
        bit_depth: "8".into(),
    };
    assert_eq!(sdr.display_line(), "H264 1920x1080  24fps");
}

#[test]
fn test_subtitle_stream_display() {
    let s = SubtitleStream {
        index: 3,
        codec: "hdmv_pgs_subtitle".into(),
        language: Some("eng".into()),
        forced: false,
    };
    assert_eq!(s.display_line(), "PGS (eng)");

    let forced = SubtitleStream {
        index: 4,
        codec: "hdmv_pgs_subtitle".into(),
        language: Some("eng".into()),
        forced: true,
    };
    assert_eq!(forced.display_line(), "PGS (eng) FORCED");

    let unknown_codec = SubtitleStream {
        index: 5,
        codec: "dvb_teletext".into(),
        language: None,
        forced: false,
    };
    assert_eq!(unknown_codec.display_line(), "dvb_teletext (und)");
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test --lib -- types::tests::test_video_stream types::tests::test_subtitle_stream`

Expected: PASS

- [ ] **Step 7: Commit**

```
feat: add VideoStream and SubtitleStream types, expand StreamInfo
```

---

### Task 2: StreamFilter Module

**Files:**
- Create: `src/streams.rs`
- Modify: `src/main.rs` (add `mod streams;`)

- [ ] **Step 1: Create `src/streams.rs` with `StreamFilter` struct and `apply()` tests**

```rust
use crate::types::{AudioStream, StreamInfo, SubtitleStream, VideoStream};

/// Config/CLI-derived stream filtering rules.
#[derive(Debug, Clone, Default)]
pub struct StreamFilter {
    /// Audio language filter. Empty = include all.
    pub audio_languages: Vec<String>,
    /// Subtitle language filter. Empty = include all.
    pub subtitle_languages: Vec<String>,
    /// If true and surround audio exists in filtered set,
    /// select surround + one stereo; otherwise select all matching.
    pub prefer_surround: bool,
}

impl StreamFilter {
    /// Returns true if this filter is empty (no filtering applied).
    pub fn is_empty(&self) -> bool {
        self.audio_languages.is_empty()
            && self.subtitle_languages.is_empty()
            && !self.prefer_surround
    }

    /// Apply filter to stream info, return selected absolute stream indices.
    /// Falls back to all streams of a type if no languages match.
    pub fn apply(&self, info: &StreamInfo) -> Vec<usize> {
        let mut selected = Vec::new();

        // All video streams always included
        for v in &info.video_streams {
            selected.push(v.index);
        }

        // Filter audio
        let audio_candidates = self.filter_by_language(
            &info.audio_streams,
            |s| s.language.as_deref(),
            |s| s.index,
            &self.audio_languages,
        );
        let audio_indices = if self.prefer_surround {
            self.apply_surround_preference(&info.audio_streams, &audio_candidates)
        } else {
            audio_candidates
        };
        selected.extend(&audio_indices);

        // Filter subtitles
        let sub_candidates = self.filter_by_language(
            &info.subtitle_streams,
            |s| s.language.as_deref(),
            |s| s.index,
            &self.subtitle_languages,
        );
        selected.extend(&sub_candidates);

        selected.sort_unstable();
        selected.dedup();
        selected
    }

    fn filter_by_language<T, F, G>(
        &self,
        streams: &[T],
        get_lang: F,
        get_index: G,
        languages: &[String],
    ) -> Vec<usize>
    where
        F: Fn(&T) -> Option<&str>,
        G: Fn(&T) -> usize,
    {
        if languages.is_empty() {
            return streams.iter().map(&get_index).collect();
        }

        let matching: Vec<usize> = streams
            .iter()
            .filter(|s| {
                let lang = get_lang(s);
                match lang {
                    None | Some("und") => true, // undetermined always included
                    Some(l) => languages.iter().any(|f| f.eq_ignore_ascii_case(l)),
                }
            })
            .map(&get_index)
            .collect();

        if matching.is_empty() {
            // Fallback: include all if no matches
            log::warn!(
                "No streams matched language filter {:?}, including all",
                languages
            );
            streams.iter().map(&get_index).collect()
        } else {
            matching
        }
    }

    fn apply_surround_preference(
        &self,
        audio_streams: &[AudioStream],
        candidates: &[usize],
    ) -> Vec<usize> {
        let candidate_streams: Vec<&AudioStream> = audio_streams
            .iter()
            .filter(|s| candidates.contains(&s.index))
            .collect();

        let surround = candidate_streams.iter().find(|s| s.is_surround());
        let stereo = candidate_streams.iter().find(|s| s.channels == 2);

        match surround {
            Some(s) => {
                let mut result = vec![s.index];
                if let Some(st) = stereo {
                    if st.index != s.index {
                        result.push(st.index);
                    }
                }
                result
            }
            None => candidates.to_vec(), // no surround, keep all
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stream_info() -> StreamInfo {
        StreamInfo {
            video_streams: vec![VideoStream {
                index: 0,
                codec: "hevc".into(),
                resolution: "1920x1080".into(),
                hdr: "SDR".into(),
                framerate: "23.976".into(),
                bit_depth: "8".into(),
            }],
            audio_streams: vec![
                AudioStream {
                    index: 1,
                    codec: "truehd".into(),
                    channels: 8,
                    channel_layout: "7.1".into(),
                    language: Some("eng".into()),
                    profile: Some("TrueHD".into()),
                },
                AudioStream {
                    index: 2,
                    codec: "ac3".into(),
                    channels: 6,
                    channel_layout: "5.1".into(),
                    language: Some("eng".into()),
                    profile: None,
                },
                AudioStream {
                    index: 3,
                    codec: "ac3".into(),
                    channels: 2,
                    channel_layout: "stereo".into(),
                    language: Some("eng".into()),
                    profile: None,
                },
                AudioStream {
                    index: 4,
                    codec: "ac3".into(),
                    channels: 6,
                    channel_layout: "5.1".into(),
                    language: Some("fra".into()),
                    profile: None,
                },
                AudioStream {
                    index: 5,
                    codec: "ac3".into(),
                    channels: 6,
                    channel_layout: "5.1".into(),
                    language: Some("deu".into()),
                    profile: None,
                },
            ],
            subtitle_streams: vec![
                SubtitleStream {
                    index: 6,
                    codec: "hdmv_pgs_subtitle".into(),
                    language: Some("eng".into()),
                    forced: false,
                },
                SubtitleStream {
                    index: 7,
                    codec: "hdmv_pgs_subtitle".into(),
                    language: Some("eng".into()),
                    forced: true,
                },
                SubtitleStream {
                    index: 8,
                    codec: "hdmv_pgs_subtitle".into(),
                    language: Some("fra".into()),
                    forced: false,
                },
                SubtitleStream {
                    index: 9,
                    codec: "hdmv_pgs_subtitle".into(),
                    language: Some("deu".into()),
                    forced: false,
                },
            ],
        }
    }

    #[test]
    fn test_empty_filter_includes_all() {
        let filter = StreamFilter::default();
        let info = make_stream_info();
        let selected = filter.apply(&info);
        assert_eq!(selected, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_audio_language_filter() {
        let filter = StreamFilter {
            audio_languages: vec!["eng".into()],
            ..Default::default()
        };
        let info = make_stream_info();
        let selected = filter.apply(&info);
        // video(0) + eng audio(1,2,3) + all subs(6,7,8,9)
        assert_eq!(selected, vec![0, 1, 2, 3, 6, 7, 8, 9]);
    }

    #[test]
    fn test_subtitle_language_filter() {
        let filter = StreamFilter {
            subtitle_languages: vec!["eng".into()],
            ..Default::default()
        };
        let info = make_stream_info();
        let selected = filter.apply(&info);
        // video(0) + all audio(1-5) + eng subs(6,7)
        assert_eq!(selected, vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn test_both_language_filters() {
        let filter = StreamFilter {
            audio_languages: vec!["eng".into()],
            subtitle_languages: vec!["eng".into()],
            ..Default::default()
        };
        let info = make_stream_info();
        let selected = filter.apply(&info);
        assert_eq!(selected, vec![0, 1, 2, 3, 6, 7]);
    }

    #[test]
    fn test_prefer_surround() {
        let filter = StreamFilter {
            audio_languages: vec!["eng".into()],
            prefer_surround: true,
            ..Default::default()
        };
        let info = make_stream_info();
        let selected = filter.apply(&info);
        // video(0) + surround eng(1) + stereo eng(3) + all subs(6-9)
        assert_eq!(selected, vec![0, 1, 3, 6, 7, 8, 9]);
    }

    #[test]
    fn test_no_match_fallback_includes_all_audio() {
        let filter = StreamFilter {
            audio_languages: vec!["jpn".into()],
            ..Default::default()
        };
        let info = make_stream_info();
        let selected = filter.apply(&info);
        // No jpn audio → fallback to all audio
        assert_eq!(selected, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_und_streams_always_included() {
        let mut info = make_stream_info();
        info.audio_streams.push(AudioStream {
            index: 10,
            codec: "ac3".into(),
            channels: 2,
            channel_layout: "stereo".into(),
            language: None,
            profile: None,
        });
        let filter = StreamFilter {
            audio_languages: vec!["fra".into()],
            ..Default::default()
        };
        let selected = filter.apply(&info);
        // video(0) + fra audio(4) + und audio(10) + all subs
        assert_eq!(selected, vec![0, 4, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn test_case_insensitive_match() {
        let filter = StreamFilter {
            audio_languages: vec!["ENG".into()],
            ..Default::default()
        };
        let info = make_stream_info();
        let selected = filter.apply(&info);
        assert_eq!(selected, vec![0, 1, 2, 3, 6, 7, 8, 9]);
    }

    #[test]
    fn test_is_empty() {
        assert!(StreamFilter::default().is_empty());
        assert!(!StreamFilter {
            audio_languages: vec!["eng".into()],
            ..Default::default()
        }
        .is_empty());
        assert!(!StreamFilter {
            prefer_surround: true,
            ..Default::default()
        }
        .is_empty());
    }
}
```

- [ ] **Step 2: Add `mod streams;` to `src/main.rs`**

Add near the other `mod` declarations:

```rust
mod streams;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib streams::tests`

Expected: All PASS

- [ ] **Step 4: Add `parse_track_spec()` and 0-based range parser with tests**

Add to `src/streams.rs` (below `StreamFilter` impl, above `#[cfg(test)]`):

```rust
/// Parse a 0-based range spec like "0,2-4" into indices.
/// Returns None on invalid input. max_val is exclusive upper bound.
pub fn parse_zero_based_ranges(text: &str, max_val: usize) -> Option<Vec<usize>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut indices = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let (start_s, end_s) = part.split_once('-')?;
            let start: usize = start_s.trim().parse().ok()?;
            let end: usize = end_s.trim().parse().ok()?;
            if start > end || end >= max_val {
                return None;
            }
            indices.extend(start..=end);
        } else {
            let val: usize = part.parse().ok()?;
            if val >= max_val {
                return None;
            }
            indices.push(val);
        }
    }
    if indices.is_empty() {
        None
    } else {
        Some(indices)
    }
}

/// Parse CLI --tracks spec (e.g. "v:0;a:0,2;s:0-1") into absolute stream indices.
/// Omitted types default to all of that type.
/// Returns Err if the spec is malformed.
pub fn parse_track_spec(spec: &str, info: &StreamInfo) -> anyhow::Result<Vec<usize>> {
    let mut selected = Vec::new();
    let mut saw_video = false;
    let mut saw_audio = false;
    let mut saw_sub = false;

    for part in spec.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (prefix, indices_str) = part
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("Invalid track spec '{}': expected 'v:', 'a:', or 's:' prefix", part))?;

        match prefix.trim() {
            "v" => {
                saw_video = true;
                match parse_zero_based_ranges(indices_str, info.video_streams.len()) {
                    Some(type_indices) => {
                        for i in type_indices {
                            selected.push(info.video_streams[i].index);
                        }
                    }
                    None => {
                        log::warn!(
                            "Track spec video indices '{}' out of range (max {}), including all video",
                            indices_str,
                            info.video_streams.len()
                        );
                        selected.extend(info.video_streams.iter().map(|s| s.index));
                    }
                }
            }
            "a" => {
                saw_audio = true;
                match parse_zero_based_ranges(indices_str, info.audio_streams.len()) {
                    Some(type_indices) => {
                        for i in type_indices {
                            selected.push(info.audio_streams[i].index);
                        }
                    }
                    None => {
                        log::warn!(
                            "Track spec audio indices '{}' out of range (max {}), including all audio",
                            indices_str,
                            info.audio_streams.len()
                        );
                        selected.extend(info.audio_streams.iter().map(|s| s.index));
                    }
                }
            }
            "s" => {
                saw_sub = true;
                match parse_zero_based_ranges(indices_str, info.subtitle_streams.len()) {
                    Some(type_indices) => {
                        for i in type_indices {
                            selected.push(info.subtitle_streams[i].index);
                        }
                    }
                    None => {
                        log::warn!(
                            "Track spec subtitle indices '{}' out of range (max {}), including all subtitles",
                            indices_str,
                            info.subtitle_streams.len()
                        );
                        selected.extend(info.subtitle_streams.iter().map(|s| s.index));
                    }
                }
            }
            other => {
                anyhow::bail!("Unknown track type '{}': expected 'v', 'a', or 's'", other);
            }
        }
    }

    // Omitted types default to all
    if !saw_video {
        selected.extend(info.video_streams.iter().map(|s| s.index));
    }
    if !saw_audio {
        selected.extend(info.audio_streams.iter().map(|s| s.index));
    }
    if !saw_sub {
        selected.extend(info.subtitle_streams.iter().map(|s| s.index));
    }

    selected.sort_unstable();
    selected.dedup();
    Ok(selected)
}

/// Validate that a track selection includes required streams.
/// Returns error messages for any violations.
pub fn validate_track_selection(selected: &[usize], info: &StreamInfo) -> Vec<String> {
    let mut errors = Vec::new();

    let has_video_source = !info.video_streams.is_empty();
    let has_audio_source = !info.audio_streams.is_empty();

    if has_video_source && !info.video_streams.iter().any(|v| selected.contains(&v.index)) {
        errors.push("no video streams selected".into());
    }
    if has_audio_source && !info.audio_streams.iter().any(|a| selected.contains(&a.index)) {
        errors.push("no audio streams selected".into());
    }
    if selected.is_empty() {
        errors.push("no streams selected".into());
    }

    errors
}
```

Add to `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn test_parse_zero_based_ranges() {
        assert_eq!(parse_zero_based_ranges("0", 3), Some(vec![0]));
        assert_eq!(parse_zero_based_ranges("0,2", 3), Some(vec![0, 2]));
        assert_eq!(parse_zero_based_ranges("0-2", 3), Some(vec![0, 1, 2]));
        assert_eq!(parse_zero_based_ranges("0, 2", 3), Some(vec![0, 2]));
        // Out of range
        assert_eq!(parse_zero_based_ranges("3", 3), None);
        assert_eq!(parse_zero_based_ranges("0-3", 3), None);
        // Empty
        assert_eq!(parse_zero_based_ranges("", 3), None);
    }

    #[test]
    fn test_parse_track_spec_basic() {
        let info = make_stream_info();
        // Select first audio only, all video and subs default
        let result = parse_track_spec("a:0", &info).unwrap();
        // video(0) + audio(1) + all subs(6-9)
        assert_eq!(result, vec![0, 1, 6, 7, 8, 9]);
    }

    #[test]
    fn test_parse_track_spec_multiple_types() {
        let info = make_stream_info();
        let result = parse_track_spec("a:0,1;s:0", &info).unwrap();
        // video(0) + audio(1,2) + sub(6)
        assert_eq!(result, vec![0, 1, 2, 6]);
    }

    #[test]
    fn test_parse_track_spec_all_types() {
        let info = make_stream_info();
        let result = parse_track_spec("v:0;a:0;s:0", &info).unwrap();
        assert_eq!(result, vec![0, 1, 6]);
    }

    #[test]
    fn test_parse_track_spec_invalid_type() {
        let info = make_stream_info();
        assert!(parse_track_spec("x:0", &info).is_err());
    }

    #[test]
    fn test_parse_track_spec_no_colon() {
        let info = make_stream_info();
        assert!(parse_track_spec("a0", &info).is_err());
    }

    #[test]
    fn test_validate_track_selection() {
        let info = make_stream_info();
        // Valid: has video + audio
        let errors = validate_track_selection(&[0, 1, 6], &info);
        assert!(errors.is_empty());

        // Missing audio
        let errors = validate_track_selection(&[0, 6], &info);
        assert_eq!(errors, vec!["no audio streams selected"]);

        // Missing video
        let errors = validate_track_selection(&[1, 6], &info);
        assert_eq!(errors, vec!["no video streams selected"]);

        // Empty
        let errors = validate_track_selection(&[], &info);
        assert!(errors.contains(&"no streams selected".to_string()));
    }
```

- [ ] **Step 5: Run all streams tests**

Run: `cargo test --lib streams::tests`

Expected: All PASS

- [ ] **Step 6: Commit**

```
feat: add streams module with StreamFilter, parse_track_spec, validation
```

---

### Task 3: Config — `[streams]` Section

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write tests for `[streams]` config parsing**

Add to `#[cfg(test)] mod tests` in `config.rs`:

```rust
#[test]
fn test_streams_config_parsing() {
    let toml = r#"
[streams]
audio_languages = ["eng", "jpn"]
subtitle_languages = ["eng"]
prefer_surround = true
"#;
    let config: Config = toml::from_str(toml).unwrap();
    let streams = config.streams.unwrap();
    assert_eq!(
        streams.audio_languages,
        Some(vec!["eng".to_string(), "jpn".to_string()])
    );
    assert_eq!(
        streams.subtitle_languages,
        Some(vec!["eng".to_string()])
    );
    assert_eq!(streams.prefer_surround, Some(true));
}

#[test]
fn test_streams_config_empty() {
    let toml = "";
    let config: Config = toml::from_str(toml).unwrap();
    assert!(config.streams.is_none());
}

#[test]
fn test_resolve_stream_filter_from_streams() {
    let toml = r#"
[streams]
audio_languages = ["eng"]
prefer_surround = true
"#;
    let config: Config = toml::from_str(toml).unwrap();
    let filter = config.resolve_stream_filter();
    assert_eq!(filter.audio_languages, vec!["eng"]);
    assert!(filter.prefer_surround);
    assert!(filter.subtitle_languages.is_empty());
}

#[test]
fn test_resolve_stream_filter_from_old_key() {
    let toml = r#"stream_selection = "prefer_surround""#;
    let config: Config = toml::from_str(toml).unwrap();
    let filter = config.resolve_stream_filter();
    assert!(filter.prefer_surround);
}

#[test]
fn test_resolve_stream_filter_new_overrides_old() {
    let toml = r#"
stream_selection = "prefer_surround"
[streams]
prefer_surround = false
audio_languages = ["fra"]
"#;
    let config: Config = toml::from_str(toml).unwrap();
    let filter = config.resolve_stream_filter();
    // New [streams] takes precedence
    assert!(!filter.prefer_surround);
    assert_eq!(filter.audio_languages, vec!["fra"]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::test_streams_config`

Expected: FAIL (fields don't exist yet)

- [ ] **Step 3: Add `StreamsConfig` struct and `streams` field to `Config`**

Add `StreamsConfig` struct near `MetadataConfig` in `config.rs`:

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StreamsConfig {
    pub audio_languages: Option<Vec<String>>,
    pub subtitle_languages: Option<Vec<String>>,
    pub prefer_surround: Option<bool>,
}
```

Add field to `Config` struct:

```rust
pub streams: Option<StreamsConfig>,
```

- [ ] **Step 4: Add `resolve_stream_filter()` method**

Add to `impl Config`, replacing `resolve_stream_selection()`:

```rust
pub fn resolve_stream_filter(&self) -> crate::streams::StreamFilter {
    // New [streams] section takes priority
    if let Some(ref streams) = self.streams {
        return crate::streams::StreamFilter {
            audio_languages: streams.audio_languages.clone().unwrap_or_default(),
            subtitle_languages: streams.subtitle_languages.clone().unwrap_or_default(),
            prefer_surround: streams.prefer_surround.unwrap_or(false),
        };
    }
    // Fall back to old stream_selection key
    match self.stream_selection.as_deref() {
        Some("prefer_surround") => {
            log::warn!("Config key 'stream_selection' is deprecated, use [streams] section instead");
            crate::streams::StreamFilter {
                prefer_surround: true,
                ..Default::default()
            }
        }
        _ => crate::streams::StreamFilter::default(),
    }
}
```

Keep `resolve_stream_selection()` for now — callers will be migrated in later tasks.

- [ ] **Step 5: Update `KNOWN_KEYS` — add `"streams"`, keep `"stream_selection"`**

Add `"streams"` to the `KNOWN_KEYS` array.

- [ ] **Step 6: Update `to_toml_string()` — emit `[streams]` section**

Add after the `[metadata]` section emission, before `[post_rip]`:

```rust
out.push('\n');
out.push_str("[streams]\n");
if let Some(ref streams) = self.streams {
    if let Some(ref langs) = streams.audio_languages {
        if !langs.is_empty() {
            let quoted: Vec<String> = langs.iter().map(|l| format!("{:?}", l)).collect();
            out.push_str(&format!("audio_languages = [{}]\n", quoted.join(", ")));
        } else {
            out.push_str("# audio_languages = []\n");
        }
    } else {
        out.push_str("# audio_languages = []\n");
    }
    if let Some(ref langs) = streams.subtitle_languages {
        if !langs.is_empty() {
            let quoted: Vec<String> = langs.iter().map(|l| format!("{:?}", l)).collect();
            out.push_str(&format!("subtitle_languages = [{}]\n", quoted.join(", ")));
        } else {
            out.push_str("# subtitle_languages = []\n");
        }
    } else {
        out.push_str("# subtitle_languages = []\n");
    }
    emit_bool(
        &mut out,
        "prefer_surround",
        streams.prefer_surround,
        false,
    );
} else {
    out.push_str("# audio_languages = []\n");
    out.push_str("# subtitle_languages = []\n");
    out.push_str("# prefer_surround = false\n");
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test --lib config::tests`

Expected: All PASS

- [ ] **Step 8: Commit**

```
feat: add [streams] config section with language filters and prefer_surround
```

---

### Task 4: Remux Pipeline Simplification

**Files:**
- Modify: `src/media/remux.rs`

- [ ] **Step 1: Remove `PreferSurround` variant from `StreamSelection`**

Update the enum (~line 14):

```rust
#[derive(Debug, Clone, Default)]
pub enum StreamSelection {
    #[default]
    All,
    Manual(Vec<usize>),
}
```

- [ ] **Step 2: Simplify `select_streams()` — remove `StreamInfo` parameter**

Replace the function (~line 44):

```rust
pub fn select_streams(selection: &StreamSelection, total_streams: usize) -> Vec<usize> {
    match selection {
        StreamSelection::All => (0..total_streams).collect(),
        StreamSelection::Manual(indices) => indices.clone(),
    }
}
```

- [ ] **Step 3: Remove `build_stream_info()` function**

Delete the function (~lines 430-473).

- [ ] **Step 4: Update `remux()` — remove `build_stream_info` call, update `select_streams` call**

Replace lines ~214-215:

```rust
let selected = select_streams(&options.stream_selection, nb_input_streams);
```

- [ ] **Step 5: Fix all compilation errors from `PreferSurround` removal**

Grep for `PreferSurround` across the codebase and fix each reference. Key locations:
- `src/config.rs` `resolve_stream_selection()` — this method still references `PreferSurround`. Keep it for now but change it to return `All` (the old behavior is handled by `resolve_stream_filter()` now).
- `src/media/remux.rs` tests — update any tests that use `PreferSurround`.

Update `resolve_stream_selection()`:

```rust
pub fn resolve_stream_selection(&self) -> crate::media::StreamSelection {
    // Deprecated: use resolve_stream_filter() instead
    crate::media::StreamSelection::All
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test`

Expected: All PASS (some tests may need updating for removed variants)

- [ ] **Step 7: Commit**

```
refactor: remove PreferSurround variant, simplify select_streams

PreferSurround is subsumed by StreamFilter { prefer_surround: true }.
build_stream_info() removed — stream info now probed upfront.
```

---

### Task 5: Unified Probing

**Files:**
- Modify: `src/media/probe.rs`
- Modify: `src/disc.rs`

- [ ] **Step 1: Add `probe_playlist()` function to `src/media/probe.rs`**

Add after `probe_media_info()`:

```rust
/// Probe both media info and detailed stream info from a single context open.
pub fn probe_playlist(
    device: &str,
    playlist_num: &str,
) -> Result<(MediaInfo, StreamInfo), MediaError> {
    let ctx = open_bluray(device, Some(playlist_num))?;

    let mut media_info = MediaInfo::default();
    let mut video_streams = Vec::new();
    let mut audio_streams = Vec::new();
    let mut subtitle_streams = Vec::new();
    let mut first_audio_done = false;

    for stream in ctx.streams() {
        let params = stream.parameters();
        match params.medium() {
            MediaType::Video => {
                let codec_id = params.id();
                let width = params.width();
                let height = params.height();
                let resolution = if height > 0 {
                    format!("{}x{}", width, height)
                } else {
                    String::new()
                };
                let rate = stream.rate();
                let framerate = format_framerate((rate.numerator(), rate.denominator()));

                let bits_raw = params.bits_per_raw_sample();
                let bit_depth = if bits_raw > 0 {
                    bits_raw.to_string()
                } else {
                    let bits_coded = params.bits_per_coded_sample();
                    if bits_coded > 0 {
                        bits_coded.to_string()
                    } else {
                        String::new() // preserve backwards compat with MediaInfo
                    }
                };

                let profile_raw = params.profile();
                let profile = Profile::from((codec_id, profile_raw));
                let color_trc = params.color_transfer_characteristic();
                let color_transfer_str = color_trc.name().unwrap_or("").to_string();
                let side_data_types = extract_side_data_types(&stream);
                let side_data_refs: Vec<&str> =
                    side_data_types.iter().map(|s| s.as_str()).collect();
                let hdr = classify_hdr(&color_transfer_str, &side_data_refs);

                // First video populates MediaInfo
                if media_info.codec.is_empty() {
                    media_info.codec = codec_id.name().to_string();
                    media_info.width = width;
                    media_info.height = height;
                    media_info.resolution = if height > 0 {
                        format!("{}p", height)
                    } else {
                        String::new()
                    };
                    media_info.aspect_ratio = format_aspect_ratio(width, height);
                    media_info.framerate = framerate.clone();
                    media_info.bit_depth = bit_depth.clone();
                    media_info.profile = format_video_profile(profile);
                    media_info.hdr = hdr.clone();
                }

                video_streams.push(crate::types::VideoStream {
                    index: stream.index(),
                    codec: codec_id.name().to_string(),
                    resolution,
                    hdr,
                    framerate,
                    bit_depth,
                });
            }
            MediaType::Audio => {
                let codec_id = params.id();
                let codec_name = codec_id.name().to_string();
                let ch_layout = params.ch_layout();
                let channels = ch_layout.channels() as u16;
                let layout_desc = ch_layout.description();
                let channel_layout = format_channel_layout(channels, &layout_desc);
                let language = stream.metadata().get("language").map(|s| s.to_string());
                let profile_raw = params.profile();
                let profile = format_codec_profile(Profile::from((codec_id, profile_raw)));

                // First audio populates MediaInfo
                if !first_audio_done {
                    first_audio_done = true;
                    let prof = Profile::from((codec_id, profile_raw));
                    media_info.audio = match &prof {
                        Profile::DTS(dts) => format_dts_profile(dts).to_string(),
                        _ => codec_name.clone(),
                    };
                    media_info.channels = channel_layout.clone();
                    media_info.audio_lang = language.clone().unwrap_or_default();
                }

                audio_streams.push(AudioStream {
                    index: stream.index(),
                    codec: codec_name,
                    channels,
                    channel_layout,
                    language,
                    profile,
                });
            }
            MediaType::Subtitle => {
                let codec_id = params.id();
                let language = stream.metadata().get("language").map(|s| s.to_string());

                // Check forced flag via disposition
                let forced = unsafe {
                    let st_ptr = stream.as_ptr();
                    ((*st_ptr).disposition & ffmpeg::ffi::AV_DISPOSITION_FORCED as i32) != 0
                };

                subtitle_streams.push(crate::types::SubtitleStream {
                    index: stream.index(),
                    codec: codec_id.name().to_string(),
                    language,
                    forced,
                });
            }
            _ => {}
        }
    }

    // Bitrate
    let bitrate = ctx.bit_rate();
    media_info.bitrate_bps = if bitrate > 0 { bitrate as u64 } else { 0 };

    let stream_info = StreamInfo {
        video_streams,
        audio_streams,
        subtitle_streams,
    };

    Ok((media_info, stream_info))
}
```

**Note:** The `forced` flag extraction uses unsafe to access `AVStream.disposition`. If `ffmpeg-the-third` doesn't expose this field or the `AV_DISPOSITION_FORCED` constant, set `forced: false` and add a TODO for future enhancement. Check during implementation.

- [ ] **Step 2: Update `disc.rs` wrapper**

Update `disc::probe_media_info()` to use `probe_playlist()`:

```rust
pub fn probe_media_info(device: &str, playlist_num: &str) -> Option<MediaInfo> {
    crate::media::probe_playlist(device, playlist_num)
        .ok()
        .map(|(info, _)| info)
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test`

Expected: All PASS

- [ ] **Step 4: Commit**

```
feat: add unified probe_playlist() returning MediaInfo + StreamInfo
```

---

### Task 6: CLI Flags

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add new CLI args to the `Args` struct**

Add after the `no_hooks` field:

```rust
/// Filter audio streams by language (e.g. "eng,jpn")
#[arg(long)]
audio_lang: Option<String>,

/// Filter subtitle streams by language (e.g. "eng")
#[arg(long)]
subtitle_lang: Option<String>,

/// Prefer surround audio (select surround + one stereo)
#[arg(long)]
prefer_surround: bool,

/// Include all streams, ignoring config filters
#[arg(long, conflicts_with_all = ["audio_lang", "subtitle_lang", "prefer_surround"])]
all_streams: bool,

/// Select streams by type-local index (e.g. "a:0,2;s:0-1")
#[arg(long, conflicts_with_all = ["audio_lang", "subtitle_lang", "prefer_surround", "all_streams"])]
tracks: Option<String>,
```

**Note:** `--audio-lang`, `--subtitle-lang`, and `--prefer-surround` compose together to build a single `StreamFilter`. `--all-streams` and `--tracks` each conflict with all filter flags. `--tracks` also conflicts with `--all-streams`.

- [ ] **Step 2: Build a `StreamFilter` from CLI args in `run_inner()`**

Add after config loading in `run_inner()`:

```rust
// Resolve stream filter: CLI flags > config
let stream_filter = if args.all_streams {
    crate::streams::StreamFilter::default() // empty = all streams
} else if args.audio_lang.is_some() || args.subtitle_lang.is_some() || args.prefer_surround {
    crate::streams::StreamFilter {
        audio_languages: args
            .audio_lang
            .as_deref()
            .map(|s| s.split(',').map(|l| l.trim().to_string()).collect())
            .unwrap_or_default(),
        subtitle_languages: args
            .subtitle_lang
            .as_deref()
            .map(|s| s.split(',').map(|l| l.trim().to_string()).collect())
            .unwrap_or_default(),
        prefer_surround: args.prefer_surround,
    }
} else {
    config.resolve_stream_filter()
};
```

Pass `stream_filter` and `args.tracks` to CLI and TUI entry points (this wiring will be completed in Tasks 8 and 9).

- [ ] **Step 3: Run `cargo build` to verify args parse**

Run: `cargo build`

Expected: Compiles (the args are defined but not fully wired yet)

- [ ] **Step 4: Commit**

```
feat: add --audio-lang, --subtitle-lang, --tracks, --prefer-surround, --all-streams CLI flags
```

---

### Task 7: Session & Wizard State Changes

**Files:**
- Modify: `src/tui/mod.rs`
- Modify: `src/types.rs`
- Modify: `src/session.rs`

- [ ] **Step 1: Add `TrackEdit` to `InputFocus` enum**

In `src/tui/mod.rs`, update `InputFocus`:

```rust
#[derive(Debug, Clone, Default, PartialEq)]
pub enum InputFocus {
    #[default]
    TextInput,
    List,
    /// Episode assignment editing (visible row index)
    InlineEdit(usize),
    /// Track selection editing (sub-row index within expanded tracks)
    TrackEdit(usize),
}
```

- [ ] **Step 2: Update `WizardState` — add new fields, change `media_infos` to `HashMap`**

```rust
#[derive(Default)]
pub struct WizardState {
    pub list_cursor: usize,
    pub input_buffer: String,
    pub input_focus: InputFocus,
    pub season_num: Option<u32>,
    pub start_episode: Option<u32>,
    pub episode_assignments: EpisodeAssignments,
    pub playlist_selected: Vec<bool>,
    pub specials: std::collections::HashSet<String>,
    pub show_filtered: bool,
    pub filenames: Vec<String>,
    pub media_infos: std::collections::HashMap<String, crate::types::MediaInfo>,
    pub stream_infos: std::collections::HashMap<String, crate::types::StreamInfo>,
    pub track_selections: std::collections::HashMap<String, Vec<usize>>,
    pub expanded_playlist: Option<usize>,
}
```

- [ ] **Step 3: Update `PlaylistView` — add stream fields**

In `src/types.rs`, add to `PlaylistView`:

```rust
pub stream_infos: HashMap<String, StreamInfo>,
pub track_selections: HashMap<String, Vec<usize>>,
pub expanded_playlist: Option<usize>,
```

- [ ] **Step 4: Update `ConfirmView` — change `media_infos` type, add track summary**

```rust
pub struct ConfirmView {
    pub filenames: Vec<String>,
    pub playlists: Vec<Playlist>,
    pub episode_assignments: EpisodeAssignments,
    pub list_cursor: usize,
    pub movie_mode: bool,
    pub label: String,
    pub output_dir: String,
    pub dry_run: bool,
    pub media_infos: HashMap<String, MediaInfo>,
    pub track_summaries: Vec<String>, // e.g. "1v 3a 2s" per playlist
}
```

- [ ] **Step 5: Update `BackgroundResult::MediaProbe` for lazy single-playlist probe**

**IMPORTANT:** Steps 5, 7, and 8 must be applied together in a single edit pass — the code will not compile between them since the variant signature change breaks all existing senders/receivers.

```rust
/// Single playlist probe result (for lazy probe of filtered playlists)
MediaProbe(String, Option<(MediaInfo, StreamInfo)>),
```

Where the `String` is the playlist number.

- [ ] **Step 6: Update `build_playlist_view()` in `session.rs`**

Add the new fields to the `PlaylistView` construction:

```rust
stream_infos: self.wizard.stream_infos.clone(),
track_selections: self.wizard.track_selections.clone(),
expanded_playlist: self.wizard.expanded_playlist,
```

- [ ] **Step 7: Update `BackgroundResult::MediaProbe` handler in `session.rs`**

Replace the old handler with lazy probe handling:

```rust
BackgroundResult::MediaProbe(playlist_num, result) => {
    if let Some((media_info, stream_info)) = result {
        self.wizard.media_infos.insert(playlist_num.clone(), media_info);
        self.wizard.stream_infos.insert(playlist_num, stream_info);
    }
    self.status_message.clear();
}
```

- [ ] **Step 8: Update Playlist Manager `Enter` handler to use cached data**

In `src/tui/wizard.rs`, replace the background probe spawn on `Enter` with direct cache lookup:

```rust
KeyCode::Enter => {
    let selected_indices: Vec<usize> = session
        .disc
        .playlists
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            session.wizard.playlist_selected.get(*i).copied().unwrap_or(false)
        })
        .map(|(i, _)| i)
        .collect();

    let filenames: Vec<String> = selected_indices
        .iter()
        .map(|&idx| {
            let pl = &session.disc.playlists[idx];
            let media_info = session.wizard.media_infos.get(&pl.num);
            session.playlist_filename(idx, media_info)
        })
        .collect();

    session.wizard.filenames = filenames;

    if session.wizard.filenames.is_empty() {
        session.status_message = "No playlists selected.".into();
    } else {
        session.wizard.list_cursor = 0;
        session.status_message.clear();
        session.screen = Screen::Confirm;
    }
}
```

- [ ] **Step 9: Add upfront probing during disc scan completion**

In `session.rs`, in the `BackgroundResult::DiscScan` handler, after playlists are stored, spawn a background probe for episode-length playlists:

```rust
// After storing playlists, probe media info + stream info for episode-length playlists
let device = self.device.to_string_lossy().to_string();
let min_duration = self.config.min_duration.unwrap_or(900);
let episode_nums: Vec<String> = self.disc.episodes_pl.iter().map(|pl| pl.num.clone()).collect();
let (tx, rx) = std::sync::mpsc::channel();
std::thread::spawn(move || {
    let mut results = std::collections::HashMap::new();
    for num in &episode_nums {
        if let Ok((media, streams)) = crate::media::probe::probe_playlist(&device, num) {
            results.insert(num.clone(), (media, streams));
        }
    }
    let _ = tx.send(BackgroundResult::BulkProbe(results));
});
self.pending_rx = Some(rx);
```

Add new `BackgroundResult` variant:

```rust
/// Bulk probe results for episode-length playlists (playlist_num → (MediaInfo, StreamInfo))
BulkProbe(HashMap<String, (MediaInfo, StreamInfo)>),
```

And its handler:

```rust
BackgroundResult::BulkProbe(results) => {
    for (num, (media, streams)) in results {
        self.wizard.media_infos.insert(num.clone(), media);
        self.wizard.stream_infos.insert(num, streams);
    }
}
```

- [ ] **Step 10: Fix all compilation errors from type changes**

The `media_infos` change from `Vec<Option<MediaInfo>>` to `HashMap<String, MediaInfo>` will break several callers. Fix each one — key locations:
- `session.rs` `build_confirm_view()` — update to use HashMap
- `tui/wizard.rs` `render_confirm_view()` — update to use HashMap lookup by playlist num
- Any `ConfirmView` consumers

- [ ] **Step 11: Run tests**

Run: `cargo test`

Expected: All PASS (some tests may need adjustment for type changes)

- [ ] **Step 12: Commit**

```
feat: update session/wizard state for per-playlist stream tracking

- WizardState uses HashMap for media_infos, stream_infos, track_selections
- Upfront probing during disc scan for episode-length playlists
- Playlist Manager Enter reads from cache (no background probe)
- BackgroundResult::BulkProbe for upfront probing
- BackgroundResult::MediaProbe repurposed for lazy single-playlist probe
```

---

### Task 8: CLI Rip Integration

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/workflow.rs`

- [ ] **Step 1: Update CLI rip flow for per-playlist stream resolution**

Replace the single `config.resolve_stream_selection()` call with per-playlist resolution. The CLI needs to receive `StreamFilter` and `Option<String>` (tracks spec) from `main.rs`.

**Probe once, use twice:** Before the rip loop, probe all selected playlists and cache results. This avoids double-probing (once for filenames, once for stream resolution):

```rust
// Probe all selected playlists once
let mut probe_cache: HashMap<String, (MediaInfo, StreamInfo)> = HashMap::new();
for pl in &selected_playlists {
    if let Ok(result) = crate::media::probe::probe_playlist(device, &pl.num) {
        probe_cache.insert(pl.num.clone(), result);
    }
}
```

Then in the rip loop, for each playlist:

```rust
let stream_selection = if let Some(ref tracks_spec) = tracks_flag {
    let stream_info = probe_cache
        .get(&pl.num)
        .map(|(_, si)| si.clone())
        .unwrap_or_default();
    let indices = crate::streams::parse_track_spec(tracks_spec, &stream_info)?;
    let errors = crate::streams::validate_track_selection(&indices, &stream_info);
    if !errors.is_empty() {
        anyhow::bail!("Playlist {}: {}", pl.num, errors.join(", "));
    }
    StreamSelection::Manual(indices)
} else if !stream_filter.is_empty() {
    let stream_info = probe_cache
        .get(&pl.num)
        .map(|(_, si)| si.clone())
        .unwrap_or_default();
    let indices = stream_filter.apply(&stream_info);
    StreamSelection::Manual(indices)
} else {
    StreamSelection::All
};
```

Also update `build_filenames()` to use the same `probe_cache` for media info instead of calling `disc::probe_media_info()` separately.

- [ ] **Step 2: Update `--list-playlists -v` to show type-local indices**

In the verbose output rendering, update to use `probe_playlist()` and show stream indices:

```rust
let verbose_info: Vec<Option<(crate::types::MediaInfo, crate::types::StreamInfo)>> =
    if args.verbose {
        log::info!("Probing streams...");
        println!("Probing streams...");
        playlists
            .iter()
            .map(|pl| crate::media::probe::probe_playlist(&device, &pl.num).ok())
            .collect()
    } else {
        vec![None; playlists.len()]
    };
```

Update the verbose stream display to include type-local indices (e.g., `a0: TrueHD 7.1 (eng)`, `s0: PGS (eng)`).

- [ ] **Step 3: Run tests**

Run: `cargo test`

Expected: All PASS

- [ ] **Step 4: Commit**

```
feat: wire per-playlist stream resolution in CLI rip flow

--tracks and --audio-lang/--subtitle-lang resolve per-playlist.
--list-playlists -v shows type-local stream indices.
```

---

### Task 9: TUI Inline Track Expansion

**Files:**
- Modify: `src/tui/wizard.rs`

This is the largest task. It adds rendering and input handling for the inline track expansion.

- [ ] **Step 1: Add `t` key handler to Playlist Manager input**

In `handle_playlist_manager_input_session()`, in the `InputFocus::List` match arm, add:

```rust
KeyCode::Char('t') | KeyCode::Char('T') => {
    let visible = session.visible_playlists();
    if let Some(&(real_idx, _)) = visible.get(session.wizard.list_cursor) {
        if session.wizard.expanded_playlist == Some(real_idx) {
            // Collapse
            session.wizard.expanded_playlist = None;
            session.wizard.input_focus = InputFocus::List;
        } else {
            // Expand (collapse any existing first)
            session.wizard.expanded_playlist = Some(real_idx);
            let pl_num = &session.disc.playlists[real_idx].num;

            // Lazy probe if not cached
            if !session.wizard.stream_infos.contains_key(pl_num) {
                let device = session.device.to_string_lossy().to_string();
                let num = pl_num.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let result = crate::media::probe::probe_playlist(&device, &num).ok();
                    let _ = tx.send(BackgroundResult::MediaProbe(num, result));
                });
                session.pending_rx = Some(rx);
                session.status_message = "Probing streams...".into();
            } else {
                session.wizard.input_focus = InputFocus::TrackEdit(0);
            }
        }
    }
}
```

- [ ] **Step 2: Add `TrackEdit` input handling**

Add a new match arm in `handle_playlist_manager_input_session()` for `InputFocus::TrackEdit(sub_row)`:

```rust
InputFocus::TrackEdit(sub_row) => match key.code {
    KeyCode::Up => {
        if sub_row > 0 {
            session.wizard.input_focus = InputFocus::TrackEdit(sub_row - 1);
        }
    }
    KeyCode::Down => {
        if let Some(real_idx) = session.wizard.expanded_playlist {
            let pl_num = &session.disc.playlists[real_idx].num;
            if let Some(info) = session.wizard.stream_infos.get(pl_num) {
                let total = info.video_streams.len()
                    + info.audio_streams.len()
                    + info.subtitle_streams.len();
                if sub_row + 1 < total {
                    session.wizard.input_focus = InputFocus::TrackEdit(sub_row + 1);
                }
            }
        }
    }
    KeyCode::Char(' ') => {
        // Toggle stream selection
        if let Some(real_idx) = session.wizard.expanded_playlist {
            let pl_num = session.disc.playlists[real_idx].num.clone();
            if let Some(info) = session.wizard.stream_infos.get(&pl_num) {
                let all_indices: Vec<usize> = info
                    .video_streams.iter().map(|s| s.index)
                    .chain(info.audio_streams.iter().map(|s| s.index))
                    .chain(info.subtitle_streams.iter().map(|s| s.index))
                    .collect();

                if let Some(&abs_idx) = all_indices.get(sub_row) {
                    let selections = session
                        .wizard
                        .track_selections
                        .entry(pl_num.clone())
                        .or_insert_with(|| {
                            // Initialize from stream filter defaults
                            let filter = session.config.resolve_stream_filter();
                            filter.apply(info)
                        });

                    if let Some(pos) = selections.iter().position(|&i| i == abs_idx) {
                        selections.remove(pos);
                    } else {
                        selections.push(abs_idx);
                        selections.sort_unstable();
                    }
                }
            }
        }
    }
    KeyCode::Esc | KeyCode::Char('t') | KeyCode::Char('T') => {
        session.wizard.expanded_playlist = None;
        session.wizard.input_focus = InputFocus::List;
    }
    _ => {}
},
```

- [ ] **Step 3: Add focus state interaction guards**

In the `InputFocus::List` match arm, guard `t` during `InlineEdit`:

The existing code already handles this since `t` is only matched under `InputFocus::List`, not `InputFocus::InlineEdit`.

For `e` during `TrackEdit`, the `TrackEdit` match arm doesn't handle `e`, so it's naturally a no-op.

For `f` during expansion, update the `f` handler:

```rust
KeyCode::Char('f') | KeyCode::Char('F') => {
    // Collapse track expansion before toggling filtered
    session.wizard.expanded_playlist = None;
    if matches!(session.wizard.input_focus, InputFocus::TrackEdit(_)) {
        session.wizard.input_focus = InputFocus::List;
    }
    session.wizard.show_filtered = !session.wizard.show_filtered;
}
```

- [ ] **Step 4: Add track expansion rendering to `render_playlist_view()`**

This is the rendering code that shows expanded tracks below a playlist row. In the row-building loop in `render_playlist_view()`, after creating each row, check if the playlist is expanded and insert track rows:

This is a significant rendering change. The implementation should insert additional rows with indentation and stream details between playlist rows when `view.expanded_playlist == Some(real_idx)`. Use the pattern from the mockup:
- Section headers (VIDEO, AUDIO, SUBTITLES) in dimmed text
- Type-local indices (v0, a0, s0)
- Checkbox [X] or [ ] based on track_selections
- Config-filtered streams dimmed
- FORCED badge in yellow for forced subtitles

The exact rendering code will be substantial — follow the existing row rendering pattern and the mockup from the spec.

- [ ] **Step 5: Update "Ch" column for custom track indicator**

When a playlist has custom track selections, show `1v 3a 2s*` instead of the default channel layout:

```rust
let ch_str = if let Some(selections) = view.track_selections.get(&pl.num) {
    if let Some(info) = view.stream_infos.get(&pl.num) {
        let nv = info.video_streams.iter().filter(|s| selections.contains(&s.index)).count();
        let na = info.audio_streams.iter().filter(|s| selections.contains(&s.index)).count();
        let ns = info.subtitle_streams.iter().filter(|s| selections.contains(&s.index)).count();
        format!("{}v {}a {}s*", nv, na, ns)
    } else {
        view.chapter_counts.get(&pl.num).map(|c| c.to_string()).unwrap_or_default()
    }
} else {
    view.chapter_counts.get(&pl.num).map(|c| c.to_string()).unwrap_or_default()
};
```

- [ ] **Step 6: Run tests**

Run: `cargo test`

Expected: All PASS

- [ ] **Step 7: Commit**

```
feat: add inline track expansion in TUI Playlist Manager

Press 't' to expand/collapse stream list under a playlist.
Space toggles individual streams. Lazy probe for filtered playlists.
Ch column shows custom track summary when selections differ from defaults.
```

---

### Task 10: Settings Panel & Env Vars

**Files:**
- Modify: `src/tui/settings.rs`
- Modify: `src/types.rs` (settings items + env overrides)

- [ ] **Step 1: Add Streams separator and items to `from_config_with_drives()`**

Add after the existing items, before the Hooks separator:

```rust
SettingItem::Separator { label: Some("Streams".into()) },
SettingItem::Text {
    label: "Audio Languages".into(),
    key: "audio_languages".into(),
    value: config
        .streams
        .as_ref()
        .and_then(|s| s.audio_languages.as_ref())
        .map(|v| v.join(","))
        .unwrap_or_default(),
},
SettingItem::Text {
    label: "Subtitle Languages".into(),
    key: "subtitle_languages".into(),
    value: config
        .streams
        .as_ref()
        .and_then(|s| s.subtitle_languages.as_ref())
        .map(|v| v.join(","))
        .unwrap_or_default(),
},
SettingItem::Toggle {
    label: "Prefer Surround".into(),
    key: "prefer_surround".into(),
    value: config
        .streams
        .as_ref()
        .and_then(|s| s.prefer_surround)
        .unwrap_or(false),
},
```

- [ ] **Step 2: Update `to_config()` to write `[streams]` fields**

Add match arms for the new keys:

```rust
"audio_languages" if !value.is_empty() => {
    let langs: Vec<String> = value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if !langs.is_empty() {
        let streams = config.streams.get_or_insert_with(Default::default);
        streams.audio_languages = Some(langs);
    }
}
"subtitle_languages" if !value.is_empty() => {
    let langs: Vec<String> = value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if !langs.is_empty() {
        let streams = config.streams.get_or_insert_with(Default::default);
        streams.subtitle_languages = Some(langs);
    }
}
// ... and for the toggle:
"prefer_surround" if *value => {
    let streams = config.streams.get_or_insert_with(Default::default);
    streams.prefer_surround = Some(true);
}
```

- [ ] **Step 3: Add env var overrides for streams settings**

In `apply_env_overrides()` in `types.rs`, add:

```rust
if let Ok(val) = std::env::var("BLUBACK_AUDIO_LANGUAGES") {
    // Find audio_languages item and update
    for item in &mut self.items {
        if let SettingItem::Text { key, value, .. } = item {
            if key == "audio_languages" {
                *value = val.clone();
                imported.push("BLUBACK_AUDIO_LANGUAGES");
                break;
            }
        }
    }
}
if let Ok(val) = std::env::var("BLUBACK_SUBTITLE_LANGUAGES") {
    for item in &mut self.items {
        if let SettingItem::Text { key, value, .. } = item {
            if key == "subtitle_languages" {
                *value = val.clone();
                imported.push("BLUBACK_SUBTITLE_LANGUAGES");
                break;
            }
        }
    }
}
if let Ok(val) = std::env::var("BLUBACK_PREFER_SURROUND") {
    for item in &mut self.items {
        if let SettingItem::Toggle { key, value, .. } = item {
            if key == "prefer_surround" {
                *value = val.eq_ignore_ascii_case("true") || val == "1";
                imported.push("BLUBACK_PREFER_SURROUND");
                break;
            }
        }
    }
}
```

- [ ] **Step 4: Update `test_settings_state_from_config_item_count`**

Change the expected non-separator count from 27 to 30 (3 new items).

- [ ] **Step 5: Run tests**

Run: `cargo test --lib types::tests::test_settings_state_from_config_item_count`

Expected: PASS

- [ ] **Step 6: Commit**

```
feat: add Streams settings to settings panel with env var overrides
```

---

### Task 11: Verify Integration & Guards

**Files:**
- Modify: `src/verify.rs` (if it exists — depends on `feature/rip-verification` being merged first)

**Prerequisite:** This task requires the rip verification feature (`feature/rip-verification` branch) to be merged to `main` first. If not yet merged, skip this task and add a TODO comment in the verify module when it lands.

- [ ] **Step 1: Update verify stream count check**

In `verify.rs`, find the stream count comparison. Update it to compare against the expected selected count rather than the source's total. The selected count should be passed as a parameter to the verify function (or the expected counts stored alongside the verify options).

Add a field to the verify options or the verify function signature:

```rust
pub expected_stream_count: Option<usize>,
```

When `StreamSelection::Manual(indices)` is used, set this to `indices.len()`. When `All`, leave as `None` to use the source total.

- [ ] **Step 2: Run tests**

Run: `cargo test`

Expected: All PASS

- [ ] **Step 3: Commit**

```
fix: verify stream count check uses selected count, not source total
```

---

### Task 12: Final Wiring & Cleanup

**Files:**
- Modify: `src/main.rs`
- Modify: `src/cli.rs`
- Modify: `src/session.rs`

- [ ] **Step 1: Add `stream_filter` and `tracks_spec` to `DriveSession`**

Add fields to `DriveSession` in `session.rs`:

```rust
pub stream_filter: crate::streams::StreamFilter,
pub tracks_spec: Option<String>,
```

Update `DriveSession::new()` to accept these parameters. Update `Coordinator` to pass them through from args.

- [ ] **Step 2: Pass `StreamFilter` and `tracks` spec through to CLI and TUI**

In `main.rs` `run_inner()`, pass the resolved `stream_filter` and `args.tracks` to the CLI and TUI entry points. Update function signatures as needed.

- [ ] **Step 3: Migrate TUI dashboard rip path to per-playlist stream resolution**

In `src/tui/dashboard.rs`, update `start_next_rip()` (or wherever `resolve_stream_selection()` is called ~line 543) to use per-playlist resolution:

```rust
let stream_selection = if let Some(indices) = session.wizard.track_selections.get(&pl.num) {
    StreamSelection::Manual(indices.clone())
} else if !session.stream_filter.is_empty() {
    if let Some(info) = session.wizard.stream_infos.get(&pl.num) {
        StreamSelection::Manual(session.stream_filter.apply(info))
    } else {
        StreamSelection::All
    }
} else {
    StreamSelection::All
};
```

This is critical — without this, TUI rips would always use `StreamSelection::All` regardless of config or user selections.

- [ ] **Step 4: Remove deprecated `resolve_stream_selection()` if no callers remain**

Grep for `resolve_stream_selection`. If all callers have been migrated to `resolve_stream_filter()`, remove the method.

- [ ] **Step 3: Run full test suite**

Run: `cargo test`

Expected: All PASS

- [ ] **Step 4: Run clippy**

Run: `cargo clippy -- -D warnings`

Expected: No warnings

- [ ] **Step 5: Run fmt**

Run: `rustup run stable cargo fmt`

- [ ] **Step 6: Commit**

```
feat: complete per-stream track selection (v0.10 item #20)

Two-layer stream filtering: config defaults + per-playlist TUI overrides.
CLI: --audio-lang, --subtitle-lang, --tracks, --prefer-surround, --all-streams.
Config: [streams] section replaces deprecated stream_selection key.
TUI: inline track expansion with 't' key in Playlist Manager.
```

---

## Dependency Graph

```
Task 1 (types)
  ├─► Task 2 (StreamFilter)
  │     ├─► Task 3 (config)
  │     │     ├─► Task 10 (settings)
  │     │     └─► Task 8 (CLI rip)
  │     └─► Task 6 (CLI flags)
  ├─► Task 4 (remux simplification)
  │     └─► Task 11 (verify)
  ├─► Task 5 (unified probing)
  │     ├─► Task 7 (session state)
  │     │     ├─► Task 8 (CLI rip)
  │     │     └─► Task 9 (TUI expansion)
  │     └─► Task 8 (CLI rip)
  └─► Task 12 (final wiring)
```

Tasks 1-5 can be parallelized (1 must go first, then 2-5 are mostly independent). Tasks 6-10 depend on earlier tasks. Task 11 requires the rip-verification branch to be merged first. Task 12 is the final integration — critically includes migrating the TUI dashboard rip path and propagating `StreamFilter` to sessions.
