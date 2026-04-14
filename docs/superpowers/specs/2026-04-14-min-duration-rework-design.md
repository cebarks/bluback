# Rework min_duration: Detection-Driven Playlist Classification

**Date:** 2026-04-14
**Version target:** 0.11.x

## Problem

`min_duration` (default 900s / 15 minutes) currently does three unrelated jobs:

1. **Probe gate** — decides which playlists get the expensive per-playlist FFmpeg open for stream info
2. **Display filter** — controls which playlists are visible in the TUI/CLI by default
3. **Detection baseline** — defines which playlists count as "normal episodes" for auto-detection median/mode calculations

This starves downstream systems of data. A 10-minute bonus feature never gets probed, so auto-detection has no stream counts, the TUI shows no codec info even when the user toggles filtered playlists visible, and the track picker has nothing to display.

## Solution

Decouple the three concerns into three independent mechanisms:

1. **Junk filter** (`min_probe_duration`, default 30s) — noise floor that skips menu clips and black frames. Configurable for weird discs. Not an episode classifier.
2. **Auto-detection** (default on) — classifies all probed playlists into Episode / Special / MultiEpisode. Replaces `min_duration`'s role as the episode classifier entirely.
3. **Display filtering** — `f` toggle shows junk (below threshold). New `S` toggle hides specials. Both are view-layer concerns.

## Config & Naming Changes

### Renamed field

`min_duration` is removed entirely. Replaced by `min_probe_duration`.

- Config key: `min_probe_duration` (was `min_duration`)
- CLI flag: `--min-probe-duration` (was `--min-duration`)
- Env var: `BLUBACK_MIN_PROBE_DURATION` (was `BLUBACK_MIN_DURATION`)
- Settings panel label: "Min Probe Duration (secs)"
- Default: **30** (was 900)
- Help text: "Min seconds to probe playlist (filters menu clips)"

No deprecation handling — old key/flag/env var are simply removed. This is a breaking config change for 0.11.x.

### auto_detect default flips to true

`auto_detect` config option defaults to `true` (was `false`). Existing configs that don't set it explicitly get the new default. `--no-auto-detect` still works to disable.

### show_filtered unchanged

`show_filtered` semantics stay the same — controls visibility of playlists below the junk threshold. At 30s, this is truly junk (menu clips, black frames).

### Settings panel implementation changes

All settings panel code referencing the old field needs updating:

- Constant: `DEFAULT_MIN_DURATION` (900) → `DEFAULT_MIN_PROBE_DURATION` (30)
- `SettingsState` item key: `"min_duration"` → `"min_probe_duration"`
- `SettingsState` item label: `"Min Duration (secs)"` → `"Min Probe Duration (secs)"`
- `to_config()` / `from_config()`: field name mapping updated
- `apply_env_overrides()`: `("BLUBACK_MIN_DURATION", "min_duration")` → `("BLUBACK_MIN_PROBE_DURATION", "min_probe_duration")`
- Validation: `min_duration > 0` check becomes `min_probe_duration > 0`

## Probe & Scan Changes

### Phase 1: Playlist Enumeration (unchanged)

Log-capture scan discovers all playlists with numbers and durations. No changes.

### Phase 1.5: Duration Pre-Classification (new)

When auto-detect is enabled, after phase 1 but before phase 2:

1. Compute median duration of playlists above `min_probe_duration`
2. Flag playlists < 50% of median as "likely specials" (high-confidence threshold only)
3. These pre-classified playlists are skipped during phase 2 probing

Only the high-confidence threshold (< 50% of median) is used for probe-skipping. The medium-confidence band (50-75%) is NOT skipped — those playlists are probed normally so that stream/chapter data is available for full detection to refine the classification. This avoids false positives like a legitimately short episode (e.g., 28 minutes on a disc of 42-minute episodes) being permanently misclassified without stream data to correct it.

This uses the same duration math as detection Layer 1 but only the duration heuristic — no stream or chapter data available yet.

When auto-detect is disabled, this phase is skipped entirely — all playlists above `min_probe_duration` are probed.

### Phase 1.5 implementation location

Pre-classification runs inside `scan_playlists_with_progress()`. The function gains an `auto_detect: bool` parameter. When true, after phase 1 collects all playlists, `pre_classify_playlists()` computes the skip set before the per-playlist probe loop. The skip set is passed to the loop to filter which playlists get probed. The skip set is also returned alongside the playlist list and probe cache so the session can track which playlists were pre-classified.

### Phase 2: Per-Playlist Deep Probe (modified)

Probes playlists above `min_probe_duration` that were NOT in the pre-classification skip set. Each probe extracts:

- Video streams: codec, resolution, framerate, bit depth, profile, HDR classification
- Audio streams: codec, channels, layout, language, profile
- Subtitle streams: codec, language, forced flag
- Format-level: bitrate

Pre-classified specials remain unprobed with zero stream info until on-demand probe.

### `start_unfiltered_probe()` removal

The existing `start_unfiltered_probe()` in `session.rs` eagerly probes all playlists not in the scan's probe cache immediately after scan completes. This would negate the phase 1.5 optimization by probing pre-classified specials in the background. This function is removed. On-demand probing (described below) replaces it for the cases where stream info is actually needed.

### On-Demand Probing (new)

When a user interacts with an unprobed playlist:

- Expands with `t` (track list) — triggers immediate probe (track list needs stream data)
- Transitions to confirm screen with unprobed playlists selected — probes all unprobed selected playlists before proceeding

Selecting with Space does NOT trigger probing — it just toggles the boolean. Probing is deferred to when stream data is actually needed (track expansion or confirm transition). This avoids unnecessary probes for playlists the user immediately deselects.

The playlist is probed at that point with a brief "probing..." indicator in the TUI. After probing, detection is re-run for that playlist with the new stream/chapter data, which may adjust its classification and confidence level.

On-demand probes are sequential — only one at a time via the existing `probe_rx` channel. If the user navigates away while a probe is in progress, the probe completes in the background and results are applied when ready (same pattern as the current scan background task). Rapid interactions queue rather than overlap.

In CLI mode, on-demand probing happens implicitly: if a user selects an unprobed playlist via `--playlists`, it is probed before remux since the remux pipeline requires stream info regardless. `--list-playlists --verbose` probes all playlists including pre-classified specials (the verbose flag opts into the full probe cost).

## episodes_pl Removal

The concept of `episodes_pl` (playlists above `min_duration` threshold) is removed entirely:

- `DiscState.episodes_pl: Vec<Playlist>` — removed
- `disc::filter_episodes()` — removed
- `PlaylistView.episodes_pl` — removed
- `TmdbView.episodes_pl_count` — replaced with count of probed playlists

### Definition: "probed playlists"

Throughout this spec, "probed playlists" means playlists that have an entry in `wizard.stream_infos` (i.e., `self.wizard.stream_infos.contains_key(&pl.num)`). This is the existing storage for per-playlist stream data, populated from the scan's `ProbeCache` and from on-demand probes. It includes:

- Playlists above `min_probe_duration` that were probed during phase 2
- Playlists that were on-demand probed after user interaction

It does NOT include:

- Playlists below `min_probe_duration` (junk)
- Playlists that were pre-classified as likely specials and have not yet been on-demand probed

A convenience method `probed_playlists(&self) -> Vec<&Playlist>` on `DriveSession` filters `disc.playlists` by `self.wizard.stream_infos.contains_key(&pl.num)`.

### Initial playlist selection

Without `episodes_pl`, `playlist_selected` initialization changes:

- **Auto-detect on (default):** Pre-select playlists classified as `Episode` or `MultiEpisode` by detection. Specials are NOT pre-selected (user opts in via `s` or `A`). This matches the current behavior where specials marked by auto-detection are selected only after the user accepts them.
- **Auto-detect off:** Pre-select all probed playlists. The user sees everything selected and manually deselects what they don't want.

### What replaces episodes_pl for each caller

| Caller | Old behavior | New behavior |
|--------|-------------|--------------|
| `visible_playlists()` | Show if in episodes_pl OR medium+ non-Episode detection | Show all playlists above junk threshold (probed or pre-classified), minus hidden specials |
| `assign_episodes()` | Receives episodes_pl minus specials | Receives non-special probed playlists |
| `guess_start_episode()` | Uses `episodes_pl.len()` | Uses `stream_infos.len()` — total probed playlist count regardless of classification (not just Episode-classified — preserves multi-disc math) |
| `reassign_regular_episodes()` | Filters episodes_pl minus specials | Filters probed playlists minus specials |
| CLI display | `*` for below min_duration | Detection indicators only; no `*` markers |
| Headless episode count | `episodes_pl.len()` | Count of non-special probed playlists |
| `apply_linked_context()` | Uses `episodes_pl` for initial assignment in multi-drive link | Uses non-special probed playlists |
| Batch `count_assigned_episodes()` | Counts from `episode_assignments` built off `episodes_pl` | Unchanged — counts from `episode_assignments`, which is now built from non-special probed playlists. Same counting, different input source. |
| Row dimming (wizard.rs) | Dims playlists not in `episodes_pl` | Dims unprobed playlists (not in `stream_infos`) and pre-classified specials |
| Season handler `assign_episodes()` | Uses `episodes_pl` as input | Uses non-special probed playlists (same as `reassign_regular_episodes()`) |
| `reorder_playlists()` on `episodes_pl` | Reorders `episodes_pl` alongside `disc.playlists` | Removed — only `disc.playlists` is reordered (probed status follows the playlist via its `num` key) |

### Auto-movie-mode detection

Currently `episodes_pl.len() == 1` triggers automatic movie mode. With `episodes_pl` removed:

- **Auto-detect on (default):** Count playlists classified as `Episode` or `MultiEpisode` by detection. If exactly 1, trigger movie mode (unless `--season` is provided). This is more accurate than the old heuristic — a disc with 1 feature film and 5 short extras would correctly trigger movie mode, whereas the old 900s threshold would too (by coincidence) but the new 30s threshold would not (6 playlists above threshold).
- **Auto-detect off:** Count all probed playlists (above `min_probe_duration`). If exactly 1, trigger movie mode. Same as old behavior but with the lower threshold.

### Empty playlist guard

The current `episodes_pl.is_empty()` guard (session.rs and cli.rs) is replaced with a check for zero probed playlists: if no playlists above `min_probe_duration` exist after scan, show error "No playlists found above probe threshold ({N}s). Try lowering --min-probe-duration." and go to Done screen (TUI) or bail (CLI).

### Fallback when auto-detect is off

No detection results means no classification. All probed playlists (above `min_probe_duration`) are treated as episode candidates. The user sees a flat unclassified list and manually picks what to rip. This is the same as today's behavior but with the lower 30s threshold.

## Display & Visibility Model

Three visibility layers with independent toggles:

| Layer | What it controls | Toggle | Default state |
|-------|-----------------|--------|---------------|
| Junk | Playlists below `min_probe_duration` (unprobed) | `f` | Hidden |
| Specials | Playlists classified as specials (detected or manual `s`) | `S` | Visible |
| Everything else | Episodes, multi-episodes | Always shown | — |

### visible_playlists() logic

1. Start with all `disc.playlists`
2. Filter out below `min_probe_duration` unless `show_filtered` is true
3. Filter out specials unless `show_specials` is true
4. Everything remaining is shown

### When specials are hidden (S toggled off)

- Specials disappear from the playlist manager
- They are deselected for ripping (skipped)
- Episode assignments cascade as if those playlists don't exist
- Hidden count shown in hints bar: `S: Show specials (3 hidden)`
- Toggling back on restores selection state and special assignments

### New WizardState fields

- `show_specials: bool` (default: `true`) — resets to `true` on rescan (via `reset_for_rescan()`), same as other wizard state. Per-disc UI preference, not a persistent setting.
- `start_episode_popup: bool` (default: `false`) — when true, the `E` popup is rendered over the playlist manager and input routes to the popup handler. Uses the existing `input_buffer` for the number input (same pattern as inline edit).

### PlaylistView changes

`PlaylistView` snapshot struct gains `show_specials: bool` (mirroring WizardState) so the rendering layer can filter specials. `PlaylistView.episodes_pl` is removed.

## Set Starting Episode Popup

### TUI: `E` hotkey in Playlist Manager

Opens a centered popup box:

- Prompt: "Start episode number:"
- Pre-filled with current starting episode (from `wizard.start_episode` or auto-guessed value)
- Number-only input
- Enter confirms, Esc cancels

### On confirm

- Sets `wizard.start_episode` to the entered value
- Calls `reassign_regular_episodes()` to re-cascade all episode numbers
- Multi-episode detection still applies
- Specials untouched (SP numbering is independent)

### Batch mode interaction

Batch auto-advance tracks the next episode across discs. The `E` popup overrides for the current disc only — batch resumes auto-advancing from wherever this disc leaves off.

### CLI

`--start-episode` already exists and does the same thing. No CLI changes needed.

## reassign_regular_episodes() Changes

New logic:

1. Collect playlists from `probed_playlists()` (probe cache members) that are NOT specials (neither auto-detected high-confidence nor manually marked via `s`)
2. When specials are hidden (`S` toggle off), special playlists are also excluded from assignment
3. Pass remaining playlists to `assign_episodes()` with current `start_episode`
4. Median duration for multi-episode detection computed from the input list (non-special probed playlists)
5. Special assignments retained independently

### Trigger points

- `s` key (toggle special) — same as today
- `R` key (reset all) — same as today
- `A` key (accept auto-detect suggestions) — same as today
- `S` key (toggle special visibility) — new, triggers reassignment
- `E` key (set start episode) — new, triggers reassignment with new start point

## Unaffected Subsystems

The following subsystems operate on `disc.playlists` (the full list), not `episodes_pl`, and require no changes:

- **Confirm screen** — filters by `playlist_selected`, independent of `episodes_pl`
- **Chapter extraction** (`chapters.rs`) — operates on playlist numbers directly
- **Index-based reordering** (`index.rs`) — reorders `disc.playlists` in place
- **Stream filtering / track selection** (`streams.rs`) — per-playlist, independent of classification

### History recording

`is_filtered` in history records currently uses `pl.seconds < min_duration`. This changes to `pl.seconds < min_probe_duration` — same semantics, new field name and threshold.

## Detection System Adjustments

### API changes

The `min_duration: u32` parameter is removed from `run_detection()` and `run_detection_with_chapters()`. The baseline is computed from all input playlists — callers are responsible for passing the appropriate set (probed playlists, not junk).

New function: `pre_classify_playlists(playlists: &[Playlist], min_probe_duration: u32) -> HashSet<String>` — takes all playlists from phase 1, computes median of those above `min_probe_duration`, returns the set of playlist numbers below 50% of median (the probe-skip set). Called by `scan_playlists_with_progress()` before the per-playlist probe loop.

### Baseline computation

- Old: baseline = playlists with `seconds >= min_duration` (900s)
- New: baseline = all input playlists (callers pass probed playlists only)
- Since pre-classification already removed likely specials from the probe set, the baseline naturally contains episode-like playlists

### Layer 1 runs at two depths

1. **Pre-classification (phase 1.5):** `pre_classify_playlists()` — duration-only heuristic on all playlists above junk threshold. Only the high-confidence threshold (< 50% of median) is used. Produces a skip set for probe-skipping. No stream/chapter confidence bumps.
2. **Full detection (after probe):** `run_detection_with_chapters()` — runs on all playlists with stream counts and chapter data. Produces final classifications with confidence stacking across all thresholds (< 50%, 50-75%, > 200%). Pre-classified specials that remain unprobed carry forward their `Special/High` duration-only classification.

### Layer 2 (TMDb matching)

Unchanged. Runs after TMDb lookup, refines classifications with runtime matching.

### On-demand re-detection

When a previously-skipped special gets probed on-demand, re-run detection for that playlist with new stream/chapter data. Classification and confidence may change. User sees indicator update.

## CLI Mode Changes

### Flag changes

- `--min-duration` removed, replaced by `--min-probe-duration`
- `--auto-detect` default on
- `--start-episode` unchanged

### New flag: --hide-specials

Equivalent of the `S` toggle off in TUI. Excludes detected specials from selection and ripping. Useful in headless/scripted workflows.

`--hide-specials` conflicts with `--specials` (clap argument conflict). Their semantics are opposing — marking specific playlists as specials while simultaneously hiding all specials is contradictory.

### --list-playlists output

- No more `*` marker for "below min_duration"
- Detection indicators shown by default (auto-detect now on by default)
- Unprobed playlists show `--` for stream columns instead of `0`

### Headless mode (--yes) behavior

- Auto-detect on (default): high-confidence specials auto-excluded from rip unless `--specials` explicitly selects them
- `--hide-specials`: skips all detected specials regardless of confidence
- `--no-auto-detect --yes`: all probed playlists selected

## TUI Keybinding Summary

New and changed keybindings in Playlist Manager:

| Key | Action |
|-----|--------|
| `S` | Toggle special visibility (hide/show all specials) |
| `E` | Open "set starting episode" popup |
| `f` | Show/hide junk playlists (unchanged, but threshold now 30s) |
| `s` | Toggle single playlist as special (unchanged) |

## Migration

No deprecation period. Old config keys, CLI flags, and env vars are removed. The `min_duration` config key will trigger the existing unknown-key validation warning, prompting users to update their config.
