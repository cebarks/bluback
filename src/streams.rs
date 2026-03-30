use crate::types::{AudioStream, StreamInfo};

#[cfg(test)]
use crate::types::{SubtitleStream, VideoStream};

/// Config/CLI-derived stream filtering rules.
#[derive(Debug, Clone, Default)]
pub struct StreamFilter {
    pub audio_languages: Vec<String>,
    pub subtitle_languages: Vec<String>,
    pub prefer_surround: bool,
}

impl StreamFilter {
    pub fn is_empty(&self) -> bool {
        self.audio_languages.is_empty()
            && self.subtitle_languages.is_empty()
            && !self.prefer_surround
    }

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
                    None | Some("und") => true,
                    Some(l) => languages.iter().any(|f| f.eq_ignore_ascii_case(l)),
                }
            })
            .map(&get_index)
            .collect();

        if matching.is_empty() {
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
            None => candidates.to_vec(),
        }
    }
}

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
        let (prefix, indices_str) = part.split_once(':').ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid track spec '{}': expected 'v:', 'a:', or 's:' prefix",
                part
            )
        })?;

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
                            indices_str, info.video_streams.len()
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
                            indices_str, info.audio_streams.len()
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
                            indices_str, info.subtitle_streams.len()
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
pub fn validate_track_selection(selected: &[usize], info: &StreamInfo) -> Vec<String> {
    let mut errors = Vec::new();
    let has_video_source = !info.video_streams.is_empty();
    let has_audio_source = !info.audio_streams.is_empty();

    if has_video_source
        && !info
            .video_streams
            .iter()
            .any(|v| selected.contains(&v.index))
    {
        errors.push("no video streams selected".into());
    }
    if has_audio_source
        && !info
            .audio_streams
            .iter()
            .any(|a| selected.contains(&a.index))
    {
        errors.push("no audio streams selected".into());
    }
    if selected.is_empty() {
        errors.push("no streams selected".into());
    }
    errors
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
            ..Default::default()
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

    #[test]
    fn test_parse_zero_based_ranges() {
        assert_eq!(parse_zero_based_ranges("0", 3), Some(vec![0]));
        assert_eq!(parse_zero_based_ranges("0,2", 3), Some(vec![0, 2]));
        assert_eq!(parse_zero_based_ranges("0-2", 3), Some(vec![0, 1, 2]));
        assert_eq!(parse_zero_based_ranges("0, 2", 3), Some(vec![0, 2]));
        assert_eq!(parse_zero_based_ranges("3", 3), None);
        assert_eq!(parse_zero_based_ranges("0-3", 3), None);
        assert_eq!(parse_zero_based_ranges("", 3), None);
    }

    #[test]
    fn test_parse_track_spec_basic() {
        let info = make_stream_info();
        let result = parse_track_spec("a:0", &info).unwrap();
        assert_eq!(result, vec![0, 1, 6, 7, 8, 9]);
    }

    #[test]
    fn test_parse_track_spec_multiple_types() {
        let info = make_stream_info();
        let result = parse_track_spec("a:0,1;s:0", &info).unwrap();
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
        let errors = validate_track_selection(&[0, 1, 6], &info);
        assert!(errors.is_empty());

        let errors = validate_track_selection(&[0, 6], &info);
        assert_eq!(errors, vec!["no audio streams selected"]);

        let errors = validate_track_selection(&[1, 6], &info);
        assert_eq!(errors, vec!["no video streams selected"]);

        let errors = validate_track_selection(&[], &info);
        assert!(errors.contains(&"no streams selected".to_string()));
    }

    // --- StreamFilter::apply() edge cases ---

    #[test]
    fn test_prefer_surround_without_language_filter() {
        // prefer_surround alone, no audio_languages — should prefer surround from ALL audio
        let filter = StreamFilter {
            prefer_surround: true,
            ..Default::default()
        };
        let info = make_stream_info();
        let selected = filter.apply(&info);
        // video(0) + first surround(1) + stereo(3) + all subs(6-9)
        assert_eq!(selected, vec![0, 1, 3, 6, 7, 8, 9]);
    }

    #[test]
    fn test_filter_no_video_streams() {
        // Audio-only source — should not crash, include all audio
        let info = StreamInfo {
            video_streams: vec![],
            audio_streams: vec![AudioStream {
                index: 0,
                codec: "ac3".into(),
                channels: 6,
                channel_layout: "5.1".into(),
                language: Some("eng".into()),
                profile: None,
            }],
            subtitle_streams: vec![],
            ..Default::default()
        };
        let filter = StreamFilter::default();
        let selected = filter.apply(&info);
        assert_eq!(selected, vec![0]);
    }

    #[test]
    fn test_filter_multiple_languages() {
        // Two audio languages — both should be included
        let filter = StreamFilter {
            audio_languages: vec!["eng".into(), "fra".into()],
            ..Default::default()
        };
        let info = make_stream_info();
        let selected = filter.apply(&info);
        // video(0) + eng audio(1,2,3) + fra audio(4) + all subs(6-9)
        assert_eq!(selected, vec![0, 1, 2, 3, 4, 6, 7, 8, 9]);
    }

    #[test]
    fn test_prefer_surround_no_surround_available() {
        // All audio is stereo — prefer_surround should keep all (no surround to prefer)
        let info = StreamInfo {
            video_streams: vec![VideoStream {
                index: 0,
                codec: "h264".into(),
                resolution: "1920x1080".into(),
                hdr: "SDR".into(),
                framerate: "24".into(),
                bit_depth: "8".into(),
            }],
            audio_streams: vec![
                AudioStream {
                    index: 1,
                    codec: "ac3".into(),
                    channels: 2,
                    channel_layout: "stereo".into(),
                    language: Some("eng".into()),
                    profile: None,
                },
                AudioStream {
                    index: 2,
                    codec: "ac3".into(),
                    channels: 2,
                    channel_layout: "stereo".into(),
                    language: Some("fra".into()),
                    profile: None,
                },
            ],
            subtitle_streams: vec![],
            ..Default::default()
        };
        let filter = StreamFilter {
            prefer_surround: true,
            ..Default::default()
        };
        let selected = filter.apply(&info);
        // No surround -> keep all audio
        assert_eq!(selected, vec![0, 1, 2]);
    }

    #[test]
    fn test_filter_all_und_audio() {
        // All audio streams have no language tag — language filter should include all (und always passes)
        let info = StreamInfo {
            video_streams: vec![VideoStream {
                index: 0,
                codec: "h264".into(),
                resolution: "1920x1080".into(),
                hdr: "SDR".into(),
                framerate: "24".into(),
                bit_depth: "8".into(),
            }],
            audio_streams: vec![
                AudioStream {
                    index: 1,
                    codec: "ac3".into(),
                    channels: 6,
                    channel_layout: "5.1".into(),
                    language: None,
                    profile: None,
                },
                AudioStream {
                    index: 2,
                    codec: "ac3".into(),
                    channels: 2,
                    channel_layout: "stereo".into(),
                    language: None,
                    profile: None,
                },
            ],
            subtitle_streams: vec![],
            ..Default::default()
        };
        let filter = StreamFilter {
            audio_languages: vec!["jpn".into()],
            ..Default::default()
        };
        let selected = filter.apply(&info);
        // All und -> all included (und always passes, even though "jpn" is the filter)
        assert_eq!(selected, vec![0, 1, 2]);
    }

    // --- parse_track_spec() edge cases ---

    #[test]
    fn test_parse_track_spec_empty_string() {
        let info = make_stream_info();
        // Empty spec — all types default to all
        let result = parse_track_spec("", &info).unwrap();
        assert_eq!(result, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_parse_track_spec_out_of_range_fallback() {
        let info = make_stream_info();
        // a:99 is out of range — should warn and include all audio
        let result = parse_track_spec("a:99", &info).unwrap();
        // video(0) + all audio(1-5, fallback) + all subs(6-9)
        assert_eq!(result, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_parse_track_spec_duplicates_deduped() {
        let info = make_stream_info();
        let result = parse_track_spec("a:0,0,1;a:0", &info).unwrap();
        // Should dedup: video(0) + audio(1,2) + all subs(6-9)
        assert_eq!(result, vec![0, 1, 2, 6, 7, 8, 9]);
    }

    #[test]
    fn test_parse_track_spec_whitespace_tolerance() {
        let info = make_stream_info();
        let result = parse_track_spec(" a : 0 ; s : 0 ", &info).unwrap();
        assert_eq!(result, vec![0, 1, 6]);
    }

    // --- parse_zero_based_ranges() edge cases ---

    #[test]
    fn test_parse_zero_based_ranges_equal_bounds() {
        // "1-1" should be valid, returns [1]
        assert_eq!(parse_zero_based_ranges("1-1", 3), Some(vec![1]));
    }

    #[test]
    fn test_parse_zero_based_ranges_reversed() {
        // "2-0" should fail
        assert_eq!(parse_zero_based_ranges("2-0", 3), None);
    }

    #[test]
    fn test_parse_zero_based_ranges_max_boundary() {
        // max_val=1, "0" should work, "1" should fail
        assert_eq!(parse_zero_based_ranges("0", 1), Some(vec![0]));
        assert_eq!(parse_zero_based_ranges("1", 1), None);
    }

    // --- validate_track_selection() edge cases ---

    #[test]
    fn test_validate_audio_only_source() {
        // Audio-only source (no video) — should NOT complain about missing video
        let info = StreamInfo {
            video_streams: vec![],
            audio_streams: vec![AudioStream {
                index: 0,
                codec: "ac3".into(),
                channels: 2,
                channel_layout: "stereo".into(),
                language: Some("eng".into()),
                profile: None,
            }],
            subtitle_streams: vec![],
            ..Default::default()
        };
        let errors = validate_track_selection(&[0], &info);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_validate_video_only_source() {
        // Video-only source (no audio) — should NOT complain about missing audio
        let info = StreamInfo {
            video_streams: vec![VideoStream {
                index: 0,
                codec: "h264".into(),
                resolution: "1920x1080".into(),
                hdr: "SDR".into(),
                framerate: "24".into(),
                bit_depth: "8".into(),
            }],
            audio_streams: vec![],
            subtitle_streams: vec![],
            ..Default::default()
        };
        let errors = validate_track_selection(&[0], &info);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_validate_both_missing_from_full_source() {
        // Full source but selection has neither video nor audio
        let info = make_stream_info();
        let errors = validate_track_selection(&[6, 7], &info); // only subtitles
        assert!(errors.contains(&"no video streams selected".to_string()));
        assert!(errors.contains(&"no audio streams selected".to_string()));
    }
}
