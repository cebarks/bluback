# Auto Episode/Special Detection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automatically detect specials and multi-episode playlists using duration/stream/chapter heuristics and TMDb runtime matching, presenting results with confidence indicators in the TUI and CLI.

**Architecture:** New `src/detection.rs` module with a pure function that takes all playlists + optional TMDb data and returns per-playlist detection results. Detection runs after scan (Layer 1 only) and again after TMDb lookup (Layer 1 + Layer 2). Results are stored in `WizardState`/`DiscState` and rendered as indicators in the Playlist Manager. High-confidence specials are pre-marked; `A` keybind batch-accepts suggestions.

**Tech Stack:** Rust, ratatui (TUI rendering), clap (CLI flags), serde/toml (config)

**Spec:** `docs/superpowers/specs/2026-04-03-auto-detection-design.md`

---

### Task 1: Data Types and Detection Module Skeleton

**Files:**
- Create: `src/detection.rs`
- Modify: `src/main.rs` (add `mod detection;`)

- [ ] **Step 1: Write failing tests for Confidence ordering and DetectionResult construction**

In `src/detection.rs`:

```rust
use crate::types::{Episode, Playlist};
use std::collections::HashMap;

/// Confidence level for a detection result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// What the detector thinks a playlist is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestedType {
    Episode,
    Special,
    MultiEpisode,
}

/// Detection result for a single playlist.
#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub playlist_num: String,
    pub suggested_type: SuggestedType,
    pub confidence: Confidence,
    /// Human-readable reasons (e.g., "duration 3:12 vs 44:30 median", "2 audio tracks vs 6 typical")
    pub reasons: Vec<String>,
}

/// Run detection heuristics on all playlists.
///
/// `all_playlists`: every playlist on the disc (unfiltered).
/// `min_duration`: threshold for "episode-length" playlists (used to compute baselines).
/// `tmdb_episodes`: regular-season episodes from TMDb (optional).
/// `tmdb_specials`: season 0 episodes from TMDb (optional).
pub fn run_detection(
    all_playlists: &[Playlist],
    min_duration: u32,
    tmdb_episodes: Option<&[Episode]>,
    tmdb_specials: Option<&[Episode]>,
) -> Vec<DetectionResult> {
    let mut results = run_heuristics(all_playlists, min_duration);
    if tmdb_episodes.is_some() || tmdb_specials.is_some() {
        apply_tmdb_layer(&mut results, all_playlists, tmdb_episodes, tmdb_specials);
    }
    results
}

fn run_heuristics(_all_playlists: &[Playlist], _min_duration: u32) -> Vec<DetectionResult> {
    Vec::new() // Skeleton — implemented in Task 2
}

fn apply_tmdb_layer(
    _results: &mut [DetectionResult],
    _all_playlists: &[Playlist],
    _tmdb_episodes: Option<&[Episode]>,
    _tmdb_specials: Option<&[Episode]>,
) {
    // Skeleton — implemented in Task 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_ordering() {
        assert!(Confidence::Low < Confidence::Medium);
        assert!(Confidence::Medium < Confidence::High);
    }

    #[test]
    fn detection_result_construction() {
        let r = DetectionResult {
            playlist_num: "00001".into(),
            suggested_type: SuggestedType::Special,
            confidence: Confidence::High,
            reasons: vec!["duration 3:12 vs 44:30 median".into()],
        };
        assert_eq!(r.confidence, Confidence::High);
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.reasons.len(), 1);
    }
}
```

- [ ] **Step 2: Register the module**

In `src/main.rs`, add `mod detection;` alongside the other module declarations. Find the existing `mod` block (near the top) and add:

```rust
mod detection;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test detection::tests -v`
Expected: 2 tests PASS

- [ ] **Step 4: Commit**

```
feat: add detection module skeleton with types
```

---

### Task 2: Layer 1 — Disc Heuristics

**Files:**
- Modify: `src/detection.rs`

- [ ] **Step 1: Write failing tests for duration heuristic**

Add to `src/detection.rs` tests module:

```rust
    fn make_playlist(num: &str, seconds: u32, audio: u32, subtitle: u32) -> Playlist {
        Playlist {
            num: num.into(),
            duration: String::new(),
            seconds,
            video_streams: 1,
            audio_streams: audio,
            subtitle_streams: subtitle,
        }
    }

    #[test]
    fn duration_high_confidence_special() {
        // 3 min playlist among 45 min episodes → < 50% median → high special
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 2700, 6, 4),
            make_playlist("00004", 180, 6, 4), // 3 min
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = results.iter().find(|r| r.playlist_num == "00004").unwrap();
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::High);
    }

    #[test]
    fn duration_medium_confidence_special() {
        // 25 min playlist among 45 min episodes → 55% of median → medium special
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 2700, 6, 4),
            make_playlist("00004", 1500, 6, 4), // 25 min
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = results.iter().find(|r| r.playlist_num == "00004").unwrap();
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::Medium);
    }

    #[test]
    fn duration_multi_episode() {
        // 90 min playlist among 45 min episodes → > 200% median → multi-episode
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 5400, 6, 4), // 90 min
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = results.iter().find(|r| r.playlist_num == "00003").unwrap();
        assert_eq!(r.suggested_type, SuggestedType::MultiEpisode);
        assert_eq!(r.confidence, Confidence::High);
    }

    #[test]
    fn uniform_durations_no_detection() {
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 2800, 6, 4),
        ];
        let results = run_detection(&playlists, 900, None, None);
        for r in &results {
            assert_eq!(r.suggested_type, SuggestedType::Episode);
        }
    }

    #[test]
    fn single_playlist_no_detection() {
        let playlists = vec![make_playlist("00001", 2700, 6, 4)];
        let results = run_detection(&playlists, 900, None, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].suggested_type, SuggestedType::Episode);
    }

    #[test]
    fn below_min_duration_evaluated_against_baseline() {
        // Playlist below min_duration should still get detected as special
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 120, 6, 4), // 2 min, below min_duration=900
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = results.iter().find(|r| r.playlist_num == "00003").unwrap();
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::High);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test detection::tests -v`
Expected: FAIL — `run_heuristics` returns empty vec

- [ ] **Step 3: Implement duration heuristic**

Replace the `run_heuristics` function in `src/detection.rs`:

```rust
fn run_heuristics(all_playlists: &[Playlist], min_duration: u32) -> Vec<DetectionResult> {
    // Compute baselines from episode-length playlists only
    let baseline_playlists: Vec<&Playlist> = all_playlists
        .iter()
        .filter(|pl| pl.seconds >= min_duration)
        .collect();

    let median_secs = compute_median(&baseline_playlists);

    all_playlists
        .iter()
        .map(|pl| {
            let mut suggested_type = SuggestedType::Episode;
            let mut confidence = Confidence::Low;
            let mut reasons = Vec::new();

            if median_secs > 0 && baseline_playlists.len() >= 2 {
                let ratio = pl.seconds as f64 / median_secs as f64;
                if ratio < 0.5 {
                    suggested_type = SuggestedType::Special;
                    confidence = Confidence::High;
                    reasons.push(format!(
                        "duration {} vs {} median ({:.0}%)",
                        format_duration(pl.seconds),
                        format_duration(median_secs),
                        ratio * 100.0
                    ));
                } else if ratio < 0.75 {
                    suggested_type = SuggestedType::Special;
                    confidence = Confidence::Medium;
                    reasons.push(format!(
                        "duration {} vs {} median ({:.0}%)",
                        format_duration(pl.seconds),
                        format_duration(median_secs),
                        ratio * 100.0
                    ));
                } else if ratio > 2.0 {
                    suggested_type = SuggestedType::MultiEpisode;
                    confidence = Confidence::High;
                    reasons.push(format!(
                        "duration {} vs {} median ({:.0}%)",
                        format_duration(pl.seconds),
                        format_duration(median_secs),
                        ratio * 100.0
                    ));
                }
            }

            DetectionResult {
                playlist_num: pl.num.clone(),
                suggested_type,
                confidence,
                reasons,
            }
        })
        .collect()
}

fn compute_median(playlists: &[&Playlist]) -> u32 {
    let mut durations: Vec<u32> = playlists.iter().map(|pl| pl.seconds).collect();
    durations.sort();
    if durations.is_empty() {
        0
    } else {
        durations[durations.len() / 2]
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
```

- [ ] **Step 4: Run tests to verify duration tests pass**

Run: `cargo test detection::tests -v`
Expected: All PASS

- [ ] **Step 5: Write failing tests for stream count and chapter count heuristics**

Add to tests module:

```rust
    #[test]
    fn stream_count_bumps_confidence() {
        // Playlist with 2 audio tracks among 6-track episodes → bump
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 2700, 6, 4),
            make_playlist("00004", 1800, 2, 4), // 1800s = 67% of median → medium, but fewer audio → high
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = results.iter().find(|r| r.playlist_num == "00004").unwrap();
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::High);
    }

    #[test]
    fn stream_count_alone_flags_low() {
        // Same duration but fewer streams → low confidence special
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 2700, 2, 1), // same duration, fewer streams
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = results.iter().find(|r| r.playlist_num == "00003").unwrap();
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::Low);
    }

    #[test]
    fn identical_stream_counts_no_bump() {
        // All playlists have same stream counts → no stream-based detection
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 2700, 6, 4),
        ];
        let results = run_detection(&playlists, 900, None, None);
        for r in &results {
            assert_eq!(r.suggested_type, SuggestedType::Episode);
        }
    }

    #[test]
    fn confidence_caps_at_high() {
        // Duration high + stream bump should still be High (not overflow)
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 180, 1, 0), // extremely short + minimal streams
        ];
        let results = run_detection(&playlists, 900, None, None);
        let r = results.iter().find(|r| r.playlist_num == "00003").unwrap();
        assert_eq!(r.confidence, Confidence::High);
    }
```

- [ ] **Step 6: Implement stream count and chapter count heuristics**

Add helper functions and modify `run_heuristics` to apply stream/chapter bumps after the initial duration pass. Add these helpers above `run_heuristics`:

```rust
fn compute_mode(values: &[u32]) -> u32 {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for &v in values {
        *counts.entry(v).or_insert(0) += 1;
    }
    counts.into_iter().max_by_key(|&(_, c)| c).map(|(v, _)| v).unwrap_or(0)
}

fn bump_confidence(current: Confidence) -> Confidence {
    match current {
        Confidence::Low => Confidence::Medium,
        Confidence::Medium | Confidence::High => Confidence::High,
    }
}
```

Then at the end of the `run_heuristics` function, after the `.map()` that builds initial results but before `.collect()`, add stream/chapter analysis. The cleanest approach: collect results first, then do a second pass.

Replace the return of `run_heuristics` to do two passes:

```rust
fn run_heuristics(all_playlists: &[Playlist], min_duration: u32) -> Vec<DetectionResult> {
    let baseline_playlists: Vec<&Playlist> = all_playlists
        .iter()
        .filter(|pl| pl.seconds >= min_duration)
        .collect();

    let median_secs = compute_median(&baseline_playlists);

    // Compute mode stream counts from baseline playlists
    let mode_audio = compute_mode(
        &baseline_playlists.iter().map(|pl| pl.audio_streams).collect::<Vec<_>>(),
    );
    let mode_subtitle = compute_mode(
        &baseline_playlists.iter().map(|pl| pl.subtitle_streams).collect::<Vec<_>>(),
    );

    let mut results: Vec<DetectionResult> = all_playlists
        .iter()
        .map(|pl| {
            let mut suggested_type = SuggestedType::Episode;
            let mut confidence = Confidence::Low;
            let mut reasons = Vec::new();

            if median_secs > 0 && baseline_playlists.len() >= 2 {
                let ratio = pl.seconds as f64 / median_secs as f64;
                if ratio < 0.5 {
                    suggested_type = SuggestedType::Special;
                    confidence = Confidence::High;
                    reasons.push(format!(
                        "duration {} vs {} median ({:.0}%)",
                        format_duration(pl.seconds),
                        format_duration(median_secs),
                        ratio * 100.0,
                    ));
                } else if ratio < 0.75 {
                    suggested_type = SuggestedType::Special;
                    confidence = Confidence::Medium;
                    reasons.push(format!(
                        "duration {} vs {} median ({:.0}%)",
                        format_duration(pl.seconds),
                        format_duration(median_secs),
                        ratio * 100.0,
                    ));
                } else if ratio > 2.0 {
                    suggested_type = SuggestedType::MultiEpisode;
                    confidence = Confidence::High;
                    reasons.push(format!(
                        "duration {} vs {} median ({:.0}%)",
                        format_duration(pl.seconds),
                        format_duration(median_secs),
                        ratio * 100.0,
                    ));
                }
            }

            DetectionResult {
                playlist_num: pl.num.clone(),
                suggested_type,
                confidence,
                reasons,
            }
        })
        .collect();

    // Second pass: stream count analysis
    if mode_audio > 0 {
        for (i, pl) in all_playlists.iter().enumerate() {
            let r = &mut results[i];
            // Only bump toward special, not away from multi-episode
            if r.suggested_type == SuggestedType::MultiEpisode {
                continue;
            }
            let audio_low = pl.audio_streams > 0 && pl.audio_streams * 2 < mode_audio;
            let subtitle_low = mode_subtitle > 0
                && pl.subtitle_streams * 2 < mode_subtitle;
            if audio_low || subtitle_low {
                if r.suggested_type == SuggestedType::Episode {
                    r.suggested_type = SuggestedType::Special;
                }
                r.confidence = bump_confidence(r.confidence);
                let mut parts = Vec::new();
                if audio_low {
                    parts.push(format!("{}/{} audio tracks", pl.audio_streams, mode_audio));
                }
                if subtitle_low {
                    parts.push(format!("{}/{} subtitle tracks", pl.subtitle_streams, mode_subtitle));
                }
                r.reasons.push(parts.join(", "));
            }
        }
    }

    results
}
```

- [ ] **Step 7: Run tests to verify all pass**

Run: `cargo test detection::tests -v`
Expected: All PASS

- [ ] **Step 8: Write and run chapter count heuristic tests**

Add to tests module:

```rust
    #[test]
    fn chapter_count_bumps_confidence() {
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 1800, 6, 4), // 67% → medium special
        ];
        // Playlist 3 has 0 chapters vs mode of 8
        let chapter_counts: HashMap<String, usize> =
            [("00001", 8), ("00002", 8), ("00003", 0)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
        let results = run_detection_with_chapters(&playlists, 900, None, None, &chapter_counts);
        let r = results.iter().find(|r| r.playlist_num == "00003").unwrap();
        assert_eq!(r.confidence, Confidence::High); // medium bumped to high
    }
```

For chapter counts, we need to extend the function signature. To avoid changing the public API, add a `run_detection_with_chapters` that `run_detection` delegates to:

```rust
pub fn run_detection(
    all_playlists: &[Playlist],
    min_duration: u32,
    tmdb_episodes: Option<&[Episode]>,
    tmdb_specials: Option<&[Episode]>,
) -> Vec<DetectionResult> {
    run_detection_with_chapters(all_playlists, min_duration, tmdb_episodes, tmdb_specials, &HashMap::new())
}

pub fn run_detection_with_chapters(
    all_playlists: &[Playlist],
    min_duration: u32,
    tmdb_episodes: Option<&[Episode]>,
    tmdb_specials: Option<&[Episode]>,
    chapter_counts: &HashMap<String, usize>,
) -> Vec<DetectionResult> {
    let mut results = run_heuristics(all_playlists, min_duration, chapter_counts);
    if tmdb_episodes.is_some() || tmdb_specials.is_some() {
        apply_tmdb_layer(&mut results, all_playlists, tmdb_episodes, tmdb_specials);
    }
    results
}
```

Update `run_heuristics` signature to take `chapter_counts: &HashMap<String, usize>` and add a third pass after the stream count pass:

```rust
    // Third pass: chapter count analysis
    if !chapter_counts.is_empty() {
        let baseline_chapters: Vec<u32> = baseline_playlists
            .iter()
            .filter_map(|pl| chapter_counts.get(&pl.num).map(|&c| c as u32))
            .collect();
        let mode_chapters = compute_mode(&baseline_chapters);

        if mode_chapters > 0 {
            for (i, pl) in all_playlists.iter().enumerate() {
                let r = &mut results[i];
                if r.suggested_type == SuggestedType::MultiEpisode {
                    continue;
                }
                let pl_chapters = chapter_counts.get(&pl.num).copied().unwrap_or(0);
                if (pl_chapters as u32) * 2 < mode_chapters || pl_chapters == 0 {
                    if r.suggested_type == SuggestedType::Episode {
                        r.suggested_type = SuggestedType::Special;
                    }
                    r.confidence = bump_confidence(r.confidence);
                    r.reasons.push(format!(
                        "{}/{} chapters",
                        pl_chapters, mode_chapters
                    ));
                }
            }
        }
    }
```

- [ ] **Step 9: Run tests**

Run: `cargo test detection::tests -v`
Expected: All PASS

- [ ] **Step 10: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 11: Commit**

```
feat: implement Layer 1 disc heuristics (duration, streams, chapters)
```

---

### Task 3: Layer 2 — TMDb Runtime Matching

**Files:**
- Modify: `src/detection.rs`

- [ ] **Step 1: Write failing tests for TMDb runtime matching**

Add to tests module:

```rust
    fn make_episode(num: u32, runtime: Option<u32>) -> Episode {
        Episode {
            episode_number: num,
            name: format!("Episode {}", num),
            runtime,
        }
    }

    #[test]
    fn tmdb_matching_clears_special_flag() {
        // Playlist at 60% of median → medium special from heuristics,
        // but matches a TMDb episode runtime → should be cleared to Episode
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 1700, 6, 4), // 63% of median → medium special
        ];
        let episodes = vec![
            make_episode(1, Some(45)),
            make_episode(2, Some(45)),
            make_episode(3, Some(28)), // 28 min ≈ 1680s, within ±10% of 1700
        ];
        let results = run_detection(&playlists, 900, Some(&episodes), None);
        let r = results.iter().find(|r| r.playlist_num == "00003").unwrap();
        assert_eq!(r.suggested_type, SuggestedType::Episode);
    }

    #[test]
    fn tmdb_no_match_boosts_special() {
        // Playlist that doesn't match any TMDb runtime → boost special confidence
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 600, 6, 4), // 10 min, already high special
        ];
        let episodes = vec![
            make_episode(1, Some(45)),
            make_episode(2, Some(45)),
        ];
        let results = run_detection(&playlists, 900, Some(&episodes), None);
        let r = results.iter().find(|r| r.playlist_num == "00003").unwrap();
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::High);
    }

    #[test]
    fn tmdb_season0_match_boosts_special() {
        // Playlist matches a season 0 (specials) runtime → boost toward special
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 1800, 6, 4), // 30 min = medium special (67%)
        ];
        let episodes = vec![
            make_episode(1, Some(45)),
            make_episode(2, Some(45)),
        ];
        let specials = vec![
            make_episode(1, Some(30)), // 30 min ≈ 1800s
        ];
        let results = run_detection(&playlists, 900, Some(&episodes), Some(&specials));
        let r = results.iter().find(|r| r.playlist_num == "00003").unwrap();
        assert_eq!(r.suggested_type, SuggestedType::Special);
        assert_eq!(r.confidence, Confidence::High);
    }

    #[test]
    fn tmdb_with_no_runtimes_is_noop() {
        // TMDb data without runtime info should not change results
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 2700, 6, 4),
            make_playlist("00003", 600, 6, 4),
        ];
        let episodes = vec![
            make_episode(1, None),
            make_episode(2, None),
        ];
        let results_without = run_detection(&playlists, 900, None, None);
        let results_with = run_detection(&playlists, 900, Some(&episodes), None);
        let r1 = results_without.iter().find(|r| r.playlist_num == "00003").unwrap();
        let r2 = results_with.iter().find(|r| r.playlist_num == "00003").unwrap();
        assert_eq!(r1.confidence, r2.confidence);
        assert_eq!(r1.suggested_type, r2.suggested_type);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test detection::tests -v`
Expected: FAIL — `apply_tmdb_layer` is a no-op

- [ ] **Step 3: Implement TMDb runtime matching**

Replace `apply_tmdb_layer` in `src/detection.rs`:

```rust
fn apply_tmdb_layer(
    results: &mut [DetectionResult],
    all_playlists: &[Playlist],
    tmdb_episodes: Option<&[Episode]>,
    tmdb_specials: Option<&[Episode]>,
) {
    // Collect TMDb runtimes in seconds
    let episode_runtimes: Vec<u32> = tmdb_episodes
        .unwrap_or(&[])
        .iter()
        .filter_map(|ep| ep.runtime.map(|r| r * 60))
        .collect();

    let special_runtimes: Vec<u32> = tmdb_specials
        .unwrap_or(&[])
        .iter()
        .filter_map(|ep| ep.runtime.map(|r| r * 60))
        .collect();

    if episode_runtimes.is_empty() && special_runtimes.is_empty() {
        return;
    }

    for (i, pl) in all_playlists.iter().enumerate() {
        let r = &mut results[i];

        let matches_regular = episode_runtimes.iter().any(|&rt| {
            duration_matches(pl.seconds, rt)
        });
        let matches_special = special_runtimes.iter().any(|&rt| {
            duration_matches(pl.seconds, rt)
        });

        if matches_regular && r.suggested_type == SuggestedType::Special {
            // Regular episode match clears special flag
            r.suggested_type = SuggestedType::Episode;
            r.confidence = Confidence::High;
            r.reasons.push("matches TMDb episode runtime".into());
        } else if matches_special {
            // Season 0 match boosts special confidence
            if r.suggested_type != SuggestedType::MultiEpisode {
                r.suggested_type = SuggestedType::Special;
                r.confidence = bump_confidence(r.confidence);
                r.reasons.push("matches TMDb special runtime".into());
            }
        } else if !episode_runtimes.is_empty()
            && r.suggested_type == SuggestedType::Special
        {
            // No TMDb match at all — doesn't change confidence but adds reason
            r.reasons.push("no matching TMDb runtime".into());
        }
    }
}

/// Check if playlist duration matches a TMDb runtime within tolerance.
/// Tolerance: ±10% or ±3 minutes (180s), whichever is more permissive.
fn duration_matches(playlist_secs: u32, tmdb_secs: u32) -> bool {
    if tmdb_secs == 0 {
        return false;
    }
    let diff = (playlist_secs as i64 - tmdb_secs as i64).unsigned_abs();
    let pct_threshold = (tmdb_secs as f64 * 0.10) as u64;
    let abs_threshold = 180u64; // 3 minutes
    diff <= pct_threshold.max(abs_threshold)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test detection::tests -v`
Expected: All PASS

- [ ] **Step 5: Write edge case tests**

Add to tests:

```rust
    #[test]
    fn duration_matches_tolerance() {
        // ±10% of 2700s = 270s, ±3min = 180s → 270s is more permissive
        assert!(duration_matches(2700, 2700)); // exact
        assert!(duration_matches(2700, 2500)); // 200s diff < 270s
        assert!(!duration_matches(2700, 2400)); // 300s diff > 270s

        // ±10% of 600s = 60s, ±3min = 180s → 180s is more permissive
        assert!(duration_matches(600, 750)); // 150s diff < 180s
        assert!(!duration_matches(600, 800)); // 200s diff > 180s
    }

    #[test]
    fn empty_playlists() {
        let results = run_detection(&[], 900, None, None);
        assert!(results.is_empty());
    }
```

- [ ] **Step 6: Run all tests and clippy**

Run: `cargo test detection::tests -v && cargo clippy -- -D warnings`
Expected: All PASS, no warnings

- [ ] **Step 7: Commit**

```
feat: implement Layer 2 TMDb runtime matching for detection
```

---

### Task 4: TMDb Season 0 Fetch

**Files:**
- Modify: `src/tmdb.rs`
- Modify: `src/tui/wizard.rs` (season fetch call site)
- Modify: `src/tui/mod.rs` (`TmdbState` — add specials field)

- [ ] **Step 1: Write failing test for get_specials**

Add to `src/tmdb.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_season_returns_episodes_type() {
        // This just verifies the function signature and Episode type compatibility
        // Actual network tests are in tests/tmdb_parsing.rs with fixtures
        let episodes: Vec<Episode> = Vec::new();
        assert!(episodes.is_empty());
    }
}
```

Note: `get_season` already works for season 0 — the TMDb API path `/tv/{id}/season/0` returns specials. No new function needed; we just call `get_season(show_id, 0, api_key)`.

- [ ] **Step 2: Add `specials` field to `TmdbState`**

In `src/tui/mod.rs`, add to `TmdbState`:

```rust
pub specials: Vec<Episode>,
```

- [ ] **Step 3: Modify season fetch to also fetch season 0**

In `src/tui/wizard.rs`, find the season fetch code (around line 957). The current code spawns a single thread to fetch the season. We need to also fetch season 0 in the same thread. Modify the spawn block:

Currently at `src/tui/wizard.rs:957-961`:
```rust
std::thread::spawn(move || {
    let _ = tx.send(BackgroundResult::SeasonFetch(tmdb::get_season(
        show_id, season, &api_key,
    )));
});
```

Change to fetch both the requested season and season 0 (specials):
```rust
std::thread::spawn(move || {
    let regular = tmdb::get_season(show_id, season, &api_key);
    let specials = if season != 0 {
        tmdb::get_season(show_id, 0, &api_key).ok()
    } else {
        None
    };
    let _ = tx.send(BackgroundResult::SeasonFetch(regular, specials));
});
```

This requires updating the `BackgroundResult::SeasonFetch` variant to carry both. Find the `BackgroundResult` enum and update:

```rust
SeasonFetch(Result<Vec<Episode>>, Option<Vec<Episode>>),
```

Then update all match arms on `BackgroundResult::SeasonFetch` to destructure both fields. The handler that receives season results (search for `BackgroundResult::SeasonFetch`) needs to store specials:

```rust
BackgroundResult::SeasonFetch(Ok(episodes), specials) => {
    session.tmdb.episodes = episodes;
    session.tmdb.specials = specials.unwrap_or_default();
    // ... rest of existing handler
}
BackgroundResult::SeasonFetch(Err(e), _) => {
    // ... existing error handler
}
```

- [ ] **Step 4: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: Compiles successfully (or fix any match arm exhaustiveness errors)

- [ ] **Step 5: Commit**

```
feat: fetch TMDb season 0 (specials) alongside regular season
```

---

### Task 5: Config, CLI Flags, and Settings Panel

**Files:**
- Modify: `src/config.rs` (add `auto_detect` field, KNOWN_KEYS, `to_toml_string`, accessor)
- Modify: `src/main.rs` (add `--auto-detect` / `--no-auto-detect` flags)
- Modify: `src/types.rs` (`SettingsState::from_config_with_drives`, `to_config`, env var handling)

- [ ] **Step 1: Add `auto_detect` to Config struct**

In `src/config.rs`, add field to `Config` struct (after `batch`):

```rust
pub auto_detect: Option<bool>,
```

- [ ] **Step 2: Add accessor method**

In `src/config.rs`, add method to `impl Config` (near `batch()` around line 370):

```rust
pub fn auto_detect(&self) -> bool {
    self.auto_detect.unwrap_or(false)
}

pub fn should_auto_detect(&self, cli_auto_detect: Option<bool>) -> bool {
    cli_auto_detect.unwrap_or_else(|| self.auto_detect())
}
```

- [ ] **Step 3: Add to KNOWN_KEYS**

In `src/config.rs`, add `"auto_detect"` to the `KNOWN_KEYS` array (after `"batch"`).

- [ ] **Step 4: Add to `to_toml_string`**

In `src/config.rs`, in the `to_toml_string` method, add after the `emit_bool` for `batch`:

```rust
emit_bool(&mut out, "auto_detect", self.auto_detect, false);
```

- [ ] **Step 5: Add CLI flags to Args**

In `src/main.rs`, add to the `Args` struct (after the `--batch`/`--no-batch` flags):

```rust
/// Enable automatic episode/special detection heuristics
#[arg(long, conflicts_with_all = ["no_auto_detect", "movie"])]
auto_detect: bool,

/// Disable auto-detection (overrides config)
#[arg(long, conflicts_with = "auto_detect")]
no_auto_detect: bool,
```

- [ ] **Step 6: Add Settings Panel toggle**

In `src/types.rs`, in `from_config_with_drives`, add a new `SettingItem::Toggle` after the "Show Filtered" toggle (around line 720):

```rust
SettingItem::Toggle {
    label: "Auto-Detect Episodes/Specials".into(),
    key: "auto_detect".into(),
    value: config.auto_detect.unwrap_or(false),
},
```

- [ ] **Step 7: Add `to_config` mapping**

In `src/types.rs`, in the `to_config` method's `Toggle` match arm (around line 1103), add:

```rust
"auto_detect" if *value => config.auto_detect = Some(true),
```

- [ ] **Step 8: Add env var support**

In `src/types.rs`, find the env var override arrays. Add to the mapping array (around line 912):

```rust
("BLUBACK_AUTO_DETECT", "auto_detect"),
```

And in the env var application section (around line 1023), add:

```rust
("BLUBACK_AUTO_DETECT", "auto_detect"),
```

And in the env var application logic for toggles (around line 1106):

```rust
"auto_detect" if *value => config.auto_detect = Some(true),
```

- [ ] **Step 9: Write config round-trip test**

Add to `src/config.rs` tests:

```rust
#[test]
fn auto_detect_config_roundtrip() {
    let mut config = Config::default();
    config.auto_detect = Some(true);
    let toml_str = config.to_toml_string();
    assert!(toml_str.contains("auto_detect = true"));

    let default_config = Config::default();
    let default_toml = default_config.to_toml_string();
    assert!(default_toml.contains("# auto_detect = false"));
}
```

- [ ] **Step 10: Run tests and clippy**

Run: `cargo test -v 2>&1 | tail -20 && cargo clippy -- -D warnings`
Expected: All PASS, no warnings

- [ ] **Step 11: Commit**

```
feat: add auto_detect config, CLI flags, and settings panel toggle
```

---

### Task 6: Wire Detection into TUI Flow

**Files:**
- Modify: `src/tui/mod.rs` (`WizardState` — add detection_results field)
- Modify: `src/tui/wizard.rs` (call detection after scan, after TMDb, render indicators, `A` keybind)

- [ ] **Step 1: Add detection results to WizardState**

In `src/tui/mod.rs`, add to `WizardState`:

```rust
pub detection_results: Vec<crate::detection::DetectionResult>,
```

- [ ] **Step 2: Call detection after episode assignment**

In `src/tui/wizard.rs`, find where `assign_episodes` is called and the screen transitions to `PlaylistManager` (around line 969). After the `assign_episodes` call, add detection:

```rust
// Run auto-detection if enabled
if session.config.auto_detect() {
    session.wizard.detection_results = crate::detection::run_detection_with_chapters(
        &session.disc.playlists,
        session.config.min_duration.unwrap_or(crate::config::DEFAULT_MIN_DURATION),
        if session.tmdb.episodes.is_empty() { None } else { Some(&session.tmdb.episodes) },
        if session.tmdb.specials.is_empty() { None } else { Some(&session.tmdb.specials) },
        &session.disc.chapter_counts,
    );
    // Pre-mark high-confidence specials
    for det in &session.wizard.detection_results {
        if det.suggested_type == crate::detection::SuggestedType::Special
            && det.confidence == crate::detection::Confidence::High
        {
            let pl_num = &det.playlist_num;
            if !session.wizard.specials.contains(pl_num) {
                // Auto-mark as special with next SP number
                let max_sp = session.wizard.specials.iter()
                    .filter_map(|snum| {
                        session.wizard.episode_assignments.get(snum)
                            .and_then(|eps| eps.first())
                            .map(|e| e.episode_number)
                    })
                    .max()
                    .unwrap_or(0);
                session.wizard.specials.insert(pl_num.clone());
                session.wizard.episode_assignments.insert(
                    pl_num.clone(),
                    vec![crate::types::Episode {
                        episode_number: max_sp + 1,
                        name: String::new(),
                        runtime: None,
                    }],
                );
                // Auto-select the playlist
                if let Some(real_idx) = session.disc.playlists.iter().position(|p| p.num == *pl_num) {
                    if let Some(sel) = session.wizard.playlist_selected.get_mut(real_idx) {
                        *sel = true;
                    }
                }
            }
        }
    }
}
```

There are multiple places where the screen transitions to `PlaylistManager` (after TMDb, after season fetch, in movie mode). Find all transition points where `assign_episodes` is called or where the screen changes to `PlaylistManager` and ensure detection runs. The main call sites are:

1. After season fetch result (around line 969) — shown above
2. Movie mode transition to PlaylistManager (around line 1502) — skip detection (movie mode)
3. TMDb skip (Esc) transition (around line 1387) — same pattern as #1

For the TMDb skip path (line ~1387), add the same detection block.

- [ ] **Step 3: Add `A` keybind for batch accept**

In `src/tui/wizard.rs`, in the `PlaylistManager` key handler section (near the other keybinds around line 1167), add:

```rust
KeyCode::Char('A') if !session.tmdb.movie_mode => {
    // Accept all high and medium detection suggestions
    for det in &session.wizard.detection_results {
        if det.suggested_type == crate::detection::SuggestedType::Special
            && det.confidence >= crate::detection::Confidence::Medium
            && !session.wizard.specials.contains(&det.playlist_num)
        {
            let pl_num = det.playlist_num.clone();
            let max_sp = session.wizard.specials.iter()
                .filter_map(|snum| {
                    session.wizard.episode_assignments.get(snum)
                        .and_then(|eps| eps.first())
                        .map(|e| e.episode_number)
                })
                .max()
                .unwrap_or(0);
            session.wizard.specials.insert(pl_num.clone());
            session.wizard.episode_assignments.insert(
                pl_num.clone(),
                vec![crate::types::Episode {
                    episode_number: max_sp + 1,
                    name: String::new(),
                    runtime: None,
                }],
            );
            if let Some(real_idx) = session.disc.playlists.iter().position(|p| p.num == pl_num) {
                if let Some(sel) = session.wizard.playlist_selected.get_mut(real_idx) {
                    *sel = true;
                }
            }
        }
    }
}
```

- [ ] **Step 4: Render confidence indicators in Playlist Manager**

In `src/tui/wizard.rs`, in the playlist row rendering code (around line 390-475), add a detection indicator column. Find where `special_marker` is built (line 474) and replace it with detection-aware logic:

```rust
let detection_indicator = session
    .wizard
    .detection_results
    .iter()
    .find(|d| d.playlist_num == pl.num)
    .map(|d| match (d.suggested_type, d.confidence) {
        (crate::detection::SuggestedType::Special, crate::detection::Confidence::High) => "[S!]",
        (crate::detection::SuggestedType::Special, crate::detection::Confidence::Medium) => "[S?]",
        (crate::detection::SuggestedType::Special, crate::detection::Confidence::Low) => "[s.]",
        (crate::detection::SuggestedType::MultiEpisode, crate::detection::Confidence::High) => "[M!]",
        (crate::detection::SuggestedType::MultiEpisode, _) => "[M?]",
        _ => "",
    })
    .unwrap_or("");

let special_marker = if is_special && detection_indicator.is_empty() {
    " [SP]"
} else if is_special {
    " [SP]"
} else {
    detection_indicator
};
```

Apply color to the indicator based on confidence. In the `Span` or `Cell` construction for the episode column, use:

```rust
let indicator_style = session
    .wizard
    .detection_results
    .iter()
    .find(|d| d.playlist_num == pl.num)
    .map(|d| match d.confidence {
        crate::detection::Confidence::High => Style::default().fg(Color::Yellow),
        crate::detection::Confidence::Medium => Style::default().fg(Color::DarkGray),
        crate::detection::Confidence::Low => Style::default().fg(Color::DarkGray),
    })
    .unwrap_or_default();
```

- [ ] **Step 5: Add detection reason to status line**

Find the status line / help text area in the Playlist Manager render function. When the cursor is on a row with a detection result, show the reason. Add after the existing key hint rendering:

```rust
if let Some(det) = session.wizard.detection_results.iter().find(|d| {
    visible.get(session.wizard.list_cursor)
        .map(|&(real_idx, _)| session.disc.playlists[real_idx].num == d.playlist_num)
        .unwrap_or(false)
}) {
    if !det.reasons.is_empty() {
        // Render reason string below the key hints
        let reason_text = det.reasons.join(", ");
        // Add as a line in the help/status area
    }
}
```

The exact rendering depends on how the current status area is structured — integrate with the existing pattern.

- [ ] **Step 6: Add `A` to key hints**

In the key hints rendering for the Playlist Manager, add `A:accept suggestions` when detection results contain any medium+ suggestions that aren't already accepted.

- [ ] **Step 7: Auto-show detected playlists below min_duration**

Per the spec: "When auto-detection flags specials among playlists below `min_duration`, those playlists are automatically shown in the Playlist Manager."

In the Playlist Manager visibility filter (around `src/tui/wizard.rs:323`), the current filter is:

```rust
.filter(|(_, pl)| view.show_filtered || view.episodes_pl.iter().any(|ep| ep.num == pl.num))
```

Extend it to also show playlists with detection results:

```rust
.filter(|(_, pl)| {
    view.show_filtered
        || view.episodes_pl.iter().any(|ep| ep.num == pl.num)
        || session.wizard.detection_results.iter().any(|d| {
            d.playlist_num == pl.num
                && d.suggested_type != crate::detection::SuggestedType::Episode
                && d.confidence >= crate::detection::Confidence::Medium
        })
})
```

This ensures detected specials are visible even if they're below `min_duration`, without enabling `show_filtered` globally.

- [ ] **Step 8: Build and verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: Compiles

- [ ] **Step 9: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 10: Commit**

```
feat: wire detection into TUI with indicators and A keybind
```

---

### Task 7: Wire Detection into CLI Flow

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Resolve auto_detect from args + config**

In `src/cli.rs`, near the top of the `run()` function where other args are resolved, add:

```rust
let auto_detect = config.should_auto_detect(
    if args.auto_detect { Some(true) }
    else if args.no_auto_detect { Some(false) }
    else { None }
);
```

Note: `args` in CLI mode is passed from `main.rs` — ensure the `auto_detect` and `no_auto_detect` fields are accessible. They're defined on the `Args` struct in `main.rs` which is passed into `cli::run()`.

- [ ] **Step 2: Run detection after episode assignment in CLI**

Find where `assign_episodes` is called in `src/cli.rs` (there may be multiple call sites for headless vs interactive). After the assignment, if `auto_detect` is enabled:

```rust
if auto_detect && !args.movie {
    let detection_results = crate::detection::run_detection_with_chapters(
        &playlists,
        config.min_duration(args.min_duration),
        if episodes.is_empty() { None } else { Some(&episodes) },
        None, // CLI doesn't fetch season 0 yet
        &chapter_counts,
    );

    if headless {
        // Auto-apply high-confidence specials
        for det in &detection_results {
            if det.suggested_type == crate::detection::SuggestedType::Special
                && det.confidence == crate::detection::Confidence::High
                && !specials_set.contains(&det.playlist_num)
            {
                eprintln!(
                    "Auto-detected: playlist {} as special ({})",
                    det.playlist_num,
                    det.reasons.join(", ")
                );
                specials_set.insert(det.playlist_num.clone());
            }
        }
    } else {
        // Interactive: show detection results and prompt
        let has_suggestions = detection_results.iter().any(|d| {
            d.suggested_type == crate::detection::SuggestedType::Special
                && d.confidence >= crate::detection::Confidence::Medium
        });
        if has_suggestions {
            println!("\nAuto-detected specials:");
            for det in &detection_results {
                if det.suggested_type == crate::detection::SuggestedType::Special
                    && det.confidence >= crate::detection::Confidence::Medium
                {
                    let indicator = match det.confidence {
                        crate::detection::Confidence::High => "[S!]",
                        crate::detection::Confidence::Medium => "[S?]",
                        _ => "[s.]",
                    };
                    println!("  {} playlist {} — {}", indicator, det.playlist_num, det.reasons.join(", "));
                }
            }
            print!("Accept auto-detected specials? [Y/n/edit]: ");
            // Read input and handle Y/n/edit
            // Y: apply medium+ specials
            // n: skip
            // edit: fall through to manual --specials prompt (existing flow)
        }
    }
}
```

Note: `--specials` takes precedence — if `args.specials.is_some()`, skip auto-detection application entirely (the explicit flag wins).

- [ ] **Step 3: Run `cargo build` to verify**

Run: `cargo build 2>&1 | head -30`
Expected: Compiles

- [ ] **Step 4: Commit**

```
feat: wire detection into CLI with headless auto-apply
```

---

### Task 8: Integration with `--list-playlists`

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Add detection indicators to `--list-playlists` output**

Find the `--list-playlists` rendering code in `src/cli.rs`. When `auto_detect` is enabled, run detection and add an indicator column.

In the table header, add a "Det" column. For each playlist row, show the detection indicator:

```rust
if auto_detect {
    let detection_results = crate::detection::run_detection(
        &playlists,
        config.min_duration(args.min_duration),
        None,
        None,
    );
    // Add indicator to each row
    for (pl, det) in playlists.iter().zip(detection_results.iter()) {
        let indicator = match (det.suggested_type, det.confidence) {
            (crate::detection::SuggestedType::Special, crate::detection::Confidence::High) => "[S!]",
            (crate::detection::SuggestedType::Special, crate::detection::Confidence::Medium) => "[S?]",
            (crate::detection::SuggestedType::Special, crate::detection::Confidence::Low) => "[s.]",
            (crate::detection::SuggestedType::MultiEpisode, _) => "[M!]",
            _ => "    ",
        };
        // Include indicator in the row output
    }
}
```

The exact integration depends on the current table rendering code.

- [ ] **Step 2: Build and verify**

Run: `cargo build 2>&1 | head -30`
Expected: Compiles

- [ ] **Step 3: Commit**

```
feat: show detection indicators in --list-playlists output
```

---

### Task 9: Tests and Edge Cases

**Files:**
- Modify: `src/detection.rs` (add comprehensive edge case tests)

- [ ] **Step 1: Add edge case tests**

Add to `src/detection.rs` tests module:

```rust
    #[test]
    fn all_playlists_below_min_duration() {
        // No baseline playlists → no detection possible
        let playlists = vec![
            make_playlist("00001", 120, 6, 4),
            make_playlist("00002", 180, 6, 4),
        ];
        let results = run_detection(&playlists, 900, None, None);
        for r in &results {
            assert_eq!(r.suggested_type, SuggestedType::Episode);
        }
    }

    #[test]
    fn two_baseline_playlists_minimum() {
        // Need at least 2 baseline playlists for meaningful detection
        let playlists = vec![
            make_playlist("00001", 2700, 6, 4),
            make_playlist("00002", 120, 6, 4),
        ];
        // Only 1 baseline playlist → no duration detection
        let results = run_detection(&playlists, 900, None, None);
        let r = results.iter().find(|r| r.playlist_num == "00002").unwrap();
        assert_eq!(r.suggested_type, SuggestedType::Episode);
    }

    #[test]
    fn multi_episode_not_bumped_by_streams() {
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
    fn format_duration_output() {
        assert_eq!(format_duration(180), "3:00");
        assert_eq!(format_duration(2700), "45:00");
        assert_eq!(format_duration(3661), "1:01:01");
        assert_eq!(format_duration(0), "0:00");
    }

    #[test]
    fn compute_mode_basic() {
        assert_eq!(compute_mode(&[6, 6, 6, 2]), 6);
        assert_eq!(compute_mode(&[1, 2, 2, 3, 3, 3]), 3);
        assert_eq!(compute_mode(&[]), 0);
    }
```

- [ ] **Step 2: Run all tests**

Run: `cargo test -v 2>&1 | tail -30`
Expected: All PASS

- [ ] **Step 3: Run full clippy and fmt**

Run: `rustup run stable cargo fmt && cargo clippy -- -D warnings`
Expected: Clean

- [ ] **Step 4: Commit**

```
test: comprehensive detection edge case tests
```

---

### Task 10: Update CLAUDE.md and Documentation

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update CLAUDE.md**

Add `--auto-detect` / `--no-auto-detect` to the CLI flags section. Add `auto_detect` to the config description. Add a bullet to "Key Design Decisions" describing the detection system. Update the test count. Add `detection.rs` to the Architecture section. Add `A:accept suggestions` to the TUI keybindings.

- [ ] **Step 2: Commit**

```
docs: update CLAUDE.md with auto-detection feature
```
