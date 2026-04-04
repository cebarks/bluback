# Automatic Episode/Special Detection

**Date:** 2026-04-03
**Status:** Approved
**Version target:** TBD (pulls forward roadmap item #32 from v0.13)

## Problem

Episode assignment is purely sequential and special detection is entirely manual. Users must individually toggle specials with `s` in the TUI or pass `--specials` in the CLI. This is tedious on discs with multiple bonus features mixed among episodes. Meanwhile, the system already collects rich metadata (duration, stream counts, chapters) that could inform automatic detection.

## Solution

A two-layer detection system:

1. **Disc-level heuristics** — analyze playlist duration distribution, stream counts, and chapter counts to flag likely specials and multi-episode playlists. Works offline, no network needed.
2. **TMDb runtime matching** — when TMDb data is available, compare playlist durations against fetched episode runtimes (including season 0/specials) to refine heuristic confidence.

Results are presented with confidence indicators in the Playlist Manager. High-confidence specials are pre-marked; medium/low are shown as hints. Users can batch-accept suggestions or override individually.

## Detection Layers

### Layer 1: Disc Heuristics

Input: ALL disc playlists (not just filtered). The median/mode baselines are computed from playlists at or above `min_duration` (the "episode-length" set), then all playlists are evaluated against those baselines.

When auto-detection flags specials among playlists below `min_duration`, those playlists are automatically shown in the Playlist Manager (equivalent to `show_filtered = true` for detected playlists only).

**Duration analysis:**
- Compute median duration from playlists >= `min_duration`
- Playlist < 50% of median: high-confidence special
- Playlist 50-75% of median: medium-confidence special
- Playlist > 200% of median: high-confidence multi-episode (feeds into existing `assign_episodes()` logic)

**Stream count analysis:**
- Compute mode (most common) audio and subtitle track counts from playlists >= `min_duration`
- Playlists with fewer than half the mode's audio or subtitle track count: bump confidence toward special by one level (low -> medium, medium -> high)

**Chapter count analysis:**
- Compute mode chapter count from playlists >= `min_duration`
- Playlists with fewer than half the mode's chapter count, or zero chapters: bump confidence toward special by one level

Confidence bumps from stream/chapter analysis stack but cap at high.

### Layer 2: TMDb Runtime Matching

Input: filtered playlist list, TMDb episode data for the selected season + season 0.

**Runtime comparison:**
- For each regular-season episode, compute expected runtime
- Match tolerance: ±10% or ±3 minutes, whichever is more permissive
- Assume sequential playlist order (no optimal assignment solving)
- Playlists that don't match any regular episode runtime: boost special confidence
- Playlists that match a season 0 episode runtime: boost special confidence

**TMDb overrides:**
- A playlist matching a regular episode runtime at high confidence clears any special heuristic flags
- TMDb data refines Layer 1 results; it doesn't replace them entirely (Layer 1 still catches things TMDb doesn't know about, like behind-the-scenes featurettes not listed on TMDb)

### Season 0 Fetch

When TMDb data is available and auto-detection is enabled, fetch season 0 alongside the selected season. This provides specials metadata (episode names, runtimes) for matching. If the season 0 fetch fails (not all shows have specials on TMDb), detection continues with Layer 1 only.

## Confidence Model

Three levels:

| Level | Meaning | Pre-marked? | Headless behavior |
|-------|---------|-------------|-------------------|
| **High** | Multiple signals agree, or single very strong signal | Yes (pre-toggled as special) | Auto-applied with `--auto-detect --yes` |
| **Medium** | Single heuristic signal, no contradicting evidence | No (indicator shown) | Not auto-applied |
| **Low** | Weak signal, shown as hint | No (indicator shown) | Not auto-applied |

## Data Types

```rust
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
    /// Human-readable reasons (e.g., "duration 3m vs 45m median", "2 audio tracks vs 6 typical")
    pub reasons: Vec<String>,
}
```

## New Module: `src/detection.rs`

Pure function, no side effects:

```rust
pub fn run_detection(
    all_playlists: &[Playlist],
    min_duration: u32,
    tmdb_episodes: Option<&[TmdbEpisode]>,
    tmdb_specials: Option<&[TmdbEpisode]>,
) -> Vec<DetectionResult>
```

Takes all disc playlists (unfiltered) and `min_duration` to establish the episode-length baseline. Returns a `DetectionResult` per playlist. Called after playlist scanning completes and (optionally) after TMDb lookup.

Detection runs twice in the typical flow:
1. After scan completes (Layer 1 only) — provides initial suggestions before TMDb
2. After TMDb lookup (Layer 1 + Layer 2) — refines with runtime data

## TUI Changes

### Playlist Manager

**Confidence indicator column** — shown between the selection checkbox and playlist number:

| Indicator | Meaning |
|-----------|---------|
| `[S!]` | High confidence special (pre-marked) |
| `[S?]` | Medium confidence special |
| `[s.]` | Low confidence special hint |
| `[M!]` | High confidence multi-episode |
| (blank) | No detection signal / regular episode |

Indicators use color coding: high = yellow, medium = dark yellow/dim, low = dark gray.

**New keybind: `A`** — Accept all high and medium suggestions. Applies special marking to all medium+ confidence playlists. Confirmation not needed (easily reversible with `R` to reset all).

**Existing keybinds unchanged:**
- `s` still toggles individual special marking (overrides detection)
- `r` / `R` still reset individual / all assignments
- `e` still edits episode assignment inline

**Tooltip/status line:** When cursor is on a row with a detection result, show the reason string in the status area (e.g., "Duration 3:12 vs 44:30 median, 2/6 audio tracks").

### TMDb Search Screen

No visual changes. Season 0 fetch happens silently alongside the selected season.

## CLI Changes

### New Flag

```
--auto-detect    Enable automatic episode/special detection heuristics
--no-auto-detect Disable auto-detection (overrides config)
```

Mutually exclusive with `--movie` (movies don't have episodes/specials to detect).

### Headless Behavior (`--auto-detect --yes`)

- Run detection after scan and TMDb lookup
- Auto-apply high-confidence suggestions only
- Print applied detections to stderr: `Auto-detected: playlist 00810 as special (duration 3:12 vs 44:30 median)`
- `--specials` takes precedence: explicit special assignments override auto-detection

### Interactive CLI (`--auto-detect` without `--yes`)

- Show detection results in playlist listing (same indicators as TUI)
- Prompt: "Accept auto-detected specials? [Y/n/edit]"
  - Y: apply all high+medium
  - n: ignore suggestions
  - edit: fall through to manual selection

## Config

```toml
auto_detect = false  # Enable automatic episode/special detection
```

Environment variable: `BLUBACK_AUTO_DETECT` (true/false).

No per-heuristic tuning knobs in this iteration. If defaults prove unreliable for specific heuristics, individual toggles can be added later under a `[detection]` table.

## Settings Panel

New toggle item in the settings overlay:

- **Auto-detect episodes/specials** — Toggle (bool), maps to `auto_detect` config

Placed after the existing `show_filtered` toggle (same logical grouping of playlist behavior).

## Interaction with Existing Features

**`assign_episodes()`** — Detection results feed into the existing assignment logic:
- Playlists detected as specials are excluded from sequential episode numbering
- Multi-episode detection from heuristics supplements the existing 1.5x median threshold
- The existing `assign_episodes()` function is called after detection, operating on the subset of playlists not marked as specials

**Batch mode** — Auto-detection runs on each disc. Episode auto-advance skips specials (existing behavior preserved).

**`--list-playlists`** — When `--auto-detect` is also passed, show detection indicators in the listing output.

**Overwrite / verification / hooks** — No interaction; detection only affects assignment, not rip behavior.

## Testing

**Unit tests in `src/detection.rs`:**
- Duration heuristic: single playlist (no detection), uniform durations (no specials), clear outliers, borderline cases at 50%/75%/200% thresholds
- Stream count heuristic: mode computation, bump logic, playlists with identical stream counts (no bump)
- Chapter count heuristic: same pattern as stream count
- Confidence stacking: verify bumps cap at high, multiple bumps from different heuristics
- TMDb matching: exact match, within tolerance, no match, season 0 match
- TMDb override: regular episode match clears special flag
- Combined layers: Layer 1 + Layer 2 interaction
- Edge cases: empty playlist list, single playlist, all same duration, all different durations

**Integration points (manual verification):**
- TUI indicator rendering and `A` keybind
- CLI `--auto-detect --yes` output
- Interaction with `--specials` precedence

## Out of Scope

- Per-heuristic configuration knobs (add later if defaults prove unreliable)
- Optimal playlist-to-episode assignment solving (sequential assumption kept)
- Inter-disc learning or rip history-based detection
- Auto-fetching TMDb special episode names for filename rendering
- Movie mode detection (movies don't have episode/special structure)
