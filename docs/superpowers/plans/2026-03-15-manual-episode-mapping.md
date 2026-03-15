# Manual Episode Mapping Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow manual playlist-to-episode mapping with multi-episode support, adding a new TUI wizard screen and CLI prompt for reviewing and editing auto-assigned episode mappings.

**Architecture:** Change `EpisodeAssignments` from `HashMap<String, Episode>` to `HashMap<String, Vec<Episode>>` and `RipJob.episode` from `Option<Episode>` to `Vec<Episode>`. Add `parse_episode_input()` for parsing manual episode input. Add a new `EpisodeMapping` wizard screen between `SeasonEpisode` and `PlaylistSelect`. Update CLI mode with an accept/manual mapping prompt loop.

**Tech Stack:** Rust, ratatui, crossterm

**Spec:** `docs/superpowers/specs/2026-03-15-manual-episode-mapping-design.md`

**Important:** Line numbers in each task reference the file state *before* any tasks are applied. Since earlier tasks insert/remove lines, later tasks should locate code by function/struct name rather than relying on exact line numbers. Line numbers are provided as hints for the original file.

---

## Chunk 1: Data Model & Pure Functions

### Task 1: Add `parse_episode_input()` with tests

**Files:**
- Modify: `src/util.rs`

- [ ] **Step 1: Write failing tests for `parse_episode_input()`**

Add these tests inside the existing `#[cfg(test)] mod tests` block at the end of `src/util.rs` (before the final closing `}`):

```rust
#[test]
fn test_parse_episode_input_single() {
    assert_eq!(parse_episode_input("3"), Some(vec![3]));
}

#[test]
fn test_parse_episode_input_range() {
    assert_eq!(parse_episode_input("3-5"), Some(vec![3, 4, 5]));
}

#[test]
fn test_parse_episode_input_comma() {
    assert_eq!(parse_episode_input("3,5"), Some(vec![3, 5]));
}

#[test]
fn test_parse_episode_input_mixed() {
    assert_eq!(parse_episode_input("1,3-5"), Some(vec![1, 3, 4, 5]));
}

#[test]
fn test_parse_episode_input_reversed_range() {
    assert_eq!(parse_episode_input("5-3"), None);
}

#[test]
fn test_parse_episode_input_zero() {
    assert_eq!(parse_episode_input("0"), None);
}

#[test]
fn test_parse_episode_input_empty() {
    assert_eq!(parse_episode_input(""), Some(vec![]));
}

#[test]
fn test_parse_episode_input_non_numeric() {
    assert_eq!(parse_episode_input("abc"), None);
}

#[test]
fn test_parse_episode_input_whitespace() {
    assert_eq!(parse_episode_input(" 3 , 5 "), Some(vec![3, 5]));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test parse_episode_input`
Expected: FAIL — `parse_episode_input` not found

- [ ] **Step 3: Implement `parse_episode_input()`**

Add this function in `src/util.rs` after the `parse_selection()` function (after line 144):

```rust
pub fn parse_episode_input(text: &str) -> Option<Vec<u32>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(vec![]);
    }

    let mut episodes = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let (start_s, end_s) = part.split_once('-')?;
            let start: u32 = start_s.trim().parse().ok()?;
            let end: u32 = end_s.trim().parse().ok()?;
            if start == 0 || end == 0 || start > end {
                return None;
            }
            episodes.extend(start..=end);
        } else {
            let val: u32 = part.parse().ok()?;
            if val == 0 {
                return None;
            }
            episodes.push(val);
        }
    }

    Some(episodes)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test parse_episode_input`
Expected: All 9 tests PASS

- [ ] **Step 5: Commit**

Suggest: `feat: add parse_episode_input() for manual episode mapping`

---

### Task 2: Change `EpisodeAssignments` type and update `assign_episodes()`

**Files:**
- Modify: `src/types.rs` (line 119 — `EpisodeAssignments` type alias)
- Modify: `src/util.rs` (lines 153-169 — `assign_episodes` function)

- [ ] **Step 1: Update the type alias in `types.rs`**

In `src/types.rs`, change line 119:

```rust
// Old:
pub type EpisodeAssignments = HashMap<String, Episode>;
// New:
pub type EpisodeAssignments = HashMap<String, Vec<Episode>>;
```

- [ ] **Step 2: Update `assign_episodes()` in `util.rs`**

Find the `assign_episodes` function in `src/util.rs` (originally at line 153) and replace it:

```rust
pub fn assign_episodes(
    playlists: &[Playlist],
    episodes: &[Episode],
    start_episode: u32,
) -> EpisodeAssignments {
    let ep_by_num: HashMap<u32, &Episode> =
        episodes.iter().map(|ep| (ep.episode_number, ep)).collect();

    let mut assignments = EpisodeAssignments::new();
    for (i, pl) in playlists.iter().enumerate() {
        let ep_num = start_episode + i as u32;
        if let Some(ep) = ep_by_num.get(&ep_num) {
            assignments.insert(pl.num.clone(), vec![(*ep).clone()]);
        }
    }
    assignments
}
```

- [ ] **Step 3: Update existing `assign_episodes` tests**

In `src/util.rs`, update the test assertions to work with `Vec<Episode>`. Find each test by name and update the assertions:

`test_assign_basic` — change assertions:
```rust
    assert_eq!(result["00001"][0].name, "Pilot");
    assert_eq!(result["00002"][0].name, "Second");
```

`test_assign_offset` — change assertion:
```rust
    assert_eq!(result["00003"][0].name, "Third");
```

`test_assign_overflow` — change assertion:
```rust
    assert_eq!(result["00001"][0].name, "Pilot");
```

`test_assign_empty` — no changes needed (checks `result.is_empty()`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test assign`
Expected: All assign tests PASS

- [ ] **Step 5: Commit**

Suggest: `refactor: change EpisodeAssignments to HashMap<String, Vec<Episode>>`

---

### Task 3: Update `make_filename()` for multi-episode support

**Files:**
- Modify: `src/util.rs` (`make_filename` function, originally at lines 208-246)

- [ ] **Step 1: Write failing tests for multi-episode filenames**

Add these tests in the `tests` module of `src/util.rs`:

```rust
#[test]
fn test_make_filename_multi_episode_consecutive() {
    let eps = vec![
        Episode {
            episode_number: 3,
            name: "Third".into(),
            runtime: Some(44),
        },
        Episode {
            episode_number: 4,
            name: "Fourth".into(),
            runtime: Some(44),
        },
    ];
    assert_eq!(
        make_filename("00001", &eps, 1, None, None, None),
        "S01E03-E04_Third.mkv"
    );
}

#[test]
fn test_make_filename_multi_episode_non_consecutive() {
    let eps = vec![
        Episode {
            episode_number: 3,
            name: "Third".into(),
            runtime: Some(44),
        },
        Episode {
            episode_number: 5,
            name: "Fifth".into(),
            runtime: Some(44),
        },
    ];
    assert_eq!(
        make_filename("00001", &eps, 1, None, None, None),
        "S01E03-E05_Third.mkv"
    );
}

#[test]
fn test_make_filename_multi_episode_custom_format() {
    let eps = vec![
        Episode {
            episode_number: 3,
            name: "Third".into(),
            runtime: Some(44),
        },
        Episode {
            episode_number: 4,
            name: "Fourth".into(),
            runtime: Some(44),
        },
    ];
    let mut extra = HashMap::new();
    extra.insert("show", "Test Show".to_string());
    assert_eq!(
        make_filename(
            "00001",
            &eps,
            1,
            Some("{show}/S{season}E{episode} - {title}.mkv"),
            None,
            Some(&extra)
        ),
        "Test Show/S01E03-E04 - Third.mkv"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test make_filename_multi`
Expected: FAIL — type mismatch (function still expects `Option<&Episode>`)

- [ ] **Step 3: Update `make_filename()` signature and implementation**

Find the `make_filename` function in `src/util.rs` (originally at line 208) and replace it:

```rust
pub fn make_filename(
    playlist_num: &str,
    episodes: &[Episode],
    season: u32,
    format: Option<&str>,
    media_info: Option<&MediaInfo>,
    extra_vars: Option<&HashMap<&str, String>>,
) -> String {
    if episodes.is_empty() {
        return format!("playlist{}.mkv", playlist_num);
    }

    let ep = &episodes[0];

    let episode_str = if episodes.len() > 1 {
        let last = &episodes[episodes.len() - 1];
        format!("{:02}-E{:02}", ep.episode_number, last.episode_number)
    } else {
        format!("{:02}", ep.episode_number)
    };

    let Some(fmt) = format else {
        return format!(
            "S{:02}E{}_{}.mkv",
            season,
            episode_str,
            sanitize_filename(&ep.name)
        );
    };

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("season", format!("{:02}", season));
    vars.insert("episode", episode_str);
    vars.insert("title", ep.name.clone());
    vars.insert("playlist", playlist_num.to_string());

    if let Some(info) = media_info {
        vars.extend(info.to_vars());
    }
    if let Some(extra) = extra_vars {
        for (k, v) in extra {
            vars.insert(k, v.clone());
        }
    }

    render_template(fmt, &vars)
}
```

- [ ] **Step 4: Update existing `make_filename` tests to use new signature**

Find each test by name and update:

`test_make_filename_with_episode`:
```rust
#[test]
fn test_make_filename_with_episode() {
    let ep = Episode {
        episode_number: 3,
        name: "The Pilot".into(),
        runtime: Some(44),
    };
    assert_eq!(
        make_filename("00001", &[ep], 1, None, None, None),
        "S01E03_The_Pilot.mkv"
    );
}
```

`test_make_filename_no_episode`:
```rust
#[test]
fn test_make_filename_no_episode() {
    assert_eq!(
        make_filename("00042", &[], 1, None, None, None),
        "playlist00042.mkv"
    );
}
```

`test_make_filename_custom_format_with_show`:
```rust
#[test]
fn test_make_filename_custom_format_with_show() {
    let ep = Episode {
        episode_number: 3,
        name: "The Pilot".into(),
        runtime: Some(44),
    };
    let mut extra = HashMap::new();
    extra.insert("show", "Test Show".to_string());
    assert_eq!(
        make_filename(
            "00001",
            &[ep],
            1,
            Some("{show}/S{season}E{episode} - {title}.mkv"),
            None,
            Some(&extra)
        ),
        "Test Show/S01E03 - The Pilot.mkv"
    );
}
```

- [ ] **Step 5: Run all tests to verify they pass**

Run: `cargo test make_filename`
Expected: All make_filename tests PASS

- [ ] **Step 6: Commit**

Suggest: `feat: update make_filename() for multi-episode support`

---

### Task 4: Update `RipJob.episode` type and dashboard rendering

This task changes the `RipJob` type AND fixes the dashboard call site in the same commit so the code always compiles.

**Files:**
- Modify: `src/types.rs` (lines 63-68 — `RipJob` struct)
- Modify: `src/tui/dashboard.rs` (lines 53-57 — episode rendering)

- [ ] **Step 1: Change `RipJob.episode` from `Option<Episode>` to `Vec<Episode>`**

In `src/types.rs`, find `RipJob` and change the `episode` field:

```rust
#[derive(Debug, Clone)]
pub struct RipJob {
    pub playlist: Playlist,
    pub episode: Vec<Episode>,
    pub filename: String,
    pub status: PlaylistStatus,
}
```

- [ ] **Step 2: Update dashboard episode rendering**

In `src/tui/dashboard.rs`, find the `ep_name` block (originally lines 53-57) and replace:

```rust
            let ep_name = if job.episode.is_empty() {
                String::new()
            } else if job.episode.len() == 1 {
                format!("E{:02} {}", job.episode[0].episode_number, job.episode[0].name)
            } else {
                let first = &job.episode[0];
                let last = &job.episode[job.episode.len() - 1];
                format!("E{:02}-E{:02} {}", first.episode_number, last.episode_number, first.name)
            };
```

- [ ] **Step 3: Run `cargo check` to see remaining compilation errors**

Run: `cargo check 2>&1 | head -30`
Expected: Errors in `cli.rs` and `wizard.rs` (will be fixed in subsequent tasks). Dashboard and types should be clean.

- [ ] **Step 4: Commit (will be amended in next task)**

Do NOT commit yet — the code doesn't compile. Continue to Task 5.

---

## Chunk 2: TUI Call Site Updates

### Task 5: Update `playlist_filename()` and `PlaylistSelect` screen

**Files:**
- Modify: `src/tui/wizard.rs`

- [ ] **Step 1: Update `playlist_filename()` call to `make_filename()`**

In `src/tui/wizard.rs`, find `playlist_filename()` and replace the `make_filename` call (originally lines 69-77 — the `} else {` branch):

```rust
    } else {
        let episodes = app
            .episode_assignments
            .get(&pl.num)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        make_filename(
            &pl.num,
            episodes,
            app.season_num.unwrap_or(0),
            fmt,
            media_info,
            Some(&extra),
        )
    }
```

- [ ] **Step 2: Update `render_playlist_select()` episode info rendering**

In `src/tui/wizard.rs`, find the `ep_info` block inside `render_playlist_select` (originally lines 601-610) and replace:

```rust
            let ep_info = if let Some(eps) = app.episode_assignments.get(&pl.num) {
                if eps.len() == 1 {
                    format!(
                        "S{:02}E{:02} - {}",
                        app.season_num.unwrap_or(0),
                        eps[0].episode_number,
                        eps[0].name
                    )
                } else if eps.len() > 1 {
                    let first = &eps[0];
                    let last = &eps[eps.len() - 1];
                    format!(
                        "S{:02}E{:02}-E{:02} - {}",
                        app.season_num.unwrap_or(0),
                        first.episode_number,
                        last.episode_number,
                        first.name
                    )
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
```

- [ ] **Step 3: Update step numbers in `render_playlist_select()`**

Find the step title in `render_playlist_select` (originally lines 572-576) and change TV mode from "Step 4" to "Step 5":

```rust
            .title(if app.movie_mode {
                "Step 3: Select Playlists"
            } else {
                "Step 5: Select Playlists"
            }),
```

- [ ] **Step 4: Update `PlaylistSelect` Esc navigation**

Find the `KeyCode::Esc` arm of `handle_playlist_select_input` (originally lines 714-731) and replace:

```rust
        KeyCode::Esc => {
            app.list_cursor = 0;
            if app.movie_mode && app.selected_movie.is_some() {
                app.screen = Screen::ShowSelect;
            } else if !app.episode_assignments.is_empty() {
                app.screen = Screen::EpisodeMapping;
            } else {
                app.input_active = true;
                app.input_buffer = app.search_query.clone();
                app.screen = Screen::TmdbSearch;
            }
        }
```

- [ ] **Step 5: Commit**

Suggest: `fix: update playlist select screen for Vec<Episode>`

---

### Task 6: Update confirm screen job building

**Files:**
- Modify: `src/tui/wizard.rs`

- [ ] **Step 1: Update job building in `handle_confirm_input()`**

Find line 844 in `handle_confirm_input()` (the `let episode = ...` line) and replace:

```rust
                let episode = app.episode_assignments.get(&pl.num).cloned().unwrap_or_default();
```

- [ ] **Step 2: Update step number in `render_confirm()`**

Find the step title in `render_confirm` (originally lines 748-751) and change TV mode from "Step 5" to "Step 6":

```rust
            .title(if app.movie_mode {
                "Step 4: Confirm"
            } else {
                "Step 6: Confirm"
            }),
```

- [ ] **Step 3: Commit**

Suggest: `fix: update confirm screen for Vec<Episode>`

---

### Task 7: Add `Screen::EpisodeMapping` and update `SeasonEpisode` transition

**Files:**
- Modify: `src/tui/mod.rs` (Screen enum, originally lines 19-29)
- Modify: `src/tui/wizard.rs` (line 545 — SeasonEpisode Enter transition)

- [ ] **Step 1: Add `EpisodeMapping` variant to `Screen` enum**

In `src/tui/mod.rs`, find the `Screen` enum (line 19) and replace:

```rust
#[derive(Debug, Clone, PartialEq)]
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

- [ ] **Step 2: Update `SeasonEpisode` Enter to transition to `EpisodeMapping`**

In `src/tui/wizard.rs`, find `handle_season_episode_input`, in the `Enter` arm for `season_field == 1` (originally line 545), change:

```rust
                app.screen = Screen::EpisodeMapping;
```

- [ ] **Step 3: Commit**

Suggest: `feat: add Screen::EpisodeMapping variant and transition`

---

## Chunk 3: New EpisodeMapping TUI Screen

### Task 8: Add `EpisodeMapping` screen state to `App`

**Files:**
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Add `mapping_edit_row` field to `App`**

In `src/tui/mod.rs`, in the `App` struct, after `pub season_field: u8,` (originally line 64), add:

```rust
    /// Which row is being edited in EpisodeMapping screen (None = navigation mode)
    pub mapping_edit_row: Option<usize>,
```

- [ ] **Step 2: Initialize the new field in `App::new()`**

In `App::new()`, after `season_field: 0,` (originally line 116), add:

```rust
            mapping_edit_row: None,
```

- [ ] **Step 3: Reset the new field in `reset_for_rescan()`**

In `reset_for_rescan()`, after `self.season_field = 0;` (originally line 212), add:

```rust
        self.mapping_edit_row = None;
```

- [ ] **Step 4: Commit**

Suggest: `feat: add mapping_edit_row state to App`

---

### Task 9: Implement `render_episode_mapping()` and `handle_episode_mapping_input()`

**Files:**
- Modify: `src/tui/wizard.rs`

- [ ] **Step 1: Add import for `parse_episode_input`**

In `src/tui/wizard.rs`, find line 9 (the `use crate::util::...` line) and add `parse_episode_input`:

```rust
use crate::util::{assign_episodes, guess_start_episode, make_filename, make_movie_filename, parse_episode_input};
```

- [ ] **Step 2: Add `render_episode_mapping()` function**

Add this function after `handle_season_episode_input()` in `src/tui/wizard.rs`:

```rust
pub fn render_episode_mapping(f: &mut Frame, app: &App) {
    let chunks = standard_layout(f.area());

    let show_name = app
        .selected_show
        .and_then(|i| app.search_results.get(i))
        .map(|s| s.name.as_str())
        .unwrap_or("Unknown");

    let title = Paragraph::new(format!("Show: {}", show_name)).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Step 4: Episode Mapping"),
    );
    f.render_widget(title, chunks[0]);

    let header = Row::new(["", "#", "Playlist", "Duration", "Episode(s)"])
        .style(Style::default().fg(Color::Yellow));

    let rows: Vec<Row> = app
        .episodes_pl
        .iter()
        .enumerate()
        .map(|(i, pl)| {
            let cursor = if i == app.list_cursor { ">" } else { " " };

            let ep_str = if app.mapping_edit_row == Some(i) {
                format!("{}|", app.input_buffer)
            } else if let Some(eps) = app.episode_assignments.get(&pl.num) {
                if eps.is_empty() {
                    "(none)".to_string()
                } else {
                    eps.iter()
                        .map(|e| {
                            if e.name.is_empty() {
                                format!("E{:02}", e.episode_number)
                            } else {
                                format!("E{:02} - {}", e.episode_number, e.name)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            } else {
                "(none)".to_string()
            };

            let row_style = if app.mapping_edit_row == Some(i) {
                Style::default().fg(Color::Yellow)
            } else if i == app.list_cursor {
                Style::default().fg(Color::White)
            } else {
                Style::default()
            };

            Row::new(vec![
                cursor.to_string(),
                format!("{}", i + 1),
                pl.num.clone(),
                pl.duration.clone(),
                ep_str,
            ])
            .style(row_style)
        })
        .collect();

    let widths = [
        Constraint::Length(2),
        Constraint::Length(4),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Min(30),
    ];

    let table = Table::new(rows, &widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(table, chunks[1]);

    let hints = if app.mapping_edit_row.is_some() {
        "Enter: Confirm | Esc: Cancel | Format: 3 or 3-4 or 3,5"
    } else {
        "e: Edit | Enter: Accept | Up/Down: Navigate | Esc: Back | Ctrl+R: Rescan"
    };
    let hints = Paragraph::new(hints).style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[2]);
}
```

- [ ] **Step 3: Add `handle_episode_mapping_input()` function**

Add this function after `render_episode_mapping()`:

```rust
pub fn handle_episode_mapping_input(app: &mut App, key: KeyEvent) {
    if let Some(edit_row) = app.mapping_edit_row {
        // Inline edit mode
        match key.code {
            KeyCode::Char(c) => {
                if c.is_ascii_digit() || c == ',' || c == '-' {
                    app.input_buffer.push(c);
                }
            }
            KeyCode::Backspace => {
                app.input_buffer.pop();
            }
            KeyCode::Enter => {
                let pl_num = app.episodes_pl[edit_row].num.clone();
                match parse_episode_input(&app.input_buffer) {
                    Some(ep_nums) if ep_nums.is_empty() => {
                        app.episode_assignments.remove(&pl_num);
                    }
                    Some(ep_nums) => {
                        let ep_by_num: std::collections::HashMap<u32, &crate::types::Episode> =
                            app.episodes.iter().map(|e| (e.episode_number, e)).collect();
                        let eps: Vec<crate::types::Episode> = ep_nums
                            .iter()
                            .map(|&num| {
                                ep_by_num
                                    .get(&num)
                                    .map(|e| (*e).clone())
                                    .unwrap_or(crate::types::Episode {
                                        episode_number: num,
                                        name: String::new(),
                                        runtime: None,
                                    })
                            })
                            .collect();
                        app.episode_assignments.insert(pl_num, eps);
                    }
                    None => {
                        // Invalid input, stay in edit mode
                        return;
                    }
                }
                app.mapping_edit_row = None;
                app.input_buffer.clear();
                app.input_active = false;
            }
            KeyCode::Esc => {
                app.mapping_edit_row = None;
                app.input_buffer.clear();
                app.input_active = false;
            }
            _ => {}
        }
        return;
    }

    // Navigation mode
    match key.code {
        KeyCode::Up => {
            if app.list_cursor > 0 {
                app.list_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if app.list_cursor + 1 < app.episodes_pl.len() {
                app.list_cursor += 1;
            }
        }
        KeyCode::Char('e') => {
            let pl_num = &app.episodes_pl[app.list_cursor].num;
            let current = app
                .episode_assignments
                .get(pl_num)
                .map(|eps| {
                    eps.iter()
                        .map(|e| e.episode_number.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            app.input_buffer = current;
            app.mapping_edit_row = Some(app.list_cursor);
            app.input_active = true;
        }
        KeyCode::Enter => {
            app.list_cursor = 0;
            app.screen = Screen::PlaylistSelect;
        }
        KeyCode::Esc => {
            app.list_cursor = 0;
            app.input_active = true;
            let disc_num = app.label_info.as_ref().map(|l| l.disc);
            let guessed = app
                .start_episode
                .unwrap_or_else(|| guess_start_episode(disc_num, app.episodes_pl.len()));
            app.input_buffer = guessed.to_string();
            app.season_field = 1;
            app.screen = Screen::SeasonEpisode;
        }
        _ => {}
    }
}
```

- [ ] **Step 4: Commit**

Suggest: `feat: add EpisodeMapping TUI screen with inline editing`

---

### Task 10: Wire `EpisodeMapping` screen into the event loop

**Files:**
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Add render case for `EpisodeMapping`**

In `src/tui/mod.rs`, find the `terminal.draw` match (originally line 257) and add the `EpisodeMapping` arm after `SeasonEpisode`:

```rust
        terminal.draw(|f| match app.screen {
            Screen::Scanning => wizard::render_scanning(f, &app),
            Screen::TmdbSearch => wizard::render_tmdb_search(f, &app),
            Screen::ShowSelect => wizard::render_show_select(f, &app),
            Screen::SeasonEpisode => wizard::render_season_episode(f, &app),
            Screen::EpisodeMapping => wizard::render_episode_mapping(f, &app),
            Screen::PlaylistSelect => wizard::render_playlist_select(f, &app),
            Screen::Confirm => wizard::render_confirm(f, &app),
            Screen::Ripping => dashboard::render(f, &app),
            Screen::Done => dashboard::render_done(f, &app),
        })?;
```

- [ ] **Step 2: Add input handler case for `EpisodeMapping`**

In `src/tui/mod.rs`, find the screen match for input handling (originally lines 323-329, ending at the `Screen::Ripping` arm) and replace:

```rust
                match app.screen {
                    Screen::TmdbSearch => wizard::handle_tmdb_search_input(&mut app, key),
                    Screen::ShowSelect => wizard::handle_show_select_input(&mut app, key),
                    Screen::SeasonEpisode => wizard::handle_season_episode_input(&mut app, key),
                    Screen::EpisodeMapping => wizard::handle_episode_mapping_input(&mut app, key),
                    Screen::PlaylistSelect => wizard::handle_playlist_select_input(&mut app, key),
                    Screen::Confirm => wizard::handle_confirm_input(&mut app, key),
                    Screen::Ripping => dashboard::handle_input(&mut app, key),
```

- [ ] **Step 3: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1 | tail -5`
Expected: Compilation errors from `cli.rs` only (will be fixed in Task 11)

- [ ] **Step 4: Commit**

Suggest: `feat: wire EpisodeMapping screen into TUI event loop`

---

## Chunk 4: CLI Mode Updates

### Task 11: Update CLI mode for manual episode mapping

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Update `assign_episodes` call and add mapping prompt in `lookup_tmdb()`**

In `src/cli.rs`, find `lookup_tmdb()`. Locate the line `ctx.episode_assignments = assign_episodes(episodes_pl, &lookup.episodes, start_ep);` (originally line 152) and the two closing braces on lines 153-154. Replace those 3 lines with:

```rust
                ctx.episode_assignments = assign_episodes(episodes_pl, &lookup.episodes, start_ep);

                // Show mappings and prompt for accept/manual
                loop {
                    println!("\n  Episode Mappings:");
                    for pl in episodes_pl.iter() {
                        let ep_str = if let Some(eps) = ctx.episode_assignments.get(&pl.num) {
                            eps.iter()
                                .map(|e| {
                                    if e.name.is_empty() {
                                        format!("E{:02}", e.episode_number)
                                    } else {
                                        format!(
                                            "S{:02}E{:02} - {}",
                                            ctx.season_num.unwrap_or(0),
                                            e.episode_number,
                                            e.name
                                        )
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join(", ")
                        } else {
                            "(none)".to_string()
                        };
                        println!("    {} ({})  ->  {}", pl.num, pl.duration, ep_str);
                    }

                    let response = prompt("\n  Accept mappings? [Y/n/manual]: ")?;
                    if response.is_empty()
                        || response.eq_ignore_ascii_case("y")
                        || response.eq_ignore_ascii_case("yes")
                    {
                        break;
                    } else if response.eq_ignore_ascii_case("n") {
                        let new_start = prompt_number(
                            &format!("  Starting episode number [{}]: ", start_ep),
                            Some(start_ep),
                        )?;
                        ctx.episode_assignments =
                            assign_episodes(episodes_pl, &lookup.episodes, new_start);
                        continue;
                    } else if response.eq_ignore_ascii_case("manual") {
                        let ep_by_num: std::collections::HashMap<u32, &crate::types::Episode> =
                            lookup.episodes.iter().map(|e| (e.episode_number, e)).collect();
                        for pl in episodes_pl.iter() {
                            let current = ctx
                                .episode_assignments
                                .get(&pl.num)
                                .map(|eps| {
                                    eps.iter()
                                        .map(|e| e.episode_number.to_string())
                                        .collect::<Vec<_>>()
                                        .join(",")
                                })
                                .unwrap_or_default();
                            loop {
                                let input = prompt(&format!(
                                    "  Playlist {} ({}) [{}]: ",
                                    pl.num, pl.duration, current
                                ))?;
                                let input = if input.is_empty() {
                                    current.clone()
                                } else {
                                    input
                                };
                                match util::parse_episode_input(&input) {
                                    Some(ep_nums) if ep_nums.is_empty() => {
                                        ctx.episode_assignments.remove(&pl.num);
                                        break;
                                    }
                                    Some(ep_nums) => {
                                        let eps: Vec<crate::types::Episode> = ep_nums
                                            .iter()
                                            .map(|&num| {
                                                ep_by_num
                                                    .get(&num)
                                                    .map(|e| (*e).clone())
                                                    .unwrap_or(crate::types::Episode {
                                                        episode_number: num,
                                                        name: String::new(),
                                                        runtime: None,
                                                    })
                                            })
                                            .collect();
                                        ctx.episode_assignments.insert(pl.num.clone(), eps);
                                        break;
                                    }
                                    None => {
                                        println!("  Invalid input. Use: 3, 3-4, or 3,5");
                                    }
                                }
                            }
                        }
                        continue; // Loop back to show updated mappings
                    } else {
                        println!("  Invalid choice. Enter Y, n, or manual.");
                    }
                }
            }
        }
```

Note: The final `}` `}` close the `if let Some(lookup)` and `if let Some(ref key)` blocks that were on the original lines 153-154.

- [ ] **Step 2: Update `display_and_select()` to handle `Vec<Episode>`**

In `src/cli.rs`, find the `ep_str` block in `display_and_select` (originally lines 180-191) and replace:

```rust
        let ep_str = if let Some(eps) = episode_assignments.get(&pl.num) {
            if eps.len() == 1 {
                format!(
                    "  S{:02}E{:02} - {}",
                    season_num.unwrap_or(0),
                    eps[0].episode_number,
                    eps[0].name
                )
            } else if eps.len() > 1 {
                let first = &eps[0];
                let last = &eps[eps.len() - 1];
                format!(
                    "  S{:02}E{:02}-E{:02} - {}",
                    season_num.unwrap_or(0),
                    first.episode_number,
                    last.episode_number,
                    first.name
                )
            } else {
                String::new()
            }
        } else if has_eps {
            "  (no episode data)".into()
        } else {
            String::new()
        };
```

- [ ] **Step 3: Update `build_filenames()` to pass `&[Episode]` to `make_filename()`**

In `src/cli.rs`, find the `make_filename` call in `build_filenames` (originally lines 302-310, the `} else {` branch through the closing `}`) and replace:

```rust
            } else {
                let episodes = tmdb_ctx
                    .episode_assignments
                    .get(&pl.num)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                util::make_filename(
                    &pl.num,
                    episodes,
                    tmdb_ctx.season_num.unwrap_or(0),
                    fmt,
                    media_info.as_ref(),
                    Some(&extra_vars),
                )
            }
```

- [ ] **Step 4: Run `cargo build` to verify clean compilation**

Run: `cargo build 2>&1 | tail -5`
Expected: Successful build

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 6: Commit**

Suggest: `feat: add manual episode mapping to CLI mode`

---

## Chunk 5: Final Verification

### Task 12: Run full test suite and clippy

**Files:** None (verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1`
Expected: No errors (warnings acceptable)

- [ ] **Step 3: Fix any clippy issues**

Address any clippy warnings in modified code.

- [ ] **Step 4: Run release build**

Run: `cargo build --release 2>&1 | tail -3`
Expected: Successful build

- [ ] **Step 5: Commit any clippy fixes**

Suggest: `fix: address clippy warnings`
