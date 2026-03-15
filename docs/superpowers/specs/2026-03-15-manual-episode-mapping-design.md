# Manual Episode Mapping

## Summary

Add a manual playlist-to-episode mapping mode that allows assigning arbitrary episodes (including multiple episodes) to individual playlists. Supplements the existing sequential auto-assign with an opt-in manual override.

## Motivation

The current sequential assignment model (playlist 1 = episode N, playlist 2 = episode N+1) doesn't handle discs where:
- A single playlist contains multiple concatenated episodes
- The disc layout doesn't match linear episode order
- Episodes are spread across playlists in a non-obvious way

## Design Decisions

- **Single file, multi-episode naming**: When a playlist maps to multiple episodes, it's still ripped as one MKV. The filename reflects the episode range (e.g., `S01E03-E04_Title.mkv`). No chapter splitting.
- **Mode toggle**: The sequential auto-assign remains the default. A new wizard screen lets the user review assignments and switch to manual mode when needed.
- **Title from first episode**: When multiple episodes are assigned, `{title}` uses the first episode's name.
- **TV mode only**: The mapping screen is skipped in movie mode and when no TMDb episodes were fetched.

## Data Model Changes

### `EpisodeAssignments` type (`types.rs`)

Changes from `HashMap<String, Episode>` to `HashMap<String, Vec<Episode>>`. Each playlist maps to zero or more episodes.

### `RipJob.episode` field (`types.rs`)

Changes from `Option<Episode>` to `Vec<Episode>`. Empty vec = no assignment.

### `{episode}` placeholder rendering (`util.rs`)

When a playlist has multiple episodes, the episode number renders as:
- Consecutive: `03-E04` (e.g., template `S{season}E{episode}` renders `S01E03-E04`)
- Non-consecutive: `03-E05` (uses same dash format — non-consecutive multi-episode is uncommon enough that range notation is fine; avoids comma-in-filename issues and matches Plex/Jellyfin conventions)
- Single: `03` (unchanged)

### `{title}` placeholder rendering (`util.rs`)

Always uses the first episode's title. No change for single-episode case.

## New Wizard Screen: EpisodeMapping (TUI)

### Position in flow

After `SeasonEpisode` confirms, before `PlaylistSelect`. Only shown in TV mode when TMDb episodes were fetched.

### Screen enum

New variant: `Screen::EpisodeMapping`

### Step numbering

**TV mode** (with new screen):
1. TMDb Search
2. Show Select
3. Season & Starting Episode
4. **Episode Mapping** (new)
5. Playlist Select (was Step 4, increment by 1)
6. Confirm (was Step 5, increment by 1)

**Movie mode** (unchanged):
1. TMDb Search
2. Show Select
3. Playlist Select
4. Confirm

The step title strings in `render_playlist_select()` and `render_confirm()` already branch on `app.movie_mode` — the TV-mode branch increments by 1.

### Layout

- Header: "Step 4: Episode Mapping" with show name
- Table columns: `#`, `Playlist`, `Duration`, `Episode(s)`
- Each row shows auto-assigned episode(s) for that playlist
- Rows with no assignment (more playlists than episodes) show "(none)"
- Cursor highlights current row
- Footer with keybindings

### Keybindings

- `Up/Down` — Navigate playlists
- `Enter` — Accept mappings, proceed to PlaylistSelect
- `e` — Edit highlighted playlist's episode assignment (enters inline input mode)
- `Esc` — Go back to SeasonEpisode

### Inline edit mode

When `e` is pressed on a row:
- Input field appears for that row, pre-filled with current assignment
- Type episode numbers: `3`, `3-4`, `3,5`, or empty to clear
- `Enter` — Confirm edit, return to navigation
- `Esc` — Cancel edit, keep previous value

### Unassigned playlists

Playlists that overflow the auto-assigned episode list show "(none)". The user can press `e` on these rows and manually type episode numbers. Episode numbers that don't exist in the TMDb fetch are allowed — the episode name will show as the number only (e.g., "E07") since we don't have metadata for it.

### Duplicate episode assignments

Duplicate assignments across playlists are allowed (the user may intentionally assign the same episode to multiple playlists). No validation or warning is needed.

## Back-Navigation Updates

### `PlaylistSelect` Esc behavior

Currently, pressing `Esc` on `PlaylistSelect` goes back to `SeasonEpisode` (if TMDb was used) or `TmdbSearch`. With the new `EpisodeMapping` screen, `Esc` from `PlaylistSelect` should go back to `EpisodeMapping` when episode assignments exist, instead of `SeasonEpisode`.

Updated logic:
- If `movie_mode` and `selected_movie.is_some()` → `ShowSelect` (unchanged)
- If `!episode_assignments.is_empty()` → `EpisodeMapping` (was `SeasonEpisode`)
- Otherwise → `TmdbSearch` (unchanged)

## CLI Mode Changes (`cli.rs`)

After `assign_episodes()` runs, show mappings and prompt:

```
  Episode Mappings:
    00001 (0:43:00)  ->  S01E01 - Pilot
    00002 (0:44:00)  ->  S01E02 - Second
    00003 (0:43:00)  ->  S01E03 - Third

  Accept mappings? [Y/n/manual]:
```

- `Y` or Enter — Accept, proceed to playlist selection
- `n` — Go back to starting episode prompt
- `manual` — Per-playlist prompts, then re-display mappings for review:

```
  Playlist 00001 (0:43:00) [1]: 1
  Playlist 00002 (0:44:00) [2]: 3-4
  Playlist 00003 (0:43:00) [3]: 5

  Updated Episode Mappings:
    00001 (0:43:00)  ->  S01E01 - Pilot
    00002 (0:44:00)  ->  S01E03-E04 - Third
    00003 (0:43:00)  ->  S01E05 - Fifth

  Accept mappings? [Y/n/manual]:
```

After manual entry, the mappings are re-displayed and the accept/reject/manual prompt loops until the user accepts.

Each per-playlist prompt shows current/default assignment in brackets. Input format: single number, range (`3-4`), comma-separated (`3,5`). Empty input keeps default.

Skipped when no TMDb episodes were fetched, or in movie mode.

## Function Changes

### `assign_episodes()` (`util.rs`)

Return type changes from `HashMap<String, Episode>` to `HashMap<String, Vec<Episode>>`. Same sequential logic, wraps each episode in a `vec![episode]`.

### New: `parse_episode_input()` (`util.rs`)

Parses manual input like `3`, `3-4`, `3,5` into `Vec<u32>` of episode numbers. Returns `None` on invalid input (non-numeric, reversed ranges like `5-3`, zero).

This is a syntax-only parser — it does not validate against the TMDb episode list. Validation happens at the call site: if an episode number exists in the fetched list, use the Episode data; if not, create a minimal Episode with just the number and an empty name.

Empty input returns `Some(vec![])` (clears assignment). This is distinct from `None` (parse error).

### `make_filename()` (`util.rs`)

Parameter `episode` changes from `Option<&Episode>` to `&[Episode]`. Multiple episodes affect `{episode}` and `{title}` rendering as described above. Empty slice = fallback to `playlist{num}.mkv`.

### `handle_season_episode_input()` (`tui/wizard.rs`)

Currently transitions to `Screen::PlaylistSelect` when the start episode is confirmed (line 545). Changes to transition to `Screen::EpisodeMapping` instead.

### Key call site: `playlist_filename()` (`tui/wizard.rs`)

Currently calls `app.episode_assignments.get(&pl.num)` and passes `Option<&Episode>` to `make_filename()`. Changes to extract the slice: `app.episode_assignments.get(&pl.num).map(|v| v.as_slice()).unwrap_or(&[])`.

### All other call sites

`episode_assignments.get()` returns `Option<&Vec<Episode>>` instead of `Option<&Episode>`. All consumers update accordingly:
- `ep.is_some()` for "has any assignment" becomes `ep.map_or(false, |v| !v.is_empty())`
- Single-episode access becomes `v.first()` or `&v[0]` where a single episode is expected

### Dashboard rendering (`tui/dashboard.rs`)

The episode column in the rip job table (line 53-57) currently renders `Option<Episode>` as `E{num} {name}`. Changes to render `Vec<Episode>`:
- Single episode: `E03 Pilot` (unchanged)
- Multiple consecutive: `E03-E04 Pilot` (first episode's name)
- Multiple non-consecutive: `E03-E05 Pilot`
- Empty: `""` (unchanged)

## Tests

New unit tests in `util.rs`:

- `parse_episode_input()`: single number, range, comma-separated, reversed range (None), zero (None), empty (Some(vec![])), non-numeric (None)
- `make_filename()` with multi-episode `&[Episode]`: consecutive rendering, non-consecutive rendering, single episode (unchanged behavior), empty slice (fallback)
- `assign_episodes()`: verify return type produces `Vec<Episode>` wrappers, existing test logic adapted

## What Doesn't Change

- **Ripping logic** (`rip.rs`) — One playlist still produces one MKV. No chapter splitting.
- **Movie mode** — Completely unaffected. Mapping screen skipped.
- **Disc scanning** (`disc.rs`) — No changes.
- **Config / format resolution** (`config.rs`) — No changes.
- **TMDb fetching** (`tmdb.rs`) — No changes.
