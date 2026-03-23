# TUI Refactor Design Spec

**Date**: 2026-03-22
**Status**: Draft
**Scope**: Wizard flow restructuring, App decomposition, specials support, UX improvements

## Overview

Refactor the TUI wizard to reduce screen count, merge related screens, add specials support, and improve code organization. The App god struct (47 fields) gets broken into logical sub-structs while keeping the existing enum-based state machine pattern.

## Goals

- Merge episode mapping and playlist selection into a single "Playlist Manager" screen
- Merge TMDb search and show/movie selection into a single screen with inline results
- Simplify the Season screen (remove starting episode field — auto-guess, edit per-playlist)
- Show all disc playlists (including filtered-out short ones), not just episode-length
- Add specials/extras support with dedicated naming template
- Add loading spinners during blocking operations
- Show disc label on all screens
- Auto-detect new disc on Done screen with popup prompt
- Break `App` struct into logical sub-structs

## Out of Scope

- Settings menu / config panel overhaul
- Pause/resume during ripping
- Terminal title updates
- macOS/Windows support

## Screen Flow

### TV Mode (7 screens, down from 9)

```
Scanning → TMDb Search → Season → Playlist Manager → Confirm → Ripping → Done
```

### Movie Mode (6 screens, down from 7)

```
Scanning → TMDb Search → Playlist Manager → Confirm → Ripping → Done
```

### Screen Enum

```rust
pub enum Screen {
    Scanning,
    TmdbSearch,    // was TmdbSearch + ShowSelect (2 screens → 1)
    Season,        // was SeasonEpisode (simplified)
    PlaylistManager, // was EpisodeMapping + PlaylistSelect (2 screens → 1)
    Confirm,
    Ripping,
    Done,
}
```

## App Struct Decomposition

The current 47-field `App` struct is broken into focused sub-structs:

```rust
pub struct App {
    pub screen: Screen,
    pub args: Args,
    pub config: Config,
    pub quit: bool,
    pub eject: bool,
    pub has_mkvpropedit: bool,
    pub status_message: String,
    pub spinner_frame: usize,  // incremented each tick, indexes into spinner chars

    pub disc: DiscState,
    pub tmdb: TmdbState,
    pub wizard: WizardState,
    pub rip: RipState,

    pub pending_rx: Option<mpsc::Receiver<BackgroundResult>>,
}
```

### DiscState

All disc-related data: detection, playlists, mounting, chapters.

```rust
pub struct DiscState {
    pub label: String,
    pub label_info: Option<LabelInfo>,
    pub playlists: Vec<Playlist>,       // ALL playlists from disc
    pub episodes_pl: Vec<Playlist>,     // filtered episode-length playlists
    pub scan_log: Vec<String>,
    pub mount_point: Option<String>,
    pub did_mount: bool,
    pub chapter_counts: HashMap<String, usize>,
}
```

### TmdbState

TMDb search state, selected show/movie, episode data.

```rust
pub struct TmdbState {
    pub api_key: Option<String>,
    pub search_query: String,
    pub movie_mode: bool,
    pub search_results: Vec<TmdbShow>,
    pub movie_results: Vec<TmdbMovie>,
    pub selected_show: Option<usize>,
    pub selected_movie: Option<usize>,
    pub show_name: String,
    pub episodes: Vec<Episode>,
}
```

### WizardState

All wizard UI state: cursors, input, selections, assignments.

```rust
pub struct WizardState {
    pub list_cursor: usize,
    pub input_buffer: String,
    pub input_focus: InputFocus,  // replaces input_active; tracks what has focus
    pub season_num: Option<u32>,
    pub start_episode: Option<u32>,
    pub episode_assignments: EpisodeAssignments,
    pub playlist_selected: Vec<bool>,    // always sized to disc.playlists.len()
    pub specials: HashSet<String>,       // playlist nums marked as special
    pub show_filtered: bool,             // whether to show below-min-duration playlists
    pub filenames: Vec<String>,
    pub media_infos: Vec<Option<MediaInfo>>,
}

/// Tracks which UI element has focus. Replaces the old `input_active: bool`
/// and `mapping_edit_row: Option<usize>` with a single enum.
pub enum InputFocus {
    /// Text input field has focus (TMDb search, season number, API key)
    TextInput,
    /// Navigating a list (search results, playlist table)
    List,
    /// Inline editing a specific row in the Playlist Manager
    InlineEdit(usize),  // row index being edited
}
```

### RipState

Ripping process state: jobs, child process, progress.

```rust
pub struct RipState {
    pub jobs: Vec<RipJob>,
    pub current_rip: usize,
    pub child: Option<std::process::Child>,
    pub progress_rx: Option<mpsc::Receiver<String>>,
    pub progress_state: HashMap<String, String>,
    pub stderr_buffer: Option<Arc<Mutex<String>>>,
    pub confirm_abort: bool,
    pub confirm_rescan: bool,
}
```

## Screen Designs

### 1. Scanning (mostly unchanged)

- Shows spinning indicator while waiting for disc
- Displays device probe log (dimmed)
- Once disc found, shows "Scanning {device}..." with spinner

**Loading indicator**: A simple rotating character (`|`, `/`, `-`, `\`) appended to the status message, driven by the 100ms event poll tick.

### 2. TMDb Search (merged with Show Select)

Combines the search input and results selection into one screen.

**Layout:**
```
┌─ TMDb Search (TV Show) ──────────────────────────┐
│ Disc: SHOW_S01_D1  |  8 playlists                │
├───────────────────────────────────────────────────┤
│ ┌─ Search query ────────────────────────────────┐ │
│ │ the show name|                                │ │
│ └───────────────────────────────────────────────┘ │
│ ┌─ Results ─────────────────────────────────────┐ │
│ │ > The Show Name (2020)                        │ │
│ │   The Show Name: Origins (2022)               │ │
│ │   Another Show (2019)                         │ │
│ └───────────────────────────────────────────────┘ │
├───────────────────────────────────────────────────┤
│ Enter: Search/Select | ↑↓: Results | Tab: Movie  │
│ Esc: Skip TMDb                                    │
└───────────────────────────────────────────────────┘
```

**Behavior:**
- On load, input has focus with the guessed search query pre-filled
- Enter in input field triggers search (shows spinner)
- When results appear, Down arrow moves focus from input into results list
- Up arrow from top of results returns focus to input
- Enter on a result selects it and proceeds
- Tab toggles TV/Movie mode
- Esc skips TMDb entirely, proceeds to Playlist Manager

**Focus states:**
- `InputFocus::TextInput`: typing in search field, Enter = search
- `InputFocus::List`: navigating results, Enter = select

### 3. Season (simplified)

Only shown in TV mode. No starting episode field.

**Layout:**
```
┌─ Season ──────────────────────────────────────────┐
│ Disc: SHOW_S01_D1  |  Show: The Show Name         │
├───────────────────────────────────────────────────┤
│ ┌─ Season number ──────────────────────────────┐  │
│ │ 1|                                           │  │
│ └──────────────────────────────────────────────┘  │
│ ┌─ Season 1: 12 episodes ─────────────────────┐  │
│ │   E01 - Pilot (45 min)                       │  │
│ │   E02 - The Second One (42 min)              │  │
│ │   ...                                        │  │
│ └──────────────────────────────────────────────┘  │
├───────────────────────────────────────────────────┤
│ Enter: Confirm/Fetch | Esc: Back                  │
└───────────────────────────────────────────────────┘
```

**Behavior:**
- Pre-filled with guessed season from volume label
- Enter fetches episodes (spinner) if not yet loaded
- Enter again after episodes loaded: auto-assigns episodes sequentially using guessed start, proceeds to Playlist Manager
- Episode assignment uses updated `assign_episodes()` with `guess_start_episode()` and multi-episode detection (see below)

### 4. Playlist Manager (new combined screen)

The core of this refactor. Combines playlist selection and episode mapping.

**Layout:**
```
┌─ Playlist Manager ────────────────────────────────────────────────────┐
│ Disc: SHOW_S01_D1  |  Show: The Show Name  |  8 selected, 2 hidden   │
├───────────────────────────────────────────────────────────────────────┤
│   # Playlist  Duration  Ch  Episode(s)           Filename            │
│ > [x]  1  00800   1:22:34   14  S01E01 - Pilot         Show S01E01.mkv│
│   [x]  2  00801   0:45:12    8  S01E02 - Second        Show S01E02.mkv│
│   [x]  3  00802   0:44:58    7  S01E03 - Third         Show S01E03.mkv│
│   [ ]  4  00803   0:02:14    1  (none)            [S]  Show S00E01.mkv│
│   ...                                                                 │
├───────────────────────────────────────────────────────────────────────┤
│ Space: Toggle | e: Edit episodes | s: Special | f: Show filtered      │
│ Enter: Confirm | Esc: Back                                            │
└───────────────────────────────────────────────────────────────────────┘
```

**Playlist indexing and visibility:**
- `disc.playlists` contains every playlist from the disc
- `disc.episodes_pl` is the filtered subset (above min-duration), used for default selection and episode assignment
- `playlist_selected` is always sized to `disc.playlists.len()` — one entry per playlist on disc, indexed by position in `disc.playlists`
- On init, entries corresponding to `episodes_pl` are set to `true`, all others to `false`
- By default, only playlists in `episodes_pl` are shown (filtered ones hidden)
- Press `f` to toggle showing all playlists; filtered ones shown dimmed
- Toggling `f` does NOT change `playlist_selected` values or indices — it only affects which rows are rendered
- `list_cursor` indexes into the currently visible list, not the full `disc.playlists` — cursor-to-playlist mapping must account for hidden rows

**Hotkeys:**
- `Space` — toggle playlist selection
- `e` — enter inline edit mode for episode assignment on highlighted row
  - Input accepts: `3` (single), `3-4` (range), `3,5` (list)
  - Enter confirms, Esc cancels
- `s` — toggle "special" flag on highlighted playlist
  - Specials use `wizard.specials: HashSet<String>` (stores playlist num)
  - Visual indicator: `[S]` marker in the row
  - Specials use a separate naming template (see Specials section)
- `f` — toggle showing filtered (below min-duration) playlists
- `Up/Down` — navigate
- `Enter` — confirm selection, trigger media probes, proceed to Confirm
- `Esc` — go back (to Season in TV mode, to TMDb Search in Movie mode)

**Episode assignment column:**
- Shows assigned episode info: `S01E03 - Title` or `S01E03-E04 - Title` for multi-episode
- Shows `(none)` if no assignment
- Shows inline edit cursor when in edit mode: `3,4|`

### 5. Confirm (mostly unchanged)

- Shows summary table of selected playlists with filenames, durations, estimated sizes
- Disc label in header
- Enter starts ripping, Esc goes back to Playlist Manager

### 6. Ripping Dashboard (unchanged)

No changes to the ripping dashboard in this refactor.

### 7. Done (enhanced)

**Auto-detect new disc:**
- After ripping completes, spawn background disc polling (same mechanism as Scanning) using `pending_rx`
- `poll_background` must handle `BackgroundResult::DiscFound` differently when `screen == Done` — instead of transitioning to `TmdbSearch`, it stores the detected disc label and sets a `disc_detected_popup: bool` flag on `App` (or `RipState`)
- When new disc detected, render a popup overlay on the Done screen
- Enter on popup triggers full rescan (reset + `start_disc_scan`), any other key exits

**Popup:**
```
┌─ Done ────────────────────────────────────────────┐
│ All done! Backed up 8 playlist(s)                 │
├───────────────────────────────────────────────────┤
│   Show S01E01 - Pilot.mkv (4.2 GB)               │
│   Show S01E02 - Second.mkv (2.1 GB)              │
│   ...                                             │
│                                                   │
│  ┌─────────────────────────────────────────────┐  │
│  │  New disc detected: SHOW_S01_D2             │  │
│  │  Press Enter to start, any other key to exit│  │
│  └─────────────────────────────────────────────┘  │
│                                                   │
├───────────────────────────────────────────────────┤
│ [Enter] Rescan  [Ctrl+E] Eject  [any key] Exit   │
└───────────────────────────────────────────────────┘
```

The popup is rendered as a centered overlay block using ratatui's `Clear` widget + `Block` with borders. The existing results content remains visible behind it.

**Behavior without auto-detect popup:**
- Enter or Ctrl+R: rescan (same as current)
- Ctrl+E: eject
- Any other key: exit (ejects if configured)

## Multi-Episode Detection in Auto-Assignment

The current `assign_episodes()` is purely sequential: playlist `i` gets episode `start + i`. This doesn't account for double-episode playlists (e.g., a ~90 min playlist when episodes are ~45 min).

### Algorithm

1. Compute the **median duration** of the episode-length playlists on disc (`disc.episodes_pl`). Median resists skew from double-episode playlists themselves.
2. For each playlist, compute how many episodes it likely contains:
   - If `playlist.seconds >= median_seconds * 1.5`: `count = floor(playlist.seconds / median_seconds)`
   - Otherwise: `count = 1`
   - This means: < 1.5x = 1 episode, 1.5x–2.49x = 2 episodes, 2.5x–3.49x = 3 episodes, etc.
3. Assign that many consecutive episodes to the playlist, advancing the episode counter accordingly

### Example

Episode-length playlists: 2580s, 2640s, 2700s, 5400s → median = 2670s:
- 2580s (0.97x) → 1 episode → S01E01
- 2640s (0.99x) → 1 episode → S01E02
- 2700s (1.01x) → 1 episode → S01E03
- 5400s (2.02x) → 2 episodes → S01E04-E05

### Updated Signature

```rust
pub fn assign_episodes(
    playlists: &[Playlist],
    episodes: &[Episode],
    start_episode: u32,
) -> HashMap<String, Vec<Episode>>
```

Signature stays the same — the function internally computes the median and applies the 1.5x threshold. This is a best-guess default; the user can override any assignment in the Playlist Manager via `e`.

### Edge Cases

- **All playlists same length:** median = that length, all get 1 episode (correct)
- **Single playlist:** median = its own length, gets 1 episode (correct — can't detect doubles with no baseline)
- **No TMDb episodes available:** function returns empty map (unchanged behavior)
- **More episode slots needed than episodes available:** extra playlists get no assignment (unchanged)

## Specials Support

### Data Model

```rust
// In WizardState
pub specials: HashSet<String>,  // playlist nums marked as special
```

A playlist marked as special:
- Gets episode assignment from season 0 (S00Exx)
- Uses a separate filename template
- Shows `[S]` visual marker in the Playlist Manager

### Naming

Specials use a dedicated format template. Resolution priority:
1. `--format` CLI flag (overrides everything, including specials)
2. `special_format` in config TOML (new field)
3. Default: `{show} S00E{episode} {title}.mkv`

New config field:
```toml
# In ~/.config/bluback/config.toml
special_format = "{show} S00E{episode} {title}.mkv"
```

### Toggle Behavior

When `s` is pressed on a playlist:
- If not already special: mark as special, auto-assign next S00 episode number (max currently assigned S00 episode + 1, starting from 1)
- If already special: unmark, clear its episode assignment (user can re-assign via `e`)
- Unmarking a special does NOT renumber other specials — gaps are allowed

### Format Override Note

`--format` CLI flag overrides everything including specials — this is by design. A user who needs independent control over TV format and specials format should use the config file (`tv_format` + `special_format`).

## Loading Indicators

A rotating spinner character (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` braille pattern, or simple `|/-\` if terminal doesn't support unicode) appended to or replacing the status message during:

- Disc scanning ("Scanning for disc...")
- TMDb search ("Searching TMDb...")
- Season fetch ("Fetching season...")
- Media probe ("Probing media info...")

The spinner is driven by the existing 100ms poll tick. A `spinner_frame: usize` field on `App` increments each tick and indexes into the spinner character array.

## Disc Label on All Screens

Every screen's header block includes the disc label when available:

```
┌─ Step Title ──────────────────────────────────────┐
│ Disc: SHOW_S01_D1  |  context info                │
```

This is already done on some screens. Extend to all wizard screens consistently.

## File Changes

### Modified Files

- `src/tui/mod.rs` — App struct decomposed, Screen enum updated, event loop updated for new screens, background polling updated for Done screen auto-detect
- `src/tui/wizard.rs` — Screen render/handle functions rewritten for merged screens, new Playlist Manager, loading spinners
- `src/tui/dashboard.rs` — Minor: update field access paths for sub-structs (e.g., `app.rip.jobs` instead of `app.rip_jobs`)
- `src/config.rs` — Add `special_format` field to Config
- `src/types.rs` — No structural changes needed (sub-structs are in tui module)

### New Files

None. The sub-structs can live in `src/tui/mod.rs` alongside App.

## Migration Strategy

This is a single-pass refactor, not incremental. The changes are interconnected:
- Sub-struct decomposition touches every field access
- Screen merging changes the flow and all screen transitions
- These can't be done independently without intermediate broken states

However, the work can be **validated incrementally**:
1. Decompose App into sub-structs (compile check — all field accesses updated)
2. Merge TMDb Search + Show Select
3. Simplify Season screen
4. Build Playlist Manager (merge EpisodeMapping + PlaylistSelect + specials)
5. Add loading spinners
6. Add disc label to all screens
7. Enhance Done screen with auto-detect popup
8. Run tests, manual testing with a disc

## Testing

- Existing unit tests (util.rs, disc.rs, rip.rs, chapters.rs, config.rs) should pass unchanged — they test pure functions not TUI code
- Add config test for `special_format` resolution
- Manual testing required for TUI flow (no TUI integration tests exist)
- Test matrix:
  - TV mode: full flow with TMDb
  - TV mode: skip TMDb (Esc)
  - Movie mode: full flow
  - Movie mode: skip TMDb
  - Specials: mark/unmark, verify filename
  - Filtered playlists: toggle visibility, select a short one
  - Auto-detect on Done screen
  - Dry run mode
  - Ctrl+R rescan from various screens
