# Per-Stream Track Selection — Design Spec

**Date:** 2026-03-29
**Roadmap item:** #20 (v0.10)
**Branch:** `feature/track-selection`

## Context

bluback currently offers two stream selection modes: `All` (every stream) and `PreferSurround` (surround audio + stereo secondary + all subtitles). Both are global — the same rule applies to every playlist. Users cannot choose individual audio, subtitle, or video streams, and there is no language-based filtering.

Blu-ray discs commonly include 5–10 audio tracks (multiple languages, commentary, descriptive audio) and 5–15 subtitle tracks across languages. Including everything inflates file size significantly. Users need a way to select which streams to keep, both via persistent config defaults and per-playlist manual overrides.

## Goals

1. Config-level language/format defaults that auto-filter streams (default: all streams included — opt-in filtering)
2. TUI inline track picker for per-playlist manual override
3. CLI flags for both language-based filtering and precise index-based selection
4. Unified probing — single probe pass returns all stream metadata
5. Deprecate old `stream_selection` config key in favor of a `[streams]` section

## Non-Goals

- Per-stream codec transcoding (post-1.0)
- Automatic stream selection based on file size targets
- Per-playlist config defaults (one config applies to all playlists; overrides are session-only)

## Architecture

### Data Flow

```
Config/CLI → StreamFilter (language + format rules)
                ↓
Disc scan  → probe_playlist() [unified: MediaInfo + StreamInfo per playlist]
                ↓
Playlist Manager: user presses `t` → expand inline
  StreamFilter.apply(StreamInfo) → initial selected indices
  User toggles → override selections
  Stored as HashMap<playlist_num, Vec<usize>> in WizardState
                ↓
prepare_remux_options() receives StreamSelection::Manual(indices) per playlist
                ↓
remux() uses Manual indices directly (unchanged packet loop)
```

### New Module: `src/streams.rs`

Pure functions for stream filtering and track spec parsing. No I/O.

```rust
/// Config/CLI-derived stream filtering rules.
#[derive(Debug, Clone, Default)]
pub struct StreamFilter {
    /// Audio language filter. Empty = include all.
    pub audio_languages: Vec<String>,
    /// Subtitle language filter. Empty = include all.
    pub subtitle_languages: Vec<String>,
    /// If true and surround audio exists in filtered set,
    /// select surround + one stereo; otherwise select all matching audio.
    pub prefer_surround: bool,
}
```

**`StreamFilter::apply(&self, info: &StreamInfo) -> Vec<usize>`**

Resolution logic:
1. All video streams included (no video filtering).
2. Filter audio by language tags. Streams with language `und` or `None` always included.
3. If no audio streams match any listed language, fall back to all audio + emit warning.
4. If `prefer_surround` and surround (≥6ch) exists in filtered set: keep surround + one stereo from the filtered set. If no surround, keep all filtered audio.
5. Filter subtitles by language tags. `und`/`None`-tagged always included.
6. If no subtitle streams match, fall back to all subtitles + emit warning.
7. Collect absolute stream indices, sort, dedup.

**`parse_track_spec(spec: &str, info: &StreamInfo) -> Result<Vec<usize>>`**

Parses CLI `--tracks` format: `"v:0;a:0,2;s:0-1"`. Type-local indices are 0-based (matching the `v0`, `a0`, `s0` labels shown in TUI and `--list-playlists -v`). Mapped to absolute container indices via `StreamInfo`. Omitted types default to all streams of that type. Uses a dedicated 0-based range parser (not `parse_selection()`, which is 1-based for user-facing playlist numbers).

If a type-local index exceeds the available streams for a playlist (e.g., `a:3` but a playlist only has 2 audio streams), emit a warning and include all available streams of that type for the affected playlist. Error only if the spec references a type with zero streams in the source.

## New Types

### `VideoStream` (in `types.rs`)

```rust
#[derive(Debug, Clone)]
pub struct VideoStream {
    pub index: usize,         // absolute stream index in container
    pub codec: String,        // "hevc", "h264", "vc1"
    pub resolution: String,   // "1920x1080"
    pub hdr: String,          // "SDR", "HDR10", "DV", "HDR10+", "HLG"
    pub framerate: String,    // "23.976"
    pub bit_depth: String,    // "10", "8"
}
```

### `SubtitleStream` (in `types.rs`)

```rust
#[derive(Debug, Clone)]
pub struct SubtitleStream {
    pub index: usize,              // absolute stream index in container
    pub codec: String,             // "hdmv_pgs_subtitle", "subrip"
    pub language: Option<String>,  // BCP-47/ISO 639-2
    pub forced: bool,              // AV_DISPOSITION_FORCED
}
```

**Note:** The `forced` flag requires reading FFmpeg's stream disposition flags via `ffmpeg-the-third`. Verify during implementation that the crate exposes `AVStream.disposition` or a `disposition()` method. If unavailable, set `forced: false` as default and add as a future enhancement.

### `StreamInfo` expanded (in `types.rs`)

```rust
#[derive(Debug, Clone, Default)]
pub struct StreamInfo {
    pub video_streams: Vec<VideoStream>,
    pub audio_streams: Vec<AudioStream>,       // existing, unchanged
    pub subtitle_streams: Vec<SubtitleStream>,  // replaces subtitle_count: u32
}
```

`subtitle_count` removed. Callers that only need the count use `info.subtitle_streams.len()`.

## Unified Probing

### `probe_playlist()` (in `media/probe.rs`)

```rust
pub fn probe_playlist(device: &str, playlist_num: &str)
    -> Result<(MediaInfo, StreamInfo), MediaError>
```

Opens the bluray context once, iterates all streams, populates both `MediaInfo` (video summary + first audio summary for display) and `StreamInfo` (all streams with full metadata). Replaces separate `probe_media_info()` and `probe_streams()` calls.

`build_stream_info()` in `remux.rs` removed — the remux pipeline receives `StreamSelection::Manual(Vec<usize>)` directly and no longer needs to probe for selection.

### Probing Timing

Episode-length playlists (those passing the `min_duration` filter) are probed during the disc scan phase. Both `MediaInfo` and `StreamInfo` cached per playlist. Filtered (short) playlists are probed lazily on first `t` expansion in the TUI — this avoids expensive probing of 50+ playlists on discs with many short extras. A spinner/status message shows while a lazy probe runs.

**Playlist Manager → Confirm transition:** Since `MediaInfo` and `StreamInfo` are already cached during scan, the `Enter` key on Playlist Manager reads from cache directly — no background probe needed. The current `BackgroundResult::MediaProbe` variant is **repurposed** for lazy probing of filtered playlists on `t` expansion (carries a single playlist's data). The old flow (spawn background thread to probe all selected playlists on `Enter`) is removed. Filename generation on `Enter` uses cached `MediaInfo`.

**Storage in session:**
```rust
// WizardState (src/tui/mod.rs)
pub media_infos: HashMap<String, MediaInfo>,       // playlist_num → probed media info (replaces Vec<Option<MediaInfo>>)
pub stream_infos: HashMap<String, StreamInfo>,     // playlist_num → probed streams
pub track_selections: HashMap<String, Vec<usize>>, // playlist_num → selected indices (manual overrides only)
```

## Config: `[streams]` Section

### TOML Format

```toml
[streams]
audio_languages = ["eng", "jpn"]    # empty or omitted = all
subtitle_languages = ["eng"]         # empty or omitted = all
prefer_surround = false              # default false
```

All fields optional. Omitting `[streams]` entirely = current behavior (all streams).

### Config Struct

The `Config` struct gets a new field for serde deserialization:

```rust
pub streams: Option<StreamsConfig>,
```

Where `StreamsConfig` mirrors `StreamFilter` but is the serde deserialization target:

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StreamsConfig {
    pub audio_languages: Option<Vec<String>>,
    pub subtitle_languages: Option<Vec<String>>,
    pub prefer_surround: Option<bool>,
}
```

### Config Resolution

`Config::resolve_stream_filter(&self) -> StreamFilter` replaces `resolve_stream_selection()`. Priority: `[streams]` fields first, falls back to old `stream_selection` for migration.

### Config Validation

Add `"streams"` to `KNOWN_KEYS` (top-level key). The existing `validate_raw_toml()` only checks top-level keys — sub-key validation within `[streams]` is not in scope (matches existing behavior for `[metadata]`, `[post_rip]`, `[post_session]`).

### Config Save

`to_toml_string()` updated to emit `[streams]` section with commented-out defaults, following the same pattern as `[metadata]`. `SettingsState::to_config()` also updated to write `[streams]` fields.

### Deprecation of `stream_selection`

- `stream_selection = "prefer_surround"` → `StreamFilter { prefer_surround: true, ..default }`, log deprecation warning.
- `stream_selection = "all"` or absent → default `StreamFilter`, no warning.
- If both `stream_selection` and `[streams]` are present, `[streams]` takes precedence and a deprecation warning is logged: `"Config key 'stream_selection' is ignored when [streams] section is present"`.
- Old key still parses for backwards compatibility but not written on save.
- `"streams"` added to `KNOWN_KEYS`, `"stream_selection"` kept for backwards compat but deprecated.
- Auto-migrated on next config save.

## TUI: Inline Track Expansion

### Interaction

- **`t`** on a playlist row in the Playlist Manager expands/collapses its track list inline.
- One expansion at a time — pressing `t` on another row collapses the current.
- **`Space`** toggles individual stream selection while expanded.
- **`↑↓`** navigates within expanded tracks.
- **`Esc`** or **`t`** collapses back to playlist level.

### Focus State Interactions

- `t` is a no-op during `InlineEdit` (episode editing). Collapse tracks first with `Esc`, then edit.
- `e` is a no-op during `TrackEdit`. Collapse tracks first with `Esc`, then press `e` to edit episodes.
- `f` (toggle filtered playlists) while a track is expanded: collapse the expansion first, then toggle. Prevents the expanded playlist from becoming invisible.
- `s` (toggle special) works normally regardless of track expansion state (operates on the playlist row, not tracks).

### State

- `WizardState.expanded_playlist: Option<usize>` — which playlist row is expanded (index into the full `playlists` vec, not the displayed/filtered subset).
- New `InputFocus::TrackEdit(usize)` variant — cursor is inside an expanded track list at the given sub-row index.
- Track data read from cached `stream_infos`.
- Manual overrides stored in `track_selections` — only populated when user changes something.

### Layout

Expanded tracks appear below the playlist row, indented with a left border. Streams grouped by type (VIDEO / AUDIO / SUBTITLES) with type-local indices shown (`v0`, `a0`, `a1`, `s0`).

- Selected streams: `[X]` with normal text color.
- Config-filtered streams: `[ ]` with dimmed text + "← filtered by config" hint.
- Forced subtitles: yellow `FORCED` badge.
- Cursor row highlighted.

### Collapsed Indicator

When a playlist has custom track selections (differs from filter defaults), the existing "Ch" column shows a summary like `1v 3a 2s` instead of the default channel layout (e.g., `7.1`), with a `*` suffix to indicate customization.

### Toggle Save Timing

Individual `Space` toggles immediately update `track_selections`. Changes are not batched on collapse — each toggle is applied immediately. This means collapsing an expansion (via `t`, `Esc`, or expanding another row) does not discard changes.

## CLI Flags

```
--audio-lang <LANGS>       Filter audio by language, comma-separated (e.g. "eng,jpn")
--subtitle-lang <LANGS>    Filter subtitles by language (e.g. "eng")
--tracks <SPEC>            Select by type-local index (e.g. "a:0,2;s:0-1")
--prefer-surround          Prefer surround audio over stereo
--all-streams              Include all streams, ignoring config filters
```

### Behavior

- `--audio-lang` / `--subtitle-lang` / `--prefer-surround` build a `StreamFilter`, overriding config values for the run. Priority: CLI flags > `[streams]` config > old `stream_selection` config.
- `--all-streams` overrides any `[streams]` config filters for the run, ensuring all streams are included (equivalent to having no `[streams]` section). Useful when config has language filters but you want everything for a specific disc.
- `--tracks` bypasses language filtering — direct index selection. Applied to all playlists. Format: `v:0;a:0,2;s:0-1`. Omitted types default to all of that type. Since type-local indices may resolve to different absolute indices per playlist, `parse_track_spec()` is called per-playlist with each playlist's `StreamInfo`.
- `--tracks` conflicts with `--audio-lang` / `--subtitle-lang` / `--prefer-surround` / `--all-streams` (clap argument group).
- `--all-streams` conflicts with `--audio-lang` / `--subtitle-lang` / `--prefer-surround` (clap argument group).
- `--tracks` and `--playlists` are orthogonal and can be combined: `--playlists` selects which playlists to rip, `--tracks` selects which streams within each.
- `--list-playlists -v` updated to show type-local indices alongside stream details.
- Headless (`--yes`): language filter or `--tracks` applied automatically. No flags = config defaults (which default to all).

## Settings Panel

Three new fields under a "Streams" separator:

| Setting | Type | Display |
|---------|------|---------|
| Audio Languages | Text | Comma-separated, e.g. `eng,jpn` |
| Subtitle Languages | Text | Comma-separated, e.g. `eng` |
| Prefer Surround | Toggle | On/Off |

Saves to `[streams]` table in config. Empty text fields = include all.

## Remux Pipeline Changes

- `prepare_remux_options()` receives `StreamSelection::Manual(Vec<usize>)` or `StreamSelection::All` per playlist — callers resolve filter + overrides to indices before calling.
- `StreamSelection::PreferSurround` variant **removed** — its behavior is fully subsumed by `StreamFilter { prefer_surround: true }`. Removing it avoids a broken code path since `build_stream_info()` is also removed. Any remaining references to `PreferSurround` are compile errors, making migration explicit.
- `build_stream_info()` removed from `remux.rs` — stream info is probed upfront by `probe_playlist()` and passed through as `Manual` indices.
- `select_streams()` simplified: only handles `All` (return `0..total_streams`) and `Manual` (return indices). No `StreamInfo` parameter needed.
- Packet loop unchanged.

### Per-Playlist Resolution at Rip Time

For each playlist:
1. Check `track_selections` for manual override → use `Manual(indices)` if present.
2. Otherwise, apply `StreamFilter` to cached `StreamInfo` → `Manual(indices)`.
3. If no filter configured (empty `StreamFilter`), use `StreamSelection::All`.

## Guards

- At least one video stream must be selected if the source has video streams. At least one audio stream must be selected if the source has audio streams. (Some playlists may be audio-only extras or video-only slideshows — don't require a stream type that doesn't exist in the source.)
- At least one stream total must be selected (can't rip an empty selection).
- **TUI:** Block Enter on Confirm screen with warning identifying the problematic playlist.
- **CLI:** Error: `"Playlist 00801: no audio streams selected"`.

## Language Handling

- Language tags matched case-insensitively against stream metadata (`eng`, `ENG`, `Eng` all match).
- Streams with no language tag (`und` or absent) are always included by language filters — never auto-excluded. Users can manually deselect in the TUI.
- Language lists are preferences, not hard requirements. If no audio streams match any listed language, all audio streams are included with a warning. Same for subtitles.
- Warnings surface as: TUI status message (not stderr, since ratatui owns the terminal) + log entry. CLI prints to stderr.

## Files Changed

| File | Change |
|------|--------|
| `src/streams.rs` | **New.** `StreamFilter`, `apply()`, `parse_track_spec()`, 0-based range parser |
| `src/types.rs` | Add `VideoStream`, `SubtitleStream`; expand `StreamInfo`; remove `subtitle_count`; repurpose `BackgroundResult::MediaProbe` for lazy single-playlist probe; add `stream_infos: HashMap<String, StreamInfo>`, `track_selections: HashMap<String, Vec<usize>>`, `expanded_playlist: Option<usize>` to `PlaylistView`; add track summary field to `ConfirmView` |
| `src/media/probe.rs` | Add `probe_playlist()`; deprecate `probe_streams()` and `probe_media_info()` |
| `src/media/remux.rs` | Remove `build_stream_info()` and `PreferSurround` variant; simplify `select_streams()` to `All` + `Manual` only |
| `src/config.rs` | Add `StreamsConfig` struct; `[streams]` parsing; `resolve_stream_filter()`; deprecate `stream_selection`; update `to_toml_string()` to emit `[streams]`; add `"streams"` to `KNOWN_KEYS` |
| `src/disc.rs` | Update `probe_media_info()` wrapper to use `probe_playlist()` |
| `src/main.rs` | Add `--audio-lang`, `--subtitle-lang`, `--tracks`, `--prefer-surround`, `--all-streams` flags |
| `src/cli.rs` | Wire new flags; per-playlist stream resolution; update `--list-playlists -v` with type-local indices |
| `src/tui/mod.rs` | Add `expanded_playlist`, `track_selections`, `stream_infos` to `WizardState`; `TrackEdit` focus variant |
| `src/tui/wizard.rs` | Render inline track expansion; handle `t`/Space/navigation in expanded state; focus state interaction rules |
| `src/session.rs` | Probe `StreamInfo` upfront for episode-length playlists; lazy probe for filtered playlists on `t`; pass per-playlist selections to `prepare_remux_options()` |
| `src/workflow.rs` | Update `prepare_remux_options()` callers |
| `src/tui/settings.rs` | Add Streams separator + 3 fields; update `SettingsState::to_config()` |
| `src/tui/wizard.rs` | *(also)* Show track summary on Confirm screen (`render_confirm_view`) |

## Testing

- **`src/streams.rs`:** Unit tests for `StreamFilter::apply()` — all combinations of language filter, prefer_surround, empty filter, no-match fallback, und streams, forced subs. Unit tests for `parse_track_spec()` — valid specs, omitted types, invalid/out-of-range indices, edge cases. Unit tests for 0-based range parser.
- **`src/types.rs`:** Tests for `VideoStream`/`SubtitleStream` display methods.
- **`src/media/probe.rs`:** Tests for `probe_playlist()` using existing synthetic fixtures (extend `generate_fixtures.sh` to include multi-audio/multi-subtitle MKVs).
- **`src/config.rs`:** Round-trip tests for `[streams]` section. Deprecation migration test (`stream_selection` → `[streams]`). Both-keys-present test (new wins, warning logged). `to_toml_string()` round-trip with `[streams]`. Validation tests.
- **`src/tui/wizard.rs`:** Rendering tests for expanded track list. Cursor navigation within expansion. Focus state interaction tests (`t` during `InlineEdit`, `e` during `TrackEdit`, `f` during expansion).
- **`src/tui/settings.rs`:** Update `test_settings_state_from_config_item_count` for new items.
- **CLI integration:** `--tracks` parsing, `--audio-lang`/`--subtitle-lang` filter application, `--all-streams` override, conflict detection, `--tracks` + `--playlists` combined.

## Environment Variable Overrides

New env vars for the `[streams]` settings, following existing `BLUBACK_*` convention:
- `BLUBACK_AUDIO_LANGUAGES` — comma-separated (e.g., `eng,jpn`)
- `BLUBACK_SUBTITLE_LANGUAGES` — comma-separated
- `BLUBACK_PREFER_SURROUND` — `true`/`false`

These are applied in `apply_env_overrides()` and noted in the settings panel import notification, matching existing behavior.

## Interaction with Verify

Rip verification (`--verify`) checks output stream counts against expected values. With per-stream track selection, the verify check must compare against the **selected** stream count, not the source's total. The selected count is known at rip time from `StreamSelection::Manual(indices).len()` or from `StreamSelection::All` (total source streams).
