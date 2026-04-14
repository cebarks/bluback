# min_duration Rework Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `min_duration` with detection-driven playlist classification: rename to `min_probe_duration` (30s junk filter), flip auto_detect default to true, remove `episodes_pl`, add S toggle to hide specials, add E popup for starting episode.

**Architecture:** The three jobs `min_duration` currently does (probe gate, display filter, detection baseline) are decoupled into three independent mechanisms: a configurable junk threshold (`min_probe_duration`), auto-detection (now default-on), and display toggles (`f` for junk, `S` for specials). The `episodes_pl` concept is removed entirely; `stream_infos` membership becomes the source of truth for "probed" status. A new phase 1.5 pre-classifies obvious specials (< 50% median duration) to skip probing them.

**Tech Stack:** Rust, ratatui, clap, ffmpeg-the-third

**Spec:** `docs/superpowers/specs/2026-04-14-min-duration-rework-design.md`

---

## File Map

| File | Changes |
|------|---------|
| `src/config.rs` | Rename constant/field/getter/validation, flip auto_detect default |
| `src/main.rs` | Rename CLI flag, add `--hide-specials` |
| `src/detection.rs` | Remove `min_duration` param, add `pre_classify_playlists()` |
| `src/media/probe.rs` | Add `auto_detect` param, phase 1.5 skip set in probe loop |
| `src/disc.rs` | Remove `filter_episodes()` |
| `src/tui/mod.rs` | Remove `episodes_pl` from DiscState, add fields to WizardState |
| `src/types.rs` | Remove `episodes_pl` from PlaylistView, `episodes_pl_count` from TmdbView, add `show_specials` to PlaylistView, update SettingsState |
| `src/session.rs` | Add `probed_playlists()`, rewrite `visible_playlists()`, remove `start_unfiltered_probe()`, update scan handler, movie_mode, empty guard, playlist_selected init, apply_linked_context |
| `src/tui/wizard.rs` | Rewrite `reassign_regular_episodes()`, `visible_playlists_view()`, row dimming, add S/E key handlers, start episode popup rendering, on-demand probe triggers |
| `src/cli.rs` | Rename min_duration usages, remove episodes_pl flow, add `--hide-specials`, update list_playlists/run |

---

### Task 1: Rename min_duration to min_probe_duration in config

**Files:**
- Modify: `src/config.rs`

This is the foundation — every subsequent task depends on the renamed constant and field.

- [ ] **Step 1: Update constant, field, and getter**

In `src/config.rs`:
- Line 19: Rename `DEFAULT_MIN_DURATION` to `DEFAULT_MIN_PROBE_DURATION`, change value from `900` to `30`
- Line 81: Rename field `min_duration` to `min_probe_duration`
- Lines 384-386: Rename getter `min_duration()` to `min_probe_duration()`, update references to the new constant and field name
- Lines 172-177: Update `to_toml_string()` to emit `min_probe_duration` with `DEFAULT_MIN_PROBE_DURATION`

- [ ] **Step 2: Update KNOWN_KEYS and validation**

- Line 544 in KNOWN_KEYS: Change `"min_duration"` to `"min_probe_duration"`
- Lines 600-603 in `validate_config()`: Update field name to `min_probe_duration` and reference `DEFAULT_MIN_PROBE_DURATION`

- [ ] **Step 3: Update all config tests**

Rename tests and update field references:
- `test_min_duration_default` → `test_min_probe_duration_default` (assert default is 30)
- `test_min_duration_config_overrides_default` → `test_min_probe_duration_config_overrides_default`
- `test_min_duration_cli_overrides_config` → `test_min_probe_duration_cli_overrides_config`
- `test_min_duration_cli_explicit_default_overrides_config` → `test_min_probe_duration_cli_explicit_default_overrides_config`
- `test_parse_min_duration` → `test_parse_min_probe_duration` (parse `min_probe_duration = 600`)
- `test_validate_min_duration_zero_warns` → `test_validate_min_probe_duration_zero_warns`
- Update any TOML string literals in tests from `min_duration` to `min_probe_duration`

- [ ] **Step 4: Run tests to verify**

Run: `cargo test config:: -- --test-threads=1`
Expected: All config tests pass with the renamed fields.

- [ ] **Step 5: Fix remaining compilation errors**

Run `cargo check` and fix all remaining references to the old names throughout the codebase. This will touch callers in `session.rs`, `cli.rs`, `tui/wizard.rs`, and `main.rs`. For now, just do direct renames (don't change logic yet):
- `config.min_duration(...)` → `config.min_probe_duration(...)`
- `args.min_duration` → `args.min_probe_duration`
- `DEFAULT_MIN_DURATION` → `DEFAULT_MIN_PROBE_DURATION`
- Any string literals referencing the old name in error messages

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass (some detection/session tests may need `min_duration` → `min_probe_duration` renames in their calls too).

- [ ] **Step 7: Commit**

Suggest: `refactor: rename min_duration to min_probe_duration and lower default to 30s`

---

### Task 2: Rename CLI flag and update settings panel

**Files:**
- Modify: `src/main.rs`
- Modify: `src/types.rs`

- [ ] **Step 1: Rename CLI flag in main.rs**

In `src/main.rs` at lines 57-59, change:
```rust
/// Min seconds to probe playlist (filters menu clips) [default: 30]
#[arg(long)]
min_probe_duration: Option<u32>,
```

Update all references to `args.min_duration` → `args.min_probe_duration` in main.rs (the field name derives from the flag name).

- [ ] **Step 2: Update SettingsState in types.rs**

In `src/types.rs`:
- Line 648-652: Change setting item key from `"min_duration"` to `"min_probe_duration"`, label to `"Min Probe Duration (secs)"`, default to `DEFAULT_MIN_PROBE_DURATION`
- In `to_config()` / `from_config()`: Update field name mapping
- Line 953 in `apply_env_overrides()`: Change `("BLUBACK_MIN_DURATION", "min_duration")` to `("BLUBACK_MIN_PROBE_DURATION", "min_probe_duration")`

- [ ] **Step 3: Update settings tests**

Fix any tests that reference `"min_duration"` key, `BLUBACK_MIN_DURATION` env var, or the old default value of 900.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

Suggest: `refactor: rename min_duration CLI flag and settings panel to min_probe_duration`

---

### Task 3: Flip auto_detect default to true

**Files:**
- Modify: `src/config.rs`
- Modify: `src/types.rs`

- [ ] **Step 1: Change default in config getter**

In `src/config.rs` at line 409-411, change:
```rust
pub fn auto_detect(&self) -> bool {
    self.auto_detect.unwrap_or(true)
}
```

- [ ] **Step 2: Update to_toml_string() default comment**

In `to_toml_string()` (around line 195), the commented-out default should now show `# auto_detect = true`.

- [ ] **Step 3: Update SettingsState default**

In `src/types.rs` at lines 750-754, change the `value` in the auto_detect toggle from `config.auto_detect.unwrap_or(false)` to `config.auto_detect.unwrap_or(true)`.

- [ ] **Step 4: Update tests**

Fix any tests that assert `auto_detect()` returns `false` by default.

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 6: Commit**

Suggest: `feat: flip auto_detect default to true`

---

### Task 4: Add pre_classify_playlists() to detection

**Files:**
- Modify: `src/detection.rs`

- [ ] **Step 1: Write tests for pre_classify_playlists()**

Add to the test module in `src/detection.rs`:

```rust
#[test]
fn test_pre_classify_empty() {
    let result = pre_classify_playlists(&[], 30);
    assert!(result.is_empty());
}

#[test]
fn test_pre_classify_all_below_threshold() {
    let playlists = vec![
        Playlist { num: "1".into(), seconds: 10, ..Default::default() },
        Playlist { num: "2".into(), seconds: 20, ..Default::default() },
    ];
    let result = pre_classify_playlists(&playlists, 30);
    assert!(result.is_empty()); // nothing above threshold, no median to compute
}

#[test]
fn test_pre_classify_skips_obvious_specials() {
    let playlists = vec![
        Playlist { num: "1".into(), seconds: 2400, ..Default::default() }, // 40 min
        Playlist { num: "2".into(), seconds: 2700, ..Default::default() }, // 45 min
        Playlist { num: "3".into(), seconds: 2500, ..Default::default() }, // ~42 min
        Playlist { num: "4".into(), seconds: 600, ..Default::default() },  // 10 min = <50% of median
    ];
    let result = pre_classify_playlists(&playlists, 30);
    assert_eq!(result.len(), 1);
    assert!(result.contains("4"));
}

#[test]
fn test_pre_classify_keeps_medium_confidence() {
    // 28 min is 50-75% of 42 min median — should NOT be skipped
    let playlists = vec![
        Playlist { num: "1".into(), seconds: 2520, ..Default::default() }, // 42 min
        Playlist { num: "2".into(), seconds: 2520, ..Default::default() }, // 42 min
        Playlist { num: "3".into(), seconds: 1680, ..Default::default() }, // 28 min = 66% of median
    ];
    let result = pre_classify_playlists(&playlists, 30);
    assert!(result.is_empty()); // 28 min is NOT <50% of 42 min
}

#[test]
fn test_pre_classify_single_playlist_no_skip() {
    let playlists = vec![
        Playlist { num: "1".into(), seconds: 2400, ..Default::default() },
    ];
    let result = pre_classify_playlists(&playlists, 30);
    assert!(result.is_empty()); // can't compute meaningful median with 1 playlist
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test detection::tests::test_pre_classify -- --test-threads=1`
Expected: FAIL — `pre_classify_playlists` does not exist.

- [ ] **Step 3: Implement pre_classify_playlists()**

Add to `src/detection.rs`:

```rust
/// Pre-classify playlists by duration to identify obvious specials before probing.
///
/// Returns the set of playlist numbers below 50% of the median duration
/// (high-confidence specials only). These can be skipped during the probe phase.
/// Only considers playlists above `min_probe_duration` for the median calculation.
pub fn pre_classify_playlists(
    playlists: &[Playlist],
    min_probe_duration: u32,
) -> std::collections::HashSet<String> {
    let above_threshold: Vec<&Playlist> = playlists
        .iter()
        .filter(|p| p.seconds >= min_probe_duration)
        .collect();

    if above_threshold.len() < 2 {
        return std::collections::HashSet::new();
    }

    let mut durations: Vec<u32> = above_threshold.iter().map(|p| p.seconds).collect();
    durations.sort();
    let median = compute_median(&durations);

    if median == 0 {
        return std::collections::HashSet::new();
    }

    let threshold = median / 2; // 50% of median

    above_threshold
        .iter()
        .filter(|p| p.seconds < threshold)
        .map(|p| p.num.clone())
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test detection::tests::test_pre_classify -- --test-threads=1`
Expected: All pass.

- [ ] **Step 5: Commit**

Suggest: `feat: add pre_classify_playlists() for probe-skipping heuristic`

---

### Task 5: Remove min_duration param from detection API

**Files:**
- Modify: `src/detection.rs`

- [ ] **Step 1: Update run_detection() and run_detection_with_chapters() signatures**

Remove the `min_duration: u32` parameter from both functions. In `run_heuristics()`, change the baseline computation from filtering by `min_duration` to using all input playlists:

```rust
pub fn run_detection(
    all_playlists: &[Playlist],
    tmdb_episodes: Option<&[Episode]>,
    tmdb_specials: Option<&[Episode]>,
) -> Vec<DetectionResult>
```

```rust
pub fn run_detection_with_chapters(
    all_playlists: &[Playlist],
    tmdb_episodes: Option<&[Episode]>,
    tmdb_specials: Option<&[Episode]>,
    chapter_counts: &HashMap<String, usize>,
) -> Vec<DetectionResult>
```

In `run_heuristics()` (line 60), remove the `min_duration` parameter and change the baseline to use all input playlists:
```rust
fn run_heuristics(
    all_playlists: &[Playlist],
    chapter_counts: &HashMap<String, usize>,
) -> Vec<DetectionResult> {
    let baseline: Vec<&Playlist> = all_playlists.iter().collect();
    // ... rest unchanged
```

- [ ] **Step 2: Update all detection tests**

Every test calling `run_detection(&playlists, 900, ...)` or `run_detection_with_chapters(&playlists, 900, ...)` needs the `900` (or other min_duration value) removed. There are ~20 tests to update.

Key test that changes behavior:
- `test_all_below_min_duration` — this test passed playlists below 900s and expected Low confidence due to insufficient baseline. With `min_duration` removed, the baseline IS the input playlists. If 2+ playlists are passed, they form a valid baseline. Rethink this test: if the caller only passes probed playlists, all playlists in the input are above threshold. Rename to `test_single_playlist_insufficient_baseline` or similar.

- [ ] **Step 3: Fix all callers of run_detection/run_detection_with_chapters**

Run `cargo check` and fix all callers that still pass a `min_duration` argument:
- `src/tui/wizard.rs` in `run_detection_if_enabled()` (~line 880)
- `src/cli.rs` in `list_playlists()` (~line 158) and `run()` (~line 471)

For now, just remove the parameter from call sites. The callers will later be updated to pass only probed playlists.

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

Suggest: `refactor: remove min_duration parameter from detection API`

---

### Task 6: Add phase 1.5 to scan and remove start_unfiltered_probe

**Files:**
- Modify: `src/media/probe.rs`
- Modify: `src/session.rs`

- [ ] **Step 1: Add auto_detect param and skip set to scan_playlists_with_progress()**

In `src/media/probe.rs`, change the function signature to add `auto_detect: bool` and return the skip set:

```rust
#[allow(clippy::type_complexity)]
pub fn scan_playlists_with_progress(
    device: &str,
    min_probe_duration: u32,
    auto_detect: bool,
    on_progress: Option<&dyn Fn(u64, u64)>,
    on_probe_progress: Option<&dyn Fn(usize, usize, &str)>,
) -> Result<(Vec<Playlist>, ProbeCache, HashSet<String>), MediaError> {
```

After building the playlists list from log parsing (around line 90), add the pre-classification:

```rust
    let skip_set = if auto_detect {
        crate::detection::pre_classify_playlists(&playlists, min_probe_duration)
    } else {
        HashSet::new()
    };
```

Modify the probe loop filter (around line 103) to also exclude the skip set:

```rust
    let probe_indices: Vec<usize> = playlists
        .iter()
        .enumerate()
        .filter(|(_, pl)| pl.seconds >= min_probe_duration && !skip_set.contains(&pl.num))
        .map(|(i, _)| i)
        .collect();
```

Return the skip set: `Ok((playlists, probe_cache, skip_set))`

- [ ] **Step 2: Update all callers of scan_playlists_with_progress()**

Fix every call site to pass `auto_detect` and destructure the new return type:
- `src/session.rs` (~line 579): pass `self.auto_detect`, destructure skip set
- `src/cli.rs` (~line 95, ~line 766): pass the resolved auto_detect value, destructure skip set

Add `use std::collections::HashSet;` where needed.

- [ ] **Step 3: Remove start_unfiltered_probe()**

In `src/session.rs`:
- Delete the `start_unfiltered_probe()` function (lines 621-652)
- Remove its call site in the `BackgroundResult::DiscScan` handler (search for `self.start_unfiltered_probe`)

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

Suggest: `feat: add phase 1.5 pre-classification to scan, remove eager background probe`

---

### Task 7: Remove episodes_pl from data model

**Files:**
- Modify: `src/tui/mod.rs`
- Modify: `src/types.rs`
- Modify: `src/disc.rs`
- Modify: `src/session.rs`

- [ ] **Step 1: Remove episodes_pl from DiscState**

In `src/tui/mod.rs` line 50, remove `pub episodes_pl: Vec<Playlist>`.

- [ ] **Step 2: Remove episodes_pl from PlaylistView**

In `src/types.rs` line 455, remove `pub episodes_pl: Vec<Playlist>`.
Add `pub show_specials: bool` to PlaylistView.

- [ ] **Step 3: Replace episodes_pl_count in TmdbView**

In `src/types.rs` line 433, rename `episodes_pl_count` to `probed_count` (or similar).

- [ ] **Step 4: Remove filter_episodes() from disc.rs**

In `src/disc.rs`, remove the `filter_episodes()` function (lines 321-326) and its test `test_filter_episodes` (around line 481).

- [ ] **Step 5: Add probed_playlists() to DriveSession**

In `src/session.rs`, add:

```rust
/// Returns playlists that have been fully probed (have stream info).
pub fn probed_playlists(&self) -> Vec<&Playlist> {
    self.disc
        .playlists
        .iter()
        .filter(|pl| self.wizard.stream_infos.contains_key(&pl.num))
        .collect()
}
```

- [ ] **Step 6: Fix all compilation errors**

Run `cargo check` and fix every reference to `episodes_pl`, `filter_episodes`, and `episodes_pl_count` across the codebase. This will produce many errors — fix them methodically:

- `session.rs`: Remove `self.disc.episodes_pl = ...` assignments, `episodes_pl.len()` references
- `session.rs` `build_tmdb_view()`: Replace `self.disc.episodes_pl.len()` with `self.probed_playlists().len()`
- `session.rs` `build_playlist_view()`: Remove `episodes_pl` field, add `show_specials: self.wizard.show_specials`
- `cli.rs`: Remove `episodes_pl` variable and all references (this will break logic — fix in Task 10)
- `tui/wizard.rs`: Remove `view.episodes_pl` references (this will break logic — fix in Task 8)

For callers that need logic changes (not just removal), add `todo!()` markers temporarily. These are addressed in Tasks 8-10.

- [ ] **Step 7: Commit**

Suggest: `refactor: remove episodes_pl from data model, add probed_playlists()`

---

### Task 8: Rewrite session.rs callers

**Files:**
- Modify: `src/session.rs`
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Add new fields to WizardState**

In `src/tui/mod.rs`, add to WizardState (if not already added in Task 7):
```rust
pub show_specials: bool,
pub start_episode_popup: bool,
```

Both default to `true` and `false` respectively (via manual `Default` impl or field defaults).

- [ ] **Step 2: Rewrite visible_playlists()**

Replace the current implementation in `src/session.rs` (~line 1348):

```rust
pub fn visible_playlists(&self) -> Vec<(usize, &Playlist)> {
    let min_probe_dur = self.config.min_probe_duration(self.min_probe_duration_arg);
    self.disc
        .playlists
        .iter()
        .enumerate()
        .filter(|(_, pl)| {
            let above_threshold = pl.seconds >= min_probe_dur;
            let is_special = self.wizard.specials.contains(&pl.num)
                || self.wizard.detection_results.iter().any(|d| {
                    d.playlist_num == pl.num
                        && d.suggested_type == crate::detection::SuggestedType::Special
                        && d.confidence >= crate::detection::Confidence::High
                });
            let visible_by_threshold = above_threshold || self.wizard.show_filtered;
            let visible_by_special = !is_special || self.wizard.show_specials;
            visible_by_threshold && visible_by_special
        })
        .collect()
}
```

- [ ] **Step 3: Update BackgroundResult::DiscScan handler**

In the scan handler (~lines 807-912):

Remove the `episodes_pl` assignment and `filter_episodes()` call. Replace the movie_mode detection:

```rust
// Auto movie-mode detection
let auto_detect_on = self.config.should_auto_detect(self.auto_detect_arg);
let episode_count = if auto_detect_on {
    self.wizard.detection_results.iter().filter(|d| {
        matches!(d.suggested_type,
            crate::detection::SuggestedType::Episode
            | crate::detection::SuggestedType::MultiEpisode)
    }).count()
} else {
    self.probed_playlists().len()
};
self.tmdb.movie_mode = self.movie_mode_arg
    || (episode_count == 1 && self.season_arg.is_none());
```

Replace the empty guard:
```rust
if self.probed_playlists().is_empty() {
    let min_dur = self.config.min_probe_duration(self.min_probe_duration_arg);
    self.status_message = format!(
        "No playlists found above probe threshold ({}s). Try lowering --min-probe-duration.",
        min_dur
    );
    self.screen = Screen::Done;
    return;
}
```

Replace playlist_selected initialization:
```rust
let auto_detect_on = self.config.should_auto_detect(self.auto_detect_arg);
self.wizard.playlist_selected = self.disc.playlists
    .iter()
    .map(|pl| {
        if auto_detect_on {
            // Pre-select Episode/MultiEpisode, not specials
            self.wizard.detection_results.iter().any(|d| {
                d.playlist_num == pl.num
                    && matches!(d.suggested_type,
                        crate::detection::SuggestedType::Episode
                        | crate::detection::SuggestedType::MultiEpisode)
            })
        } else {
            // Auto-detect off: select all probed
            self.wizard.stream_infos.contains_key(&pl.num)
        }
    })
    .collect();
```

Remove the `reorder_playlists(&mut self.disc.episodes_pl, ...)` call (only `disc.playlists` reorder remains).

- [ ] **Step 4: Update apply_linked_context()**

Replace `&self.disc.episodes_pl` with non-special probed playlists:
```rust
let probed_non_special: Vec<Playlist> = self.probed_playlists()
    .into_iter()
    .filter(|pl| !self.wizard.specials.contains(&pl.num))
    .cloned()
    .collect();
self.wizard.episode_assignments =
    crate::util::assign_episodes(&probed_non_special, &self.tmdb.episodes, context.next_episode);
```

- [ ] **Step 5: Update build_playlist_view()**

Add `show_specials: self.wizard.show_specials` to the PlaylistView construction. Remove `episodes_pl` field.

- [ ] **Step 6: Update build_tmdb_view()**

Replace `episodes_pl_count: self.disc.episodes_pl.len()` with `probed_count: self.probed_playlists().len()`.

- [ ] **Step 7: Run tests**

Run: `cargo test session::`
Expected: Session tests compile and pass (some may need updating if they constructed `episodes_pl` directly).

- [ ] **Step 8: Commit**

Suggest: `refactor: rewrite session.rs to use probed_playlists() instead of episodes_pl`

---

### Task 9: Rewrite wizard.rs callers

**Files:**
- Modify: `src/tui/wizard.rs`

- [ ] **Step 1: Rewrite visible_playlists_view()**

Replace the current implementation (~line 369):

```rust
fn visible_playlists_view(view: &PlaylistView) -> Vec<(usize, &crate::types::Playlist)> {
    view.playlists
        .iter()
        .enumerate()
        .filter(|(_, pl)| {
            let above_threshold = view.stream_infos.contains_key(&pl.num)
                || view.show_filtered;
            let is_special = view.specials.contains(&pl.num)
                || view.detection_results.iter().any(|d| {
                    d.playlist_num == pl.num
                        && d.suggested_type == crate::detection::SuggestedType::Special
                        && d.confidence >= crate::detection::Confidence::High
                });
            let visible_by_special = !is_special || view.show_specials;
            above_threshold && visible_by_special
        })
        .collect()
}
```

Note: this function renders from the `PlaylistView` snapshot which doesn't have access to `min_probe_duration`. Using `stream_infos.contains_key()` as the "above threshold" check works because probed playlists are exactly those above the junk threshold (plus any on-demand probed).

- [ ] **Step 2: Rewrite reassign_regular_episodes()**

```rust
fn reassign_regular_episodes(session: &mut crate::session::DriveSession) {
    let non_special_pl: Vec<crate::types::Playlist> = session
        .probed_playlists()
        .into_iter()
        .filter(|pl| !session.wizard.specials.contains(&pl.num))
        .cloned()
        .collect();

    let disc_num = session.disc.label_info.as_ref().map(|l| l.disc);
    let start_ep = session.wizard.start_episode.unwrap_or_else(|| {
        crate::util::guess_start_episode(disc_num, session.wizard.stream_infos.len())
    });

    let new_assignments =
        crate::util::assign_episodes(&non_special_pl, &session.tmdb.episodes, start_ep);
    session
        .wizard
        .episode_assignments
        .retain(|k, _| session.wizard.specials.contains(k));
    session.wizard.episode_assignments.extend(new_assignments);
}
```

- [ ] **Step 3: Update row dimming**

In the playlist manager rendering (~line 613), replace the `is_episode_pl` check:

```rust
let is_probed = view.stream_infos.contains_key(&pl.num);
// ...
} else if !is_probed {
    Style::default().fg(Color::DarkGray)
} else {
```

- [ ] **Step 4: Update season handler**

In `handle_season_input_session()` (~line 1235), replace the `episodes_pl` reference:
```rust
let probed_non_special: Vec<crate::types::Playlist> = session
    .probed_playlists()
    .into_iter()
    .filter(|pl| !session.wizard.specials.contains(&pl.num))
    .cloned()
    .collect();
session.wizard.episode_assignments =
    crate::util::assign_episodes(&probed_non_special, &session.tmdb.episodes, start_ep);
```

- [ ] **Step 5: Update run_detection_if_enabled()**

Remove the `min_duration` argument from the `run_detection_with_chapters()` call.

- [ ] **Step 6: Fix remaining compilation errors in wizard.rs**

Run `cargo check` and fix any remaining references to `episodes_pl` or `episodes_pl_count`.

- [ ] **Step 7: Run tests**

Run: `cargo test wizard::`
Expected: Wizard tests compile and pass (test helpers that constructed `episodes_pl` need updating).

- [ ] **Step 8: Commit**

Suggest: `refactor: rewrite wizard.rs to use probed_playlists() instead of episodes_pl`

---

### Task 10: Update CLI mode

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add --hide-specials flag**

In `src/main.rs`, add to the Args struct:

```rust
/// Hide detected specials from ripping (skips all specials)
#[arg(long, conflicts_with = "specials")]
hide_specials: bool,
```

- [ ] **Step 2: Remove episodes_pl from scan_disc()**

In `src/cli.rs`, the `scan_disc()` function currently returns `episodes_pl`. Remove it from the return tuple. Replace `disc::filter_episodes()` with direct use of the probe cache to determine probed playlists. Update the auto-movie-mode detection to count probed playlists or Episode-classified playlists (matching the auto_detect on/off logic from the spec).

- [ ] **Step 3: Update list_playlists()**

- Remove the `filtered_index` mapping that used `min_duration` to number only episode-length playlists
- Number all playlists sequentially (probed playlists get numbers, junk gets `--`)
- Remove the `* = below min_duration` legend
- Show detection indicators by default
- For `--verbose`, probe pre-classified specials before display
- Show `--` for stream columns on unprobed playlists instead of `0`

- [ ] **Step 4: Update run()**

- Remove the `episodes_pl` variable and all references
- Use probed playlists (playlists in probe cache) instead
- Apply `--hide-specials`: filter detected specials from the rip set
- Update headless logic to auto-exclude high-confidence specials when auto_detect is on
- Update specials handling to work without `episodes_pl`

- [ ] **Step 5: Update history is_filtered**

Change `pl.seconds < min_dur` to `pl.seconds < min_probe_dur` (should already be renamed from Task 1).

- [ ] **Step 6: Run tests**

Run: `cargo test cli:: && cargo test cli_batch && cargo test cli_flag`
Expected: All CLI-related tests pass.

- [ ] **Step 7: Commit**

Suggest: `feat: update CLI mode for detection-driven classification, add --hide-specials`

---

### Task 11: Add S toggle (hide/show specials)

**Files:**
- Modify: `src/tui/wizard.rs`

- [ ] **Step 1: Write test for S key handler**

```rust
#[test]
fn test_s_uppercase_toggles_show_specials() {
    let mut session = make_test_session();
    session.wizard.show_specials = true;

    handle_playlist_manager_key(
        &mut session,
        KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
    );
    assert!(!session.wizard.show_specials);

    handle_playlist_manager_key(
        &mut session,
        KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
    );
    assert!(session.wizard.show_specials);
}
```

- [ ] **Step 2: Implement S key handler**

In `handle_playlist_manager_input_session()`, add a handler for uppercase `S`:

```rust
KeyCode::Char('S') => {
    session.wizard.expanded_playlist = None;
    if matches!(session.wizard.input_focus, InputFocus::TrackEdit(_)) {
        session.wizard.input_focus = InputFocus::List;
    }
    session.wizard.show_specials = !session.wizard.show_specials;
    let new_visible = session.visible_playlists();
    if session.wizard.list_cursor >= new_visible.len() {
        session.wizard.list_cursor = new_visible.len().saturating_sub(1);
    }
    reassign_regular_episodes(session);
}
```

- [ ] **Step 3: Update hints bar**

In the hints section of the playlist manager rendering, add the S toggle hint with hidden count:

```rust
let special_count = view.playlists.iter()
    .filter(|pl| view.specials.contains(&pl.num) || view.detection_results.iter().any(|d| {
        d.playlist_num == pl.num
            && d.suggested_type == crate::detection::SuggestedType::Special
            && d.confidence >= crate::detection::Confidence::High
    }))
    .count();
if view.show_specials {
    parts.push("S: Hide specials".into());
} else {
    parts.push(format!("S: Show specials ({} hidden)", special_count));
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test wizard::`
Expected: All wizard tests pass including the new one.

- [ ] **Step 5: Commit**

Suggest: `feat: add S toggle to hide/show specials in playlist manager`

---

### Task 12: Add E popup (set starting episode)

**Files:**
- Modify: `src/tui/wizard.rs`

- [ ] **Step 1: Write tests for E key handler**

```rust
#[test]
fn test_e_uppercase_opens_start_episode_popup() {
    let mut session = make_test_session();
    session.tmdb.movie_mode = false;
    session.wizard.start_episode_popup = false;

    handle_playlist_manager_key(
        &mut session,
        KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT),
    );
    assert!(session.wizard.start_episode_popup);
}

#[test]
fn test_start_episode_popup_enter_sets_and_reassigns() {
    let mut session = make_test_session_with_playlists();
    session.wizard.start_episode_popup = true;
    session.wizard.input_buffer = "5".into();

    handle_playlist_manager_key(
        &mut session,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    assert!(!session.wizard.start_episode_popup);
    assert_eq!(session.wizard.start_episode, Some(5));
}

#[test]
fn test_start_episode_popup_esc_cancels() {
    let mut session = make_test_session();
    session.wizard.start_episode_popup = true;
    session.wizard.input_buffer = "5".into();
    let original_start = session.wizard.start_episode;

    handle_playlist_manager_key(
        &mut session,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    );
    assert!(!session.wizard.start_episode_popup);
    assert_eq!(session.wizard.start_episode, original_start);
}
```

- [ ] **Step 2: Implement E key handler and popup logic**

In `handle_playlist_manager_input_session()`, add at the top (before other key handling) a guard for when the popup is active:

```rust
if session.wizard.start_episode_popup {
    match key.code {
        KeyCode::Enter => {
            if let Ok(n) = session.wizard.input_buffer.parse::<u32>() {
                if n > 0 {
                    session.wizard.start_episode = Some(n);
                    reassign_regular_episodes(session);
                }
            }
            session.wizard.start_episode_popup = false;
            session.wizard.input_buffer.clear();
            return;
        }
        KeyCode::Esc => {
            session.wizard.start_episode_popup = false;
            session.wizard.input_buffer.clear();
            return;
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            session.wizard.input_buffer.push(c);
            return;
        }
        KeyCode::Backspace => {
            session.wizard.input_buffer.pop();
            return;
        }
        _ => return,
    }
}
```

Add the E key case (only in TV mode):

```rust
KeyCode::Char('E') if !session.tmdb.movie_mode => {
    session.wizard.start_episode_popup = true;
    let current = session.wizard.start_episode.unwrap_or_else(|| {
        let disc_num = session.disc.label_info.as_ref().map(|l| l.disc);
        crate::util::guess_start_episode(disc_num, session.wizard.stream_infos.len())
    });
    session.wizard.input_buffer = current.to_string();
}
```

- [ ] **Step 3: Add popup rendering**

In the playlist manager render function, after drawing the main content but before the hints bar, check `view.start_episode_popup` and render a centered popup:

```rust
if view.start_episode_popup {
    let popup_width = 30;
    let popup_height = 3;
    let popup_area = centered_rect(popup_width, popup_height, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Start Episode")
        .border_style(Style::default().fg(Color::Cyan));

    let text = Paragraph::new(format!("Episode: {}", view.input_buffer))
        .block(block);
    frame.render_widget(text, popup_area);
}
```

Add `start_episode_popup` to PlaylistView and its construction in `build_playlist_view()`.

- [ ] **Step 4: Update hints**

Add to the hints bar: `"E: Set start episode"` (only in TV mode).

- [ ] **Step 5: Run tests**

Run: `cargo test wizard::`
Expected: All tests pass including new popup tests.

- [ ] **Step 6: Commit**

Suggest: `feat: add E popup for setting starting episode number`

---

### Task 13: On-demand probing

**Files:**
- Modify: `src/tui/wizard.rs`
- Modify: `src/session.rs`

- [ ] **Step 1: Add on-demand probe trigger for track expansion (t key)**

In the `t` key handler in wizard.rs, when the user expands a playlist that has no entry in `stream_infos`, start a background probe:

```rust
KeyCode::Char('t') => {
    if let Some(&(real_idx, _)) = visible.get(session.wizard.list_cursor) {
        let pl_num = session.disc.playlists[real_idx].num.clone();
        if !session.wizard.stream_infos.contains_key(&pl_num) {
            // Trigger on-demand probe
            session.start_on_demand_probe(&pl_num);
            // Don't expand yet — will expand when probe completes
        } else {
            // Toggle expansion as normal
            // ... existing logic
        }
    }
}
```

- [ ] **Step 2: Add start_on_demand_probe() to DriveSession**

In `src/session.rs`, add:

```rust
pub fn start_on_demand_probe(&mut self, playlist_num: &str) {
    let device = self.device.to_string_lossy().to_string();
    let num = playlist_num.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(format!("probe-{}", num))
        .spawn(move || {
            match crate::media::probe::probe_playlist(&device, &num) {
                Ok((media, streams)) => {
                    let mut results = std::collections::HashMap::new();
                    results.insert(num, (media, streams));
                    let _ = tx.send(BackgroundResult::BulkProbe(results));
                }
                Err(e) => {
                    log::warn!("On-demand probe failed for {}: {}", num, e);
                }
            }
        })
        .expect("failed to spawn probe thread");
    self.probe_rx = Some(rx);
}
```

- [ ] **Step 3: Add on-demand probe at confirm transition**

In the Enter handler that transitions from PlaylistManager to Confirm, check for unprobed selected playlists and probe them synchronously (or queue them):

```rust
// Before building rip jobs, probe any unprobed selected playlists
let unprobed_selected: Vec<String> = session.disc.playlists
    .iter()
    .enumerate()
    .filter(|(i, pl)| {
        session.wizard.playlist_selected.get(*i).copied().unwrap_or(false)
            && !session.wizard.stream_infos.contains_key(&pl.num)
    })
    .map(|(_, pl)| pl.num.clone())
    .collect();

if !unprobed_selected.is_empty() {
    // Probe synchronously before proceeding
    for num in &unprobed_selected {
        if let Ok((media, streams)) = crate::media::probe::probe_playlist(
            &session.device.to_string_lossy(), num
        ) {
            session.wizard.stream_infos.insert(num.clone(), streams);
            session.wizard.media_infos.insert(num.clone(), media);
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

Suggest: `feat: add on-demand probing for unprobed playlists`

---

### Task 14: Final cleanup and full test pass

**Files:**
- All modified files

- [ ] **Step 1: Run clippy**

Run: `cargo clippy -- -D warnings`
Fix any warnings.

- [ ] **Step 2: Run formatter**

Run: `rustup run stable cargo fmt`

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Verify CLI flag help**

Run: `cargo run -- --help`
Expected: Shows `--min-probe-duration` (not `--min-duration`), shows `--hide-specials`, `--auto-detect` present.

- [ ] **Step 5: Commit any cleanup**

Suggest: `chore: clippy and fmt cleanup for min_duration rework`
