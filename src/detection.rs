use crate::types::{Episode, Playlist};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestedType {
    Episode,
    Special,
    MultiEpisode,
}

#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub playlist_num: String,
    pub suggested_type: SuggestedType,
    pub confidence: Confidence,
    pub reasons: Vec<String>,
}

/// Run detection without chapter information.
#[allow(dead_code)] // Used by tests, will be used by CLI in Task 7
pub fn run_detection(
    all_playlists: &[Playlist],
    min_duration: u32,
    tmdb_episodes: Option<&[Episode]>,
    tmdb_specials: Option<&[Episode]>,
) -> Vec<DetectionResult> {
    run_detection_with_chapters(
        all_playlists,
        min_duration,
        tmdb_episodes,
        tmdb_specials,
        &HashMap::new(),
    )
}

/// Run detection with optional chapter counts.
pub fn run_detection_with_chapters(
    all_playlists: &[Playlist],
    min_duration: u32,
    tmdb_episodes: Option<&[Episode]>,
    tmdb_specials: Option<&[Episode]>,
    chapter_counts: &HashMap<String, usize>,
) -> Vec<DetectionResult> {
    let mut results = run_heuristics(all_playlists, min_duration, chapter_counts);
    apply_tmdb_layer(&mut results, all_playlists, tmdb_episodes, tmdb_specials);
    results
}

// =============================================================================
// Layer 1: Disc Heuristics
// =============================================================================

fn run_heuristics(
    all_playlists: &[Playlist],
    min_duration: u32,
    chapter_counts: &HashMap<String, usize>,
) -> Vec<DetectionResult> {
    // Baseline: playlists >= min_duration
    let baseline: Vec<&Playlist> = all_playlists
        .iter()
        .filter(|p| p.seconds >= min_duration)
        .collect();

    // Need at least 2 baseline playlists for meaningful detection
    if baseline.len() < 2 {
        // Single playlist or all below threshold: classify as Episode with Low confidence
        return all_playlists
            .iter()
            .map(|p| DetectionResult {
                playlist_num: p.num.clone(),
                suggested_type: SuggestedType::Episode,
                confidence: Confidence::Low,
                reasons: vec!["insufficient baseline for detection".into()],
            })
            .collect();
    }

    let median_duration = compute_median(&baseline);
    let audio_mode = compute_mode(&baseline.iter().map(|p| p.audio_streams).collect::<Vec<_>>());
    let subtitle_mode = compute_mode(
        &baseline
            .iter()
            .map(|p| p.subtitle_streams)
            .collect::<Vec<_>>(),
    );

    // Compute chapter mode only from playlists that have chapter data
    let chapter_mode = {
        let chapter_values: Vec<u32> = baseline
            .iter()
            .filter_map(|p| chapter_counts.get(&p.num).map(|&count| count as u32))
            .collect();
        if chapter_values.len() >= 2 {
            Some(compute_mode(&chapter_values))
        } else {
            None
        }
    };

    all_playlists
        .iter()
        .map(|p| {
            classify_playlist(
                p,
                median_duration,
                audio_mode,
                subtitle_mode,
                chapter_mode,
                chapter_counts,
            )
        })
        .collect()
}

fn classify_playlist(
    p: &Playlist,
    median: u32,
    audio_mode: u32,
    subtitle_mode: u32,
    chapter_mode: Option<u32>,
    chapter_counts: &HashMap<String, usize>,
) -> DetectionResult {
    let mut reasons = Vec::new();

    // Duration analysis (always sets both fields)
    let (mut suggested_type, mut confidence) = if p.seconds < median / 2 {
        // < 50% of median
        reasons.push(format!(
            "duration {} < 50% of median {}",
            format_duration(p.seconds),
            format_duration(median)
        ));
        (SuggestedType::Special, Confidence::High)
    } else if p.seconds < (median * 3 / 4) {
        // 50-75% of median
        reasons.push(format!(
            "duration {} is 50-75% of median {}",
            format_duration(p.seconds),
            format_duration(median)
        ));
        (SuggestedType::Special, Confidence::Medium)
    } else if p.seconds > median * 2 {
        // > 200% of median
        reasons.push(format!(
            "duration {} > 200% of median {}",
            format_duration(p.seconds),
            format_duration(median)
        ));
        (SuggestedType::MultiEpisode, Confidence::High)
    } else {
        // Uniform duration
        reasons.push(format!(
            "duration {} near median {}",
            format_duration(p.seconds),
            format_duration(median)
        ));
        (SuggestedType::Episode, Confidence::Medium)
    };

    // Stream count analysis
    let half_audio_mode = audio_mode / 2;
    let half_subtitle_mode = subtitle_mode / 2;

    if p.audio_streams < half_audio_mode || p.subtitle_streams < half_subtitle_mode {
        reasons.push(format!(
            "low stream count (audio: {}/{}, subs: {}/{})",
            p.audio_streams, audio_mode, p.subtitle_streams, subtitle_mode
        ));

        // If Episode, change to Special with Low confidence
        if suggested_type == SuggestedType::Episode {
            suggested_type = SuggestedType::Special;
            confidence = Confidence::Low;
        }

        // Bump confidence (unless MultiEpisode)
        if suggested_type != SuggestedType::MultiEpisode {
            confidence = bump_confidence(confidence);
        }
    }

    // Chapter count analysis
    if let Some(mode) = chapter_mode {
        if let Some(&count) = chapter_counts.get(&p.num) {
            let half_mode = mode / 2;
            if count == 0 || (count as u32) < half_mode {
                reasons.push(format!("low chapter count ({} vs mode {})", count, mode));

                // If Episode, change to Special with Low confidence
                if suggested_type == SuggestedType::Episode {
                    suggested_type = SuggestedType::Special;
                    confidence = Confidence::Low;
                }

                // Bump confidence toward Special (unless MultiEpisode)
                if suggested_type != SuggestedType::MultiEpisode {
                    confidence = bump_confidence(confidence);
                }
            }
        }
    }

    DetectionResult {
        playlist_num: p.num.clone(),
        suggested_type,
        confidence,
        reasons,
    }
}

fn compute_median(playlists: &[&Playlist]) -> u32 {
    let mut durations: Vec<u32> = playlists.iter().map(|p| p.seconds).collect();
    durations.sort_unstable();
    let mid = durations.len() / 2;
    if durations.len().is_multiple_of(2) {
        (durations[mid - 1] + durations[mid]) / 2
    } else {
        durations[mid]
    }
}

fn compute_mode(values: &[u32]) -> u32 {
    if values.is_empty() {
        return 0;
    }

    let mut counts = HashMap::new();
    for &val in values {
        *counts.entry(val).or_insert(0) += 1;
    }

    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(val, _)| val)
        .unwrap_or(0)
}

fn bump_confidence(current: Confidence) -> Confidence {
    match current {
        Confidence::Low => Confidence::Medium,
        Confidence::Medium | Confidence::High => Confidence::High,
    }
}

fn format_duration(seconds: u32) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;

    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

// =============================================================================
// Layer 2: TMDb Runtime Matching
// =============================================================================

fn apply_tmdb_layer(
    results: &mut [DetectionResult],
    all_playlists: &[Playlist],
    tmdb_episodes: Option<&[Episode]>,
    tmdb_specials: Option<&[Episode]>,
) {
    // Collect runtimes in seconds
    let episode_runtimes: Vec<u32> = tmdb_episodes
        .iter()
        .flat_map(|eps| eps.iter())
        .filter_map(|ep| ep.runtime.map(|r| r * 60))
        .collect();

    let special_runtimes: Vec<u32> = tmdb_specials
        .iter()
        .flat_map(|specs| specs.iter())
        .filter_map(|sp| sp.runtime.map(|r| r * 60))
        .collect();

    // No-op if no runtimes available
    if episode_runtimes.is_empty() && special_runtimes.is_empty() {
        return;
    }

    for (result, playlist) in results.iter_mut().zip(all_playlists.iter()) {
        let matches_regular = episode_runtimes
            .iter()
            .any(|&rt| duration_matches(playlist.seconds, rt));
        let matches_special = special_runtimes
            .iter()
            .any(|&rt| duration_matches(playlist.seconds, rt));

        // If flagged as Special but matches regular episode runtime → clear to Episode
        if result.suggested_type == SuggestedType::Special && matches_regular {
            result.suggested_type = SuggestedType::Episode;
            result.confidence = Confidence::High;
            result.reasons.push("matches TMDb episode runtime".into());
        }

        // If matches special runtime (and not MultiEpisode) → bump toward Special
        if result.suggested_type != SuggestedType::MultiEpisode && matches_special {
            result.confidence = bump_confidence(result.confidence);
            result.reasons.push("matches TMDb special runtime".into());
        }
    }
}

/// Check if playlist duration matches TMDb runtime within tolerance.
/// Tolerance: ±10% or ±3 minutes (180s), whichever is more permissive.
fn duration_matches(playlist_secs: u32, tmdb_secs: u32) -> bool {
    let percent_tolerance = (tmdb_secs as f64 * 0.1) as u32;
    let tolerance = percent_tolerance.max(180);
    let lower = tmdb_secs.saturating_sub(tolerance);
    let upper = tmdb_secs.saturating_add(tolerance);
    playlist_secs >= lower && playlist_secs <= upper
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_playlist(num: &str, seconds: u32, audio: u32, subs: u32) -> Playlist {
        Playlist {
            num: num.into(),
            duration: format_duration(seconds),
            seconds,
            video_streams: 1,
            audio_streams: audio,
            subtitle_streams: subs,
        }
    }

    // =========================================================================
    // Confidence ordering
    // =========================================================================

    #[test]
    fn test_confidence_ordering() {
        assert!(Confidence::Low < Confidence::Medium);
        assert!(Confidence::Medium < Confidence::High);
        assert!(Confidence::High == Confidence::High);
    }

    #[test]
    fn test_bump_confidence() {
        assert_eq!(bump_confidence(Confidence::Low), Confidence::Medium);
        assert_eq!(bump_confidence(Confidence::Medium), Confidence::High);
        assert_eq!(bump_confidence(Confidence::High), Confidence::High);
    }

    // =========================================================================
    // Duration thresholds
    // =========================================================================

    #[test]
    fn test_duration_threshold_high_special() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 600, 2, 2), // < 50% of median
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = &results[2];
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::High);
        assert!(r.reasons[0].contains("< 50%"));
    }

    #[test]
    fn test_duration_threshold_medium_special() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 1000, 2, 2), // 50-75% of median
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = &results[2];
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::Medium);
        assert!(r.reasons[0].contains("50-75%"));
    }

    #[test]
    fn test_duration_threshold_multi_episode() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 3500, 2, 2), // > 200% of median
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = &results[2];
        assert_eq!(r.suggested_type, SuggestedType::MultiEpisode);
        assert_eq!(r.confidence, Confidence::High);
        assert!(r.reasons[0].contains("> 200%"));
    }

    #[test]
    fn test_duration_threshold_uniform() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 1500, 2, 2),
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = &results[0];
        assert_eq!(r.suggested_type, SuggestedType::Episode);
        assert_eq!(r.confidence, Confidence::Medium);
        assert!(r.reasons[0].contains("near median"));
    }

    #[test]
    fn test_single_playlist() {
        let playlists = vec![make_playlist("00001", 1500, 2, 2)];
        let results = run_detection(&playlists, 900, None, None);
        let r = &results[0];
        assert_eq!(r.suggested_type, SuggestedType::Episode);
        assert_eq!(r.confidence, Confidence::Low);
        assert!(r.reasons[0].contains("insufficient baseline"));
    }

    #[test]
    fn test_all_below_min_duration() {
        let playlists = vec![
            make_playlist("00001", 500, 2, 2),
            make_playlist("00002", 600, 2, 2),
        ];
        let results = run_detection(&playlists, 900, None, None);
        assert_eq!(results.len(), 2);
        for r in &results {
            assert_eq!(r.suggested_type, SuggestedType::Episode);
            assert_eq!(r.confidence, Confidence::Low);
        }
    }

    #[test]
    fn test_below_min_duration_evaluated_against_baseline() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 600, 2, 2), // below min_duration but compared to baseline
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = &results[2];
        // Even though it's below min_duration, it's compared to baseline median
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::High);
    }

    // =========================================================================
    // Stream count bump logic
    // =========================================================================

    #[test]
    fn test_stream_count_bump_confidence() {
        let playlists = vec![
            make_playlist("00001", 1500, 4, 4),
            make_playlist("00002", 1500, 4, 4),
            make_playlist("00003", 1500, 1, 1), // < half mode
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = &results[2];
        // Starts as Episode (Medium), bumps to Special (Low), then bumped to Medium
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::Medium);
        assert!(r.reasons.iter().any(|s| s.contains("low stream count")));
    }

    #[test]
    fn test_stream_count_alone_flags_low() {
        let playlists = vec![
            make_playlist("00001", 1500, 4, 4),
            make_playlist("00002", 1500, 4, 4),
            make_playlist("00003", 1500, 1, 1),
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = &results[2];
        // Started as Episode, changed to Special with Low confidence, then bumped to Medium
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::Medium);
    }

    #[test]
    fn test_stream_count_identical_no_bump() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 1500, 2, 2),
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = &results[2];
        assert_eq!(r.suggested_type, SuggestedType::Episode);
        assert_eq!(r.confidence, Confidence::Medium);
        assert!(!r.reasons.iter().any(|s| s.contains("low stream count")));
    }

    #[test]
    fn test_stream_count_does_not_affect_multi_episode() {
        let playlists = vec![
            make_playlist("00001", 1500, 4, 4),
            make_playlist("00002", 1500, 4, 4),
            make_playlist("00003", 3500, 1, 1), // MultiEpisode with low stream count
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = &results[2];
        assert_eq!(r.suggested_type, SuggestedType::MultiEpisode);
        assert_eq!(r.confidence, Confidence::High); // Not bumped
    }

    // =========================================================================
    // Chapter count bump logic
    // =========================================================================

    #[test]
    fn test_chapter_count_bump() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 1500, 2, 2),
        ];
        let mut chapters = HashMap::new();
        chapters.insert("00001".into(), 12);
        chapters.insert("00002".into(), 12);
        chapters.insert("00003".into(), 3); // < half mode

        let results = run_detection_with_chapters(&playlists, 900, None, None, &chapters);
        let r = &results[2];
        // Starts as Episode (Medium), changed to Special (Low), bumped to Medium
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::Medium);
        assert!(r.reasons.iter().any(|s| s.contains("low chapter count")));
    }

    #[test]
    fn test_chapter_count_zero_triggers_bump() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 1500, 2, 2),
        ];
        let mut chapters = HashMap::new();
        chapters.insert("00001".into(), 12);
        chapters.insert("00002".into(), 12);
        chapters.insert("00003".into(), 0);

        let results = run_detection_with_chapters(&playlists, 900, None, None, &chapters);
        let r = &results[2];
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert!(r.reasons.iter().any(|s| s.contains("low chapter count")));
    }

    #[test]
    fn test_chapter_count_does_not_affect_multi_episode() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 3500, 2, 2),
        ];
        let mut chapters = HashMap::new();
        chapters.insert("00001".into(), 12);
        chapters.insert("00002".into(), 12);
        chapters.insert("00003".into(), 0);

        let results = run_detection_with_chapters(&playlists, 900, None, None, &chapters);
        let r = &results[2];
        assert_eq!(r.suggested_type, SuggestedType::MultiEpisode);
        assert_eq!(r.confidence, Confidence::High); // Not bumped
    }

    #[test]
    fn test_chapter_count_no_chapter_data_available() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
        ];
        let chapters = HashMap::new(); // No chapter data

        let results = run_detection_with_chapters(&playlists, 900, None, None, &chapters);
        let r = &results[0];
        // No chapter-based reasoning
        assert!(!r.reasons.iter().any(|s| s.contains("chapter")));
    }

    // =========================================================================
    // Confidence capping at High
    // =========================================================================

    #[test]
    fn test_confidence_caps_at_high() {
        let playlists = vec![
            make_playlist("00001", 1500, 4, 4),
            make_playlist("00002", 1500, 4, 4),
            make_playlist("00003", 600, 1, 1), // High special + stream bump
        ];
        let mut chapters = HashMap::new();
        chapters.insert("00001".into(), 12);
        chapters.insert("00002".into(), 12);
        chapters.insert("00003".into(), 0);

        let results = run_detection_with_chapters(&playlists, 900, None, None, &chapters);
        let r = &results[2];
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::High); // Multiple bumps capped
    }

    // =========================================================================
    // TMDb matching
    // =========================================================================

    #[test]
    fn test_tmdb_clears_special_to_episode() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 1000, 2, 2), // Flagged as Special
        ];
        let episodes = vec![Episode {
            episode_number: 1,
            name: "Test".into(),
            runtime: Some(17), // 1020 seconds (matches 1000 within tolerance)
        }];

        let results = run_detection(&playlists, 900, Some(&episodes), None);
        let r = &results[2];
        assert_eq!(r.suggested_type, SuggestedType::Episode);
        assert_eq!(r.confidence, Confidence::High);
        assert!(r
            .reasons
            .iter()
            .any(|s| s.contains("matches TMDb episode runtime")));
    }

    #[test]
    fn test_tmdb_boosts_special() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 1000, 2, 2),
        ];
        let specials = vec![Episode {
            episode_number: 1,
            name: "Special".into(),
            runtime: Some(17), // 1020 seconds
        }];

        let results = run_detection(&playlists, 900, None, Some(&specials));
        let r = &results[2];
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert!(r
            .reasons
            .iter()
            .any(|s| s.contains("matches TMDb special runtime")));
    }

    #[test]
    fn test_tmdb_season_zero_does_not_change_multi_episode() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 3500, 2, 2),
        ];
        let specials = vec![Episode {
            episode_number: 1,
            name: "Special".into(),
            runtime: Some(58), // 3480 seconds (matches 3500)
        }];

        let results = run_detection(&playlists, 900, None, Some(&specials));
        let r = &results[2];
        assert_eq!(r.suggested_type, SuggestedType::MultiEpisode);
        // Not bumped toward Special
        assert!(!r
            .reasons
            .iter()
            .any(|s| s.contains("matches TMDb special runtime")));
    }

    #[test]
    fn test_tmdb_no_runtime_is_noop() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 1000, 2, 2),
        ];
        let episodes = vec![Episode {
            episode_number: 1,
            name: "Test".into(),
            runtime: None,
        }];

        let results_without = run_detection(&playlists, 900, None, None);
        let results_with = run_detection(&playlists, 900, Some(&episodes), None);

        // Should be identical
        assert_eq!(
            results_without[2].suggested_type,
            results_with[2].suggested_type
        );
        assert_eq!(results_without[2].confidence, results_with[2].confidence);
    }

    // =========================================================================
    // duration_matches tolerance
    // =========================================================================

    #[test]
    fn test_duration_matches_exact() {
        assert!(duration_matches(1500, 1500));
    }

    #[test]
    fn test_duration_matches_within_percent() {
        // 1500s ± 10% = ±150s, but ±3min (180s) is more permissive
        // So tolerance is ±180s: 1320-1680
        assert!(duration_matches(1320, 1500));
        assert!(duration_matches(1680, 1500));
        assert!(!duration_matches(1319, 1500));
        assert!(!duration_matches(1681, 1500));
    }

    #[test]
    fn test_duration_matches_within_3min() {
        // Small TMDb runtime: 300s ± 10% = 270-330, but ±180s is more permissive
        // So 300 ± 180 = 120-480
        assert!(duration_matches(120, 300));
        assert!(duration_matches(480, 300));
        assert!(!duration_matches(119, 300));
        assert!(!duration_matches(481, 300));
    }

    #[test]
    fn test_duration_matches_uses_more_permissive_tolerance() {
        // 1000s: ±10% = 100s, ±3min = 180s → use 180s
        // 1000 ± 180 = 820-1180
        assert!(duration_matches(820, 1000));
        assert!(duration_matches(1180, 1000));
    }

    // =========================================================================
    // Helper function tests
    // =========================================================================

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3661), "1:01:01");
        assert_eq!(format_duration(7200), "2:00:00");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(61), "1:01");
        assert_eq!(format_duration(599), "9:59");
    }

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(0), "0:00");
    }

    #[test]
    fn test_compute_mode_single_value() {
        assert_eq!(compute_mode(&[5]), 5);
    }

    #[test]
    fn test_compute_mode_multiple_values() {
        assert_eq!(compute_mode(&[1, 2, 2, 3, 2]), 2);
    }

    #[test]
    fn test_compute_mode_tie_returns_any() {
        let result = compute_mode(&[1, 1, 2, 2]);
        assert!(result == 1 || result == 2);
    }

    #[test]
    fn test_compute_mode_empty() {
        assert_eq!(compute_mode(&[]), 0);
    }

    #[test]
    fn test_compute_median_odd_length() {
        let playlists = vec![
            make_playlist("00001", 1000, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 2000, 2, 2),
        ];
        let baseline: Vec<&Playlist> = playlists.iter().collect();
        assert_eq!(compute_median(&baseline), 1500);
    }

    #[test]
    fn test_compute_median_even_length() {
        let playlists = vec![
            make_playlist("00001", 1000, 2, 2),
            make_playlist("00002", 1500, 2, 2),
            make_playlist("00003", 2000, 2, 2),
            make_playlist("00004", 2500, 2, 2),
        ];
        let baseline: Vec<&Playlist> = playlists.iter().collect();
        assert_eq!(compute_median(&baseline), 1750);
    }

    // =========================================================================
    // Edge cases
    // =========================================================================

    #[test]
    fn test_two_baseline_playlists_minimum() {
        let playlists = vec![
            make_playlist("00001", 1500, 2, 2),
            make_playlist("00002", 1500, 2, 2),
        ];
        let results = run_detection(&playlists, 900, None, None);
        // Should have enough baseline to detect
        assert_eq!(results[0].suggested_type, SuggestedType::Episode);
        assert_eq!(results[0].confidence, Confidence::Medium);
        assert!(!results[0].reasons[0].contains("insufficient baseline"));
    }

    #[test]
    fn test_multi_episode_not_bumped_by_streams() {
        // Multi-episode detection should not be overridden by stream analysis
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 5500, 2, 1), // long + fewer streams
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = results.iter().find(|r| r.playlist_num == "00003").unwrap();
        assert_eq!(r.suggested_type, SuggestedType::MultiEpisode);
    }

    #[test]
    fn test_empty_playlists() {
        let results = run_detection(&[], 900, None, None);
        assert!(results.is_empty());
    }

    #[test]
    fn test_format_duration_edge_cases() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(59), "0:59");
        assert_eq!(format_duration(60), "1:00");
        assert_eq!(format_duration(3600), "1:00:00");
        assert_eq!(format_duration(3661), "1:01:01");
    }
}
