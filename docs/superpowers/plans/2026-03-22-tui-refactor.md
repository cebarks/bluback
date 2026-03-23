# TUI Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the TUI wizard to merge screens, decompose the App struct, add specials support, multi-episode detection, loading spinners, and auto-detect on Done screen.

**Architecture:** Incremental refactor of the existing enum-based state machine. The `App` god struct is decomposed into `DiscState`, `TmdbState`, `WizardState`, and `RipState` sub-structs. Screens are merged (TMDb Search + Show Select, Episode Mapping + Playlist Select) to reduce wizard steps. The `input_active`/`mapping_edit_row` pattern is replaced by an `InputFocus` enum.

**Tech Stack:** Rust, ratatui, crossterm, serde/toml

**Spec:** `docs/superpowers/specs/2026-03-22-tui-refactor-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/tui/mod.rs` | Modify | App struct → sub-structs, Screen enum, event loop, background polling, `reset_for_rescan`, `start_disc_scan` |
| `src/tui/wizard.rs` | Modify | All wizard screen render + input handlers (merged TMDb, simplified Season, new Playlist Manager) |
| `src/tui/dashboard.rs` | Modify | Update field access paths for sub-structs |
| `src/config.rs` | Modify | Add `special_format` field, `resolve_special_format()` method |
| `src/util.rs` | Modify | Update `assign_episodes()` with median-based multi-episode detection |
| `src/cli.rs` | Modify | Update field access if any shared types change |

No new files — sub-structs live in `src/tui/mod.rs`.

---

### Task 1: Update `assign_episodes()` with multi-episode detection

**Files:**
- Modify: `src/util.rs:182-198` (function body)
- Test: `src/util.rs` (inline `#[cfg(test)]` module)

This task is independent of the TUI refactor and can be done first with full test coverage.

- [ ] **Step 1: Write failing test for multi-episode detection**

Add to the `tests` module in `src/util.rs`:

```rust
#[test]
fn test_assign_double_episode() {
    // Three normal playlists + one double-length
    let playlists = vec![
        Playlist { num: "00001".into(), duration: "0:43:00".into(), seconds: 2580 },
        Playlist { num: "00002".into(), duration: "0:44:00".into(), seconds: 2640 },
        Playlist { num: "00003".into(), duration: "0:45:00".into(), seconds: 2700 },
        Playlist { num: "00004".into(), duration: "1:30:00".into(), seconds: 5400 },
    ];
    let episodes: Vec<Episode> = (1..=5).map(|n| Episode {
        episode_number: n,
        name: format!("Episode {}", n),
        runtime: Some(44),
    }).collect();
    let result = assign_episodes(&playlists, &episodes, 1);
    // First three get one episode each
    assert_eq!(result["00001"].len(), 1);
    assert_eq!(result["00001"][0].episode_number, 1);
    assert_eq!(result["00002"][0].episode_number, 2);
    assert_eq!(result["00003"][0].episode_number, 3);
    // Double-length playlist gets two episodes
    assert_eq!(result["00004"].len(), 2);
    assert_eq!(result["00004"][0].episode_number, 4);
    assert_eq!(result["00004"][1].episode_number, 5);
}

#[test]
fn test_assign_all_same_length_no_doubles() {
    let playlists = vec![
        Playlist { num: "00001".into(), duration: "0:43:00".into(), seconds: 2580 },
        Playlist { num: "00002".into(), duration: "0:44:00".into(), seconds: 2640 },
    ];
    let episodes: Vec<Episode> = (1..=2).map(|n| Episode {
        episode_number: n,
        name: format!("Episode {}", n),
        runtime: Some(44),
    }).collect();
    let result = assign_episodes(&playlists, &episodes, 1);
    assert_eq!(result["00001"].len(), 1);
    assert_eq!(result["00002"].len(), 1);
}

#[test]
fn test_assign_single_playlist_no_double_detect() {
    let playlists = vec![
        Playlist { num: "00001".into(), duration: "1:30:00".into(), seconds: 5400 },
    ];
    let episodes: Vec<Episode> = (1..=2).map(|n| Episode {
        episode_number: n,
        name: format!("Episode {}", n),
        runtime: Some(44),
    }).collect();
    let result = assign_episodes(&playlists, &episodes, 1);
    // Single playlist — can't detect doubles, gets 1 episode
    assert_eq!(result["00001"].len(), 1);
    assert_eq!(result["00001"][0].episode_number, 1);
}

#[test]
fn test_assign_double_episode_exhausts_episodes() {
    let playlists = vec![
        Playlist { num: "00001".into(), duration: "0:44:00".into(), seconds: 2640 },
        Playlist { num: "00002".into(), duration: "1:30:00".into(), seconds: 5400 },
    ];
    let episodes: Vec<Episode> = (1..=2).map(|n| Episode {
        episode_number: n,
        name: format!("Episode {}", n),
        runtime: Some(44),
    }).collect();
    let result = assign_episodes(&playlists, &episodes, 1);
    assert_eq!(result["00001"][0].episode_number, 1);
    // Double wants 2 episodes but only 1 remains
    assert_eq!(result["00002"].len(), 1);
    assert_eq!(result["00002"][0].episode_number, 2);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_assign_double`
Expected: FAIL — current implementation assigns 1 episode per playlist

- [ ] **Step 3: Implement median-based multi-episode detection**

Replace the body of `assign_episodes` in `src/util.rs:182-198`:

```rust
pub fn assign_episodes(
    playlists: &[Playlist],
    episodes: &[Episode],
    start_episode: u32,
) -> HashMap<String, Vec<Episode>> {
    let ep_by_num: HashMap<u32, &Episode> =
        episodes.iter().map(|ep| (ep.episode_number, ep)).collect();

    // Compute median duration for multi-episode detection
    let median_secs = {
        let mut durations: Vec<u32> = playlists.iter().map(|pl| pl.seconds).collect();
        durations.sort();
        if durations.is_empty() {
            0
        } else {
            durations[durations.len() / 2]
        }
    };

    let mut assignments = HashMap::new();
    let mut ep_cursor = start_episode;

    for pl in playlists {
        // Determine how many episodes this playlist likely contains
        let ep_count = if playlists.len() > 1
            && median_secs > 0
            && pl.seconds as f64 >= median_secs as f64 * 1.5
        {
            (pl.seconds / median_secs).max(1)
        } else {
            1
        };

        let mut eps = Vec::new();
        for _ in 0..ep_count {
            if let Some(ep) = ep_by_num.get(&ep_cursor) {
                eps.push((*ep).clone());
            }
            ep_cursor += 1;
        }

        if !eps.is_empty() {
            assignments.insert(pl.num.clone(), eps);
        }
    }

    assignments
}
```

- [ ] **Step 4: Run all tests to verify they pass**

Run: `cargo test`
Expected: ALL tests pass, including existing `test_assign_basic`, `test_assign_offset`, `test_assign_overflow`, `test_assign_empty`, and the new double-episode tests

- [ ] **Step 5: Commit**

```bash
git add src/util.rs
git commit -m "feat: detect multi-episode playlists in auto-assignment

Uses median playlist duration with 1.5x threshold to detect
double-episode playlists and assign multiple episodes accordingly."
```

---

### Task 2: Add `special_format` to Config

**Files:**
- Modify: `src/config.rs:5-7` (add constant), `src/config.rs:16-23` (add field), `src/config.rs:63-87` (add method)
- Test: `src/config.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write failing tests for special format resolution**

Add to the `tests` module in `src/config.rs`:

```rust
#[test]
fn test_parse_special_format() {
    let config: Config = toml::from_str(r#"special_format = "{show} S00E{episode}.mkv""#).unwrap();
    assert_eq!(config.special_format.unwrap(), "{show} S00E{episode}.mkv");
}

#[test]
fn test_resolve_special_format_from_config() {
    let config = Config {
        special_format: Some("custom/{show} S00E{episode}.mkv".into()),
        ..Default::default()
    };
    assert_eq!(
        config.resolve_special_format(None),
        "custom/{show} S00E{episode}.mkv"
    );
}

#[test]
fn test_resolve_special_format_cli_overrides() {
    let config = Config {
        special_format: Some("config/{show}.mkv".into()),
        ..Default::default()
    };
    assert_eq!(
        config.resolve_special_format(Some("cli/{title}.mkv")),
        "cli/{title}.mkv"
    );
}

#[test]
fn test_resolve_special_format_default() {
    let config = Config::default();
    assert_eq!(
        config.resolve_special_format(None),
        DEFAULT_SPECIAL_FORMAT
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_resolve_special_format`
Expected: FAIL — `special_format` field and method don't exist yet

- [ ] **Step 3: Add `special_format` field and resolution method**

In `src/config.rs`, add the constant near the top (after line 6):

```rust
pub const DEFAULT_SPECIAL_FORMAT: &str = "{show} S00E{episode} {title}.mkv";
```

Add the field to `Config` struct (after `movie_format`):

```rust
pub special_format: Option<String>,
```

Add method to `impl Config` (after `resolve_format`):

```rust
pub fn resolve_special_format(&self, cli_format: Option<&str>) -> String {
    if let Some(fmt) = cli_format {
        return fmt.to_string();
    }
    if let Some(ref fmt) = self.special_format {
        return fmt.clone();
    }
    DEFAULT_SPECIAL_FORMAT.to_string()
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: ALL tests pass

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: add special_format config field for specials/extras naming"
```

---

### Task 3: Decompose App struct into sub-structs

**Files:**
- Modify: `src/tui/mod.rs` (entire file — struct definitions, `new()`, `reset_for_rescan()`, `start_disc_scan()`, `poll_background()`, `run_app()`)
- Modify: `src/tui/wizard.rs` (all field access paths)
- Modify: `src/tui/dashboard.rs` (all field access paths)

This is the largest mechanical change. Every `app.field` becomes `app.sub.field`. The logic doesn't change, just the access paths.

- [ ] **Step 1: Define sub-structs and `InputFocus` enum in `src/tui/mod.rs`**

Add before the `App` struct definition. Include `Default` derives:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum InputFocus {
    TextInput,
    List,
    InlineEdit(usize),
}

impl Default for InputFocus {
    fn default() -> Self {
        InputFocus::TextInput
    }
}

pub struct DiscState {
    pub label: String,
    pub label_info: Option<LabelInfo>,
    pub playlists: Vec<Playlist>,
    pub episodes_pl: Vec<Playlist>,
    pub scan_log: Vec<String>,
    pub mount_point: Option<String>,
    pub did_mount: bool,
    pub chapter_counts: HashMap<String, usize>,
}

impl Default for DiscState {
    fn default() -> Self {
        Self {
            label: String::new(),
            label_info: None,
            playlists: Vec::new(),
            episodes_pl: Vec::new(),
            scan_log: Vec::new(),
            mount_point: None,
            did_mount: false,
            chapter_counts: HashMap::new(),
        }
    }
}

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

impl Default for TmdbState {
    fn default() -> Self {
        Self {
            api_key: None,
            search_query: String::new(),
            movie_mode: false,
            search_results: Vec::new(),
            movie_results: Vec::new(),
            selected_show: None,
            selected_movie: None,
            show_name: String::new(),
            episodes: Vec::new(),
        }
    }
}

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
    pub media_infos: Vec<Option<MediaInfo>>,
}

impl Default for WizardState {
    fn default() -> Self {
        Self {
            list_cursor: 0,
            input_buffer: String::new(),
            input_focus: InputFocus::TextInput,
            season_num: None,
            start_episode: None,
            episode_assignments: HashMap::new(),
            playlist_selected: Vec::new(),
            specials: std::collections::HashSet::new(),
            show_filtered: false,
            filenames: Vec::new(),
            media_infos: Vec::new(),
        }
    }
}

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

impl Default for RipState {
    fn default() -> Self {
        Self {
            jobs: Vec::new(),
            current_rip: 0,
            child: None,
            progress_rx: None,
            progress_state: HashMap::new(),
            stderr_buffer: None,
            confirm_abort: false,
            confirm_rescan: false,
        }
    }
}
```

- [ ] **Step 2: Rewrite the `App` struct to use sub-structs**

Replace the current `App` struct with:

```rust
pub struct App {
    pub screen: Screen,
    pub args: Args,
    pub config: crate::config::Config,
    pub quit: bool,
    pub eject: bool,
    pub has_mkvpropedit: bool,
    pub status_message: String,
    pub spinner_frame: usize,

    pub disc: DiscState,
    pub tmdb: TmdbState,
    pub wizard: WizardState,
    pub rip: RipState,

    pub pending_rx: Option<mpsc::Receiver<BackgroundResult>>,
}
```

Update `App::new()` to initialize sub-structs with `Default::default()`.

- [ ] **Step 3: Update `reset_for_rescan()` to use sub-structs**

Replace field-by-field resets with sub-struct resets. Preserve the rip child kill and unmount logic:

```rust
pub fn reset_for_rescan(&mut self) {
    if let Some(ref mut child) = self.rip.child {
        let _ = child.kill();
        let _ = child.wait();
    }
    if self.disc.did_mount {
        let _ = crate::disc::unmount_disc(&self.args.device().to_string_lossy());
    }
    self.disc = DiscState::default();
    self.tmdb.search_query = String::new();
    self.tmdb.movie_mode = false;
    self.tmdb.search_results = Vec::new();
    self.tmdb.movie_results = Vec::new();
    self.tmdb.selected_show = None;
    self.tmdb.selected_movie = None;
    self.tmdb.show_name = String::new();
    self.tmdb.episodes = Vec::new();
    // Keep tmdb.api_key
    self.wizard = WizardState::default();
    self.rip = RipState::default();
    self.status_message = String::new();
    self.spinner_frame = 0;
    self.pending_rx = None;
}
```

- [ ] **Step 4: Update `start_disc_scan()` and `poll_background()` field access**

Mechanically update all `app.field` references to `app.sub.field`. Key mappings:
- `app.label` → `app.disc.label`
- `app.label_info` → `app.disc.label_info`
- `app.playlists` → `app.disc.playlists`
- `app.episodes_pl` → `app.disc.episodes_pl`
- `app.scan_log` → `app.disc.scan_log`
- `app.search_query` → `app.tmdb.search_query`
- `app.movie_mode` → `app.tmdb.movie_mode`
- `app.search_results` → `app.tmdb.search_results`
- `app.movie_results` → `app.tmdb.movie_results`
- `app.selected_show` → `app.tmdb.selected_show`
- `app.selected_movie` → `app.tmdb.selected_movie`
- `app.show_name` → `app.tmdb.show_name`
- `app.episodes` → `app.tmdb.episodes`
- `app.api_key` → `app.tmdb.api_key`
- `app.season_num` → `app.wizard.season_num`
- `app.start_episode` → `app.wizard.start_episode`
- `app.episode_assignments` → `app.wizard.episode_assignments`
- `app.playlist_selected` → `app.wizard.playlist_selected`
- `app.filenames` → `app.wizard.filenames`
- `app.media_infos` → `app.wizard.media_infos`
- `app.list_cursor` → `app.wizard.list_cursor`
- `app.input_buffer` → `app.wizard.input_buffer`
- `app.input_active` → check against `app.wizard.input_focus` (e.g., `matches!(app.wizard.input_focus, InputFocus::TextInput | InputFocus::InlineEdit(_))`)
- `app.season_field` → removed (Season screen simplified)
- `app.mapping_edit_row` → `InputFocus::InlineEdit(row)`
- `app.rip_jobs` → `app.rip.jobs`
- `app.current_rip` → `app.rip.current_rip`
- `app.rip_child` → `app.rip.child`
- `app.progress_rx` → `app.rip.progress_rx`
- `app.progress_state` → `app.rip.progress_state`
- `app.stderr_buffer` → `app.rip.stderr_buffer`
- `app.confirm_abort` → `app.rip.confirm_abort`
- `app.confirm_rescan` → `app.rip.confirm_rescan`
- `app.mount_point` → `app.disc.mount_point`
- `app.did_mount` → `app.disc.did_mount`
- `app.chapter_counts` → `app.disc.chapter_counts`

- [ ] **Step 5: Update `run_app()` event loop**

Update the global keybind checks to use `input_focus`:
- Replace `!app.input_active` checks with `matches!(app.wizard.input_focus, InputFocus::List)`
- Update `app.confirm_rescan` → `app.rip.confirm_rescan`
- Update `app.confirm_abort` → `app.rip.confirm_abort`

Also update the `Screen` enum — keep current variants for now (the screen merging happens in later tasks). This task only moves fields into sub-structs.

- [ ] **Step 6: Update `src/tui/wizard.rs` field access paths**

Mechanically update all `app.field` → `app.sub.field` using the mapping above. This is the largest file. Every render function and every input handler needs updating.

Also update `playlist_filename()` similarly.

- [ ] **Step 7: Update `src/tui/dashboard.rs` field access paths**

Same mechanical update for dashboard functions:
- `app.rip_jobs` → `app.rip.jobs`
- `app.current_rip` → `app.rip.current_rip`
- `app.rip_child` → `app.rip.child`
- `app.progress_rx` → `app.rip.progress_rx`
- etc.

- [ ] **Step 8: Update `src/cli.rs` if needed**

Check if `cli.rs` references any fields that moved. It uses its own local variables for most things, but `assign_episodes` is called — verify it still compiles.

- [ ] **Step 9: Run all tests and compile check**

Run: `cargo test`
Run: `cargo clippy --locked -- -D warnings`
Expected: ALL tests pass, no warnings

- [ ] **Step 10: Commit**

```bash
git add src/tui/mod.rs src/tui/wizard.rs src/tui/dashboard.rs src/cli.rs
git commit -m "refactor: decompose App struct into DiscState, TmdbState, WizardState, RipState"
```

---

### Task 4: Update Screen enum and merge TMDb Search + Show Select

**Files:**
- Modify: `src/tui/mod.rs` (Screen enum, event loop match arms)
- Modify: `src/tui/wizard.rs` (replace `render_tmdb_search`, `handle_tmdb_search_input`, `render_show_select`, `handle_show_select_input` with unified versions)

- [ ] **Step 1: Update Screen enum**

In `src/tui/mod.rs`, replace:
```rust
pub enum Screen {
    Scanning,
    TmdbSearch,
    ShowSelect,
    SeasonEpisode,
    EpisodeMapping,
    PlaylistSelect,
    Confirm,
    Ripping,
    Done,
}
```
with:
```rust
pub enum Screen {
    Scanning,
    TmdbSearch,       // merged: search input + inline results
    Season,           // simplified: just season number
    PlaylistManager,  // merged: playlist select + episode mapping
    Confirm,
    Ripping,
    Done,
}
```

- [ ] **Step 2: Update event loop match arms in `run_app()`**

Remove `Screen::ShowSelect`, `Screen::SeasonEpisode`, `Screen::EpisodeMapping`, `Screen::PlaylistSelect` arms. Add `Screen::Season`, `Screen::PlaylistManager` arms. Wire to new handler functions (which will be written in the next steps).

For now, create stub render/handle functions in wizard.rs so it compiles:
```rust
pub fn render_tmdb_search(f: &mut Frame, app: &App) { /* TODO */ }
pub fn handle_tmdb_search_input(app: &mut App, key: KeyEvent) { /* TODO */ }
pub fn render_season(f: &mut Frame, app: &App) { /* TODO */ }
pub fn handle_season_input(app: &mut App, key: KeyEvent) { /* TODO */ }
pub fn render_playlist_manager(f: &mut Frame, app: &App) { /* TODO */ }
pub fn handle_playlist_manager_input(app: &mut App, key: KeyEvent) { /* TODO */ }
```

- [ ] **Step 3: Implement merged TMDb Search screen render**

Replace the stub `render_tmdb_search` with the full implementation. Layout:
- Header: disc label + playlist count + mode label
- Search input field (always visible)
- Results list below (visible after search)
- Hints bar at bottom

The input field shows cursor when `InputFocus::TextInput`. The results list shows `>` marker on `list_cursor` item when `InputFocus::List`.

Key detail: when `app.tmdb.api_key.is_none()`, show API key input prompt instead (same as current behavior).

- [ ] **Step 4: Implement merged TMDb Search input handler**

Replace stub `handle_tmdb_search_input`. Focus-aware behavior:

When `InputFocus::TextInput`:
- `Char(c)` → append to `input_buffer`
- `Backspace` → pop from `input_buffer`
- `Enter` → if no API key, save key; else spawn TMDb search in background
- `Down` → if results exist, switch to `InputFocus::List`, set `list_cursor = 0`
- `Tab` → toggle `movie_mode`
- `Esc` → skip TMDb entirely, go to `PlaylistManager`

When `InputFocus::List`:
- `Up` → if `list_cursor == 0`, switch back to `InputFocus::TextInput`; else `list_cursor -= 1`
- `Down` → `list_cursor += 1` (clamped)
- `Enter` → select show/movie at cursor:
  - Movie mode: set `selected_movie`, `show_name`, go to `PlaylistManager`
  - TV mode: set `selected_show`, `show_name`, fetch season if `season_num` is set, go to `Season`
- `Esc` → clear results, switch back to `InputFocus::TextInput`

- [ ] **Step 5: Handle background results for TMDb**

In `poll_background()`, the `ShowSearch` and `MovieSearch` result handlers should switch to `InputFocus::List` and keep `screen` as `TmdbSearch` (no separate `ShowSelect` screen). Update these handlers.

- [ ] **Step 6: Run tests and compile check**

Run: `cargo test && cargo clippy --locked -- -D warnings`
Expected: ALL pass

- [ ] **Step 7: Commit**

```bash
git add src/tui/mod.rs src/tui/wizard.rs
git commit -m "feat: merge TMDb Search and Show Select into single screen with inline results"
```

---

### Task 5: Simplify Season screen

**Files:**
- Modify: `src/tui/wizard.rs` (replace stubs with implementations)

- [ ] **Step 1: Implement Season screen render**

Replace `render_season` stub. Layout:
- Header: disc label + show name
- Single season number input field
- Episode list preview below (when fetched)
- Hints bar

No starting episode field. Just the season number.

- [ ] **Step 2: Implement Season screen input handler**

Replace `handle_season_input` stub:
- `Char(c)` → append digit to `input_buffer`
- `Backspace` → pop
- `Enter` →
  - If episodes not yet fetched: parse season number, spawn `SeasonFetch` background task
  - If episodes already fetched: auto-assign using `assign_episodes()` with `guess_start_episode()`, go to `PlaylistManager`
- `Esc` → go back to `TmdbSearch`

- [ ] **Step 3: Update `poll_background()` for `SeasonFetch`**

When `SeasonFetch` result arrives, store episodes in `app.tmdb.episodes`. Compute and store the guessed start episode. The user can proceed with Enter.

- [ ] **Step 4: Run tests and compile check**

Run: `cargo test && cargo clippy --locked -- -D warnings`

- [ ] **Step 5: Commit**

```bash
git add src/tui/wizard.rs src/tui/mod.rs
git commit -m "feat: simplify Season screen to single field, remove starting episode input"
```

---

### Task 6: Build Playlist Manager (combined screen)

**Files:**
- Modify: `src/tui/wizard.rs` (replace stubs)
- Modify: `src/tui/mod.rs` (playlist_selected init, poll_background MediaProbe handling)

This is the core of the refactor.

- [ ] **Step 1: Update playlist_selected initialization**

In `poll_background()` where `DiscScan` result is handled, change `playlist_selected` to be sized to `disc.playlists.len()`:

```rust
// Mark episode-length playlists as selected, rest as deselected
app.wizard.playlist_selected = app.disc.playlists.iter().map(|pl| {
    app.disc.episodes_pl.iter().any(|ep| ep.num == pl.num)
}).collect();
```

- [ ] **Step 2: Implement Playlist Manager render**

Replace `render_playlist_manager` stub. Layout per spec:
- Header: disc label, show name, selected count, hidden count
- Table with columns: cursor+checkbox, #, playlist, duration, chapters, episode(s), filename
- When `show_filtered` is false, only show playlists that are in `episodes_pl`
- When `show_filtered` is true, show all playlists — dimmed style for filtered ones
- `[S]` marker for specials
- Inline edit cursor when `InputFocus::InlineEdit(row)`
- Hints bar showing available hotkeys

Helper function needed: `visible_playlists(app) -> Vec<(usize, &Playlist)>` — returns `(index_in_disc_playlists, playlist)` for currently visible rows. Used by both render and input handler.

- [ ] **Step 3: Implement Playlist Manager input handler**

Replace `handle_playlist_manager_input` stub. Focus-aware:

When `InputFocus::List` (default for this screen):
- `Up/Down` → move cursor through visible playlists
- `Space` → toggle `playlist_selected[real_index]` for the highlighted playlist
- `e` → switch to `InputFocus::InlineEdit(cursor)`, pre-fill `input_buffer` with current assignment
- `s` → toggle special on highlighted playlist (add/remove from `wizard.specials`, update episode assignment)
- `f` → toggle `show_filtered`, clamp cursor if needed
- `Enter` → validate at least one selected, spawn media probes, go to `Confirm`
- `Esc` → go back to `Season` (TV) or `TmdbSearch` (Movie)

When `InputFocus::InlineEdit(row)`:
- `Char(c)` → append digit/comma/dash to `input_buffer`
- `Backspace` → pop
- `Enter` → parse input with `parse_episode_input()`, update `episode_assignments`, switch back to `InputFocus::List`
- `Esc` → cancel, switch back to `InputFocus::List`

- [ ] **Step 4: Implement specials toggle logic**

When `s` is pressed on a playlist:
```rust
let pl_num = &visible[cursor].num;
if app.wizard.specials.contains(pl_num) {
    app.wizard.specials.remove(pl_num);
    app.wizard.episode_assignments.remove(pl_num);
} else {
    app.wizard.specials.insert(pl_num.clone());
    let next_ep = app.wizard.specials.iter()
        .filter_map(|s| app.wizard.episode_assignments.get(s))
        .flat_map(|eps| eps.iter().map(|e| e.episode_number))
        .max()
        .unwrap_or(0) + 1;
    app.wizard.episode_assignments.insert(pl_num.clone(), vec![Episode {
        episode_number: next_ep,
        name: String::new(),
        runtime: None,
    }]);
}
```

- [ ] **Step 5: Update `playlist_filename()` for specials**

In `src/tui/wizard.rs`, modify `playlist_filename()` to check if the playlist is in `app.wizard.specials`. If so, use `config.resolve_special_format()` and season 0 for the filename.

- [ ] **Step 6: Update media probe spawning**

In the Playlist Manager's Enter handler, spawn media probes for selected playlists. Update `poll_background()` `MediaProbe` handler to work with the new indexing (selected from `disc.playlists` not just `episodes_pl`).

- [ ] **Step 7: Run tests and compile check**

Run: `cargo test && cargo clippy --locked -- -D warnings`

- [ ] **Step 8: Commit**

```bash
git add src/tui/wizard.rs src/tui/mod.rs
git commit -m "feat: build Playlist Manager combining playlist select, episode mapping, and specials"
```

---

### Task 7: Add loading spinners

**Files:**
- Modify: `src/tui/mod.rs` (spinner tick in event loop)
- Modify: `src/tui/wizard.rs` (render functions for Scanning, TMDb Search, Season)

- [ ] **Step 1: Add spinner tick to event loop**

In `run_app()`, increment `app.spinner_frame` on every poll tick (the 100ms loop iteration), but only when a background task is active:

```rust
if app.pending_rx.is_some() {
    app.spinner_frame = app.spinner_frame.wrapping_add(1);
}
```

- [ ] **Step 2: Add spinner helper function**

Add to `src/tui/wizard.rs`:

```rust
const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn spinner_char(frame: usize) -> char {
    SPINNER_CHARS[frame % SPINNER_CHARS.len()]
}
```

- [ ] **Step 3: Use spinner in render functions**

In `render_scanning`, `render_tmdb_search`, and `render_season`, when `app.pending_rx.is_some()`, prepend the spinner character to the status message:

```rust
if app.pending_rx.is_some() && !app.status_message.is_empty() {
    format!("{} {}", spinner_char(app.spinner_frame), app.status_message)
} else {
    app.status_message.clone()
}
```

- [ ] **Step 4: Run and verify visually**

Run: `cargo build && cargo clippy --locked -- -D warnings`

- [ ] **Step 5: Commit**

```bash
git add src/tui/mod.rs src/tui/wizard.rs
git commit -m "feat: add loading spinners during background operations"
```

---

### Task 8: Add disc label to all screens

**Files:**
- Modify: `src/tui/wizard.rs` (render functions)

- [ ] **Step 1: Add disc label helper**

Add helper to `src/tui/wizard.rs`:

```rust
fn disc_label_text(app: &App) -> String {
    if app.disc.label.is_empty() {
        String::new()
    } else {
        format!("Disc: {}", app.disc.label)
    }
}
```

- [ ] **Step 2: Update all render functions**

Ensure every screen's header includes the disc label via the helper. Screens that already show it (TMDb Search) just need to use the helper. Add it to:
- `render_scanning` (after disc is detected)
- `render_season`
- `render_playlist_manager`
- `render_confirm`

The dashboard and done screens can also show it if available.

- [ ] **Step 3: Run and verify**

Run: `cargo clippy --locked -- -D warnings`

- [ ] **Step 4: Commit**

```bash
git add src/tui/wizard.rs src/tui/dashboard.rs
git commit -m "feat: show disc label consistently on all wizard screens"
```

---

### Task 9: Auto-detect new disc on Done screen

**Files:**
- Modify: `src/tui/mod.rs` (App field, poll_background, Done screen handling in event loop)
- Modify: `src/tui/dashboard.rs` (render_done popup overlay)

- [ ] **Step 1: Add popup state to App**

Add to `App` struct:

```rust
pub disc_detected_label: Option<String>,  // label of newly detected disc on Done screen
```

Initialize to `None` in `App::new()` and `reset_for_rescan()`.

- [ ] **Step 2: Start disc polling on Done screen transition**

In `dashboard.rs` `check_all_done()`, after setting `app.screen = Screen::Done`, spawn disc polling:

```rust
// Start polling for next disc
super::start_disc_scan(app);
```

Note: `start_disc_scan` needs to be changed to `pub(crate)` in `src/tui/mod.rs` for this to compile.

- [ ] **Step 3: Update `poll_background` for Done screen**

In `poll_background()`, handle `DiscFound` when `app.screen == Screen::Done`:

```rust
BackgroundResult::DiscFound(ref device) => {
    if app.screen == Screen::Done {
        let label = crate::disc::get_volume_label(device);
        app.disc_detected_label = Some(if label.is_empty() {
            device.clone()
        } else {
            label
        });
        return; // Keep polling alive
    }
    // ... existing behavior for Scanning screen
}
```

Also handle `DiscScan` when on Done screen — ignore it (we only want the detection notification, not a full scan).

- [ ] **Step 4: Render popup overlay in `render_done`**

In `dashboard.rs` `render_done()`, after rendering the normal content, check `app.disc_detected_label`. If `Some`, render a centered popup:

```rust
if let Some(ref label) = app.disc_detected_label {
    let popup_area = centered_rect(50, 5, f.area());
    f.render_widget(Clear, popup_area);
    let popup_text = format!("New disc detected: {}\nPress Enter to start, any other key to exit", label);
    let popup = Paragraph::new(popup_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title("New Disc"));
    f.render_widget(popup, popup_area);
}
```

Add the `centered_rect` helper:

```rust
fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_width = area.width * percent_x / 100;
    let x = (area.width - popup_width) / 2;
    let y = (area.height - height) / 2;
    Rect::new(area.x + x, area.y + y, popup_width, height)
}
```

- [ ] **Step 5: Update Done screen input handling**

In `run_app()` event loop, update the `Screen::Done` match arm:

```rust
Screen::Done => {
    if app.disc_detected_label.is_some() {
        // Popup is showing
        if key.code == KeyCode::Enter {
            app.disc_detected_label = None;
            app.reset_for_rescan();
            app.tmdb.api_key = crate::tmdb::get_api_key(config);
            start_disc_scan(&mut app);
        } else {
            // Any other key exits
            let all_succeeded = app.rip.jobs.iter()
                .all(|j| matches!(j.status, PlaylistStatus::Done(_)));
            if app.eject && !app.rip.jobs.is_empty() && all_succeeded {
                let device = app.args.device().to_string_lossy().to_string();
                let _ = crate::disc::eject_disc(&device);
            }
            app.quit = true;
        }
    } else if key.code == KeyCode::Enter {
        app.reset_for_rescan();
        app.tmdb.api_key = crate::tmdb::get_api_key(config);
        start_disc_scan(&mut app);
    } else {
        // existing exit behavior
        let all_succeeded = app.rip.jobs.iter()
            .all(|j| matches!(j.status, PlaylistStatus::Done(_)));
        if app.eject && !app.rip.jobs.is_empty() && all_succeeded {
            let device = app.args.device().to_string_lossy().to_string();
            let _ = crate::disc::eject_disc(&device);
        }
        app.quit = true;
    }
}
```

- [ ] **Step 6: Run tests and compile check**

Run: `cargo test && cargo clippy --locked -- -D warnings`

- [ ] **Step 7: Commit**

```bash
git add src/tui/mod.rs src/tui/dashboard.rs
git commit -m "feat: auto-detect new disc on Done screen with popup prompt"
```

---

### Task 10: Clean up old code and final verification

**Files:**
- Modify: `src/tui/wizard.rs` (remove dead code)
- Modify: `src/tui/mod.rs` (remove dead code)

- [ ] **Step 1: Remove old screen functions**

Delete the following functions from `wizard.rs` if they still exist:
- `render_show_select` / `handle_show_select_input`
- `render_season_episode` / `handle_season_episode_input`
- `render_episode_mapping` / `handle_episode_mapping_input`
- `render_playlist_select` / `handle_playlist_select_input`

- [ ] **Step 2: Remove unused imports and dead fields**

Run: `cargo clippy --locked -- -D warnings`

Fix any warnings about unused imports, dead code, or unnecessary fields.

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: ALL tests pass

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --locked -- -D warnings`
Expected: No warnings

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: clean up old screen code and dead imports"
```

- [ ] **Step 6: Manual testing checklist**

Test the following scenarios (requires a Blu-ray disc or mock):
- [ ] TV mode: full flow with TMDb (search → select → season → playlist manager → confirm → rip)
- [ ] TV mode: skip TMDb with Esc (goes directly to playlist manager)
- [ ] Movie mode: full flow
- [ ] Movie mode: skip TMDb
- [ ] Specials: press `s` to mark, verify `[S]` marker and S00 filename
- [ ] Specials: unmark with `s`, verify assignment cleared
- [ ] Filtered playlists: press `f` to show, verify dimmed, select one
- [ ] Episode edit: press `e`, type `3-4`, verify multi-episode assignment
- [ ] Multi-episode auto-detection: disc with a double-length playlist gets 2 episodes
- [ ] Loading spinner visible during scan and TMDb search
- [ ] Disc label visible on all screens
- [ ] Done screen: insert new disc, verify popup appears
- [ ] Ctrl+R rescan from various screens
- [ ] Dry run mode
- [ ] `--no-tui` CLI mode still works (cli.rs unchanged)
