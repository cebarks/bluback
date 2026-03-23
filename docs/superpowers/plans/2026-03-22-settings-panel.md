# Settings Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an in-TUI settings overlay accessible from any screen via `Ctrl+S`, with all config fields editable and a save action that writes commented-out defaults to `config.toml`.

**Architecture:** Overlay system on `App` that intercepts input when active. Settings state (`SettingsState`) holds a list of typed `SettingItem` variants. Config save generates TOML manually with commented defaults. Save triggers workflow reset (rescan) unless mid-rip. New `--settings` standalone mode and `--config` flag for config path override.

**Tech Stack:** Rust, ratatui, crossterm, toml, clap, serde

**Spec:** `docs/superpowers/specs/2026-03-22-settings-panel-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/config.rs` | Modify | New fields (`output_dir`, `device`), `config_path()` resolution, `save()` with commented defaults, default constants |
| `src/types.rs` | Modify | Add `Overlay`, `SettingsState`, `SettingItem` types |
| `src/tui/settings.rs` | Create | Render and input handler for settings overlay |
| `src/tui/mod.rs` | Modify | Add `overlay` field to `App`, input routing, overlay render dispatch, `Ctrl+S` global handler, `open_settings()`, `apply_settings()` |
| `src/tui/wizard.rs` | Modify | Add `Ctrl+S` to hint text on wizard screens |
| `src/tui/dashboard.rs` | Modify | Add `Ctrl+S` to hint text on ripping/done screens |
| `src/main.rs` | Modify | Add `--settings` and `--config` flags, config path resolution, `--settings` dispatch, apply `output_dir`/`device` from config |

---

### Task 1: Config — New Fields and Default Constants

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write tests for new config fields**

Add these tests in the existing `#[cfg(test)] mod tests` block in `src/config.rs`:

```rust
#[test]
fn test_parse_output_dir() {
    let config: Config = toml::from_str(r#"output_dir = "/tmp/rips""#).unwrap();
    assert_eq!(config.output_dir.as_deref(), Some("/tmp/rips"));
}

#[test]
fn test_parse_device() {
    let config: Config = toml::from_str(r#"device = "/dev/sr1""#).unwrap();
    assert_eq!(config.device.as_deref(), Some("/dev/sr1"));
}

#[test]
fn test_output_dir_default_absent() {
    let config: Config = toml::from_str("").unwrap();
    assert!(config.output_dir.is_none());
}

#[test]
fn test_device_default_absent() {
    let config: Config = toml::from_str("").unwrap();
    assert!(config.device.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_parse_output_dir test_parse_device test_output_dir_default test_device_default 2>&1`
Expected: FAIL — `output_dir` and `device` fields don't exist on `Config`

- [ ] **Step 3: Add new fields and default constants to Config**

In `src/config.rs`, add to the `Config` struct (after `show_filtered`):

```rust
pub output_dir: Option<String>,
pub device: Option<String>,
```

Add default constants at the top of the file (after the existing format constants):

```rust
pub const DEFAULT_OUTPUT_DIR: &str = ".";
pub const DEFAULT_DEVICE: &str = "auto-detect";
pub const DEFAULT_MIN_DURATION: u32 = 900;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -- test_parse_output_dir test_parse_device test_output_dir_default test_device_default 2>&1`
Expected: All 4 PASS

- [ ] **Step 5: Commit**

```
feat(config): add output_dir, device fields and default constants
```

---

### Task 2: Config — Save with Commented Defaults

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write tests for config save**

Add these tests in the existing `#[cfg(test)] mod tests` block in `src/config.rs`:

```rust
#[test]
fn test_save_default_config_all_commented() {
    let config = Config::default();
    let output = config.to_toml_string();
    // All lines should be comments (start with #) or blank
    for line in output.lines() {
        let trimmed = line.trim();
        assert!(
            trimmed.is_empty() || trimmed.starts_with('#'),
            "Expected comment or blank, got: {}",
            line
        );
    }
    // Should contain all known keys as comments
    assert!(output.contains("# eject = false"));
    assert!(output.contains("# max_speed = true"));
    assert!(output.contains("# min_duration = 900"));
    assert!(output.contains("# show_filtered = false"));
}

#[test]
fn test_save_modified_config_mixed() {
    let config = Config {
        eject: Some(true),
        min_duration: Some(600),
        ..Default::default()
    };
    let output = config.to_toml_string();
    // Modified values are active (no #)
    assert!(output.contains("\neject = true\n") || output.starts_with("eject = true\n"));
    assert!(output.contains("\nmin_duration = 600\n") || output.starts_with("min_duration = 600\n"));
    // Defaults are still commented
    assert!(output.contains("# max_speed = true"));
    assert!(output.contains("# show_filtered = false"));
}

#[test]
fn test_save_roundtrip() {
    let config = Config {
        eject: Some(true),
        preset: Some("plex".into()),
        min_duration: Some(600),
        output_dir: Some("/tmp/rips".into()),
        ..Default::default()
    };
    let toml_str = config.to_toml_string();
    // Parsing the active (non-commented) lines should produce the same values
    let reparsed: Config = toml::from_str(&toml_str).unwrap();
    assert_eq!(reparsed.eject, Some(true));
    assert_eq!(reparsed.preset.as_deref(), Some("plex"));
    assert_eq!(reparsed.min_duration, Some(600));
    assert_eq!(reparsed.output_dir.as_deref(), Some("/tmp/rips"));
    // Fields left at default should not be parsed (they're comments)
    assert!(reparsed.max_speed.is_none());
    assert!(reparsed.show_filtered.is_none());
}

#[test]
fn test_save_string_values_quoted() {
    let config = Config {
        tv_format: Some("custom/{show}.mkv".into()),
        tmdb_api_key: Some("abc123".into()),
        ..Default::default()
    };
    let output = config.to_toml_string();
    assert!(output.contains(r#"tv_format = "custom/{show}.mkv""#));
    assert!(output.contains(r#"tmdb_api_key = "abc123""#));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_save_default_config test_save_modified_config test_save_roundtrip test_save_string_values 2>&1`
Expected: FAIL — `to_toml_string` method doesn't exist

- [ ] **Step 3: Implement `to_toml_string()` and `save()`**

Add to `impl Config` in `src/config.rs`:

```rust
pub fn to_toml_string(&self) -> String {
    let mut out = String::new();

    fn emit_bool(out: &mut String, key: &str, val: Option<bool>, default: bool) {
        match val {
            Some(v) if v != default => out.push_str(&format!("{} = {}\n", key, v)),
            _ => out.push_str(&format!("# {} = {}\n", key, default)),
        }
    }

    fn emit_u32(out: &mut String, key: &str, val: Option<u32>, default: u32) {
        match val {
            Some(v) if v != default => out.push_str(&format!("{} = {}\n", key, v)),
            _ => out.push_str(&format!("# {} = {}\n", key, default)),
        }
    }

    fn emit_str(out: &mut String, key: &str, val: &Option<String>, default: &str) {
        match val {
            Some(ref v) if v != default => {
                out.push_str(&format!("{} = {:?}\n", key, v));
            }
            _ => {
                out.push_str(&format!("# {} = {:?}\n", key, default));
            }
        }
    }

    emit_str(&mut out, "output_dir", &self.output_dir, DEFAULT_OUTPUT_DIR);
    emit_str(&mut out, "device", &self.device, DEFAULT_DEVICE);
    emit_bool(&mut out, "eject", self.eject, false);
    emit_bool(&mut out, "max_speed", self.max_speed, true);
    emit_u32(&mut out, "min_duration", self.min_duration, DEFAULT_MIN_DURATION);
    out.push('\n');
    emit_str(&mut out, "preset", &self.preset, "");
    emit_str(&mut out, "tv_format", &self.tv_format, DEFAULT_TV_FORMAT);
    emit_str(&mut out, "movie_format", &self.movie_format, DEFAULT_MOVIE_FORMAT);
    emit_str(&mut out, "special_format", &self.special_format, DEFAULT_SPECIAL_FORMAT);
    emit_bool(&mut out, "show_filtered", self.show_filtered, false);
    out.push('\n');
    emit_str(&mut out, "tmdb_api_key", &self.tmdb_api_key, "");

    out
}

pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, self.to_toml_string())?;
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -- test_save_default_config test_save_modified_config test_save_roundtrip test_save_string_values 2>&1`
Expected: All 4 PASS

- [ ] **Step 5: Commit**

```
feat(config): add save with commented-out defaults
```

---

### Task 3: Config — Config Path Resolution

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write tests for config path resolution**

```rust
#[test]
fn test_resolve_config_path_default() {
    // With no flag and no env var, should return default path
    let path = resolve_config_path(None);
    assert!(path.to_string_lossy().ends_with(".config/bluback/config.toml"));
}

#[test]
fn test_resolve_config_path_explicit() {
    let path = resolve_config_path(Some(std::path::PathBuf::from("/tmp/custom.toml")));
    assert_eq!(path, std::path::PathBuf::from("/tmp/custom.toml"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_resolve_config_path 2>&1`
Expected: FAIL — `resolve_config_path` doesn't exist

- [ ] **Step 3: Implement `resolve_config_path()`**

Add as a public function in `src/config.rs`:

```rust
pub fn resolve_config_path(cli_path: Option<PathBuf>) -> PathBuf {
    if let Some(path) = cli_path {
        return path;
    }
    if let Ok(env_path) = std::env::var("BLUBACK_CONFIG") {
        return PathBuf::from(env_path);
    }
    config_dir().join("config.toml")
}

pub fn load_from(path: &std::path::Path) -> Config {
    if path.exists() {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        Config::default()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -- test_resolve_config_path 2>&1`
Expected: All PASS

- [ ] **Step 5: Commit**

```
feat(config): add config path resolution (--config flag, env var, default)
```

---

### Task 4: Types — Overlay, SettingsState, SettingItem

**Files:**
- Modify: `src/types.rs`

- [ ] **Step 1: Write test for SettingsState construction from Config**

Add to `#[cfg(test)] mod tests` in `src/types.rs`:

```rust
#[test]
fn test_settings_state_from_config_item_count() {
    let config = crate::config::Config::default();
    let state = SettingsState::from_config(&config);
    // 4 separators + 11 settings + 1 action = 16 items
    let non_separator_count = state.items.iter().filter(|i| !matches!(i, SettingItem::Separator { .. })).count();
    assert_eq!(non_separator_count, 12); // 11 settings + 1 action
}

#[test]
fn test_settings_state_from_config_values() {
    let config = crate::config::Config {
        eject: Some(true),
        min_duration: Some(600),
        ..Default::default()
    };
    let state = SettingsState::from_config(&config);
    // Find the eject toggle
    let eject = state.items.iter().find(|i| matches!(i, SettingItem::Toggle { key, .. } if key == "eject"));
    assert!(matches!(eject, Some(SettingItem::Toggle { value: true, .. })));
    // Find the min_duration number
    let min_dur = state.items.iter().find(|i| matches!(i, SettingItem::Number { key, .. } if key == "min_duration"));
    assert!(matches!(min_dur, Some(SettingItem::Number { value: 600, .. })));
}

#[test]
fn test_settings_state_to_config_roundtrip() {
    let config = crate::config::Config {
        eject: Some(true),
        preset: Some("plex".into()),
        min_duration: Some(600),
        output_dir: Some("/tmp/rips".into()),
        ..Default::default()
    };
    let state = SettingsState::from_config(&config);
    let restored = state.to_config();
    assert_eq!(restored.eject, Some(true));
    assert_eq!(restored.preset.as_deref(), Some("plex"));
    assert_eq!(restored.min_duration, Some(600));
    assert_eq!(restored.output_dir.as_deref(), Some("/tmp/rips"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_settings_state 2>&1`
Expected: FAIL — types don't exist

- [ ] **Step 3: Add Overlay, SettingsState, and SettingItem types**

Add to `src/types.rs` (before `#[cfg(test)]`):

```rust
pub enum Overlay {
    Settings(SettingsState),
}

pub struct SettingsState {
    pub cursor: usize,
    pub items: Vec<SettingItem>,
    pub editing: Option<usize>,
    pub input_buffer: String,
    pub cursor_pos: usize, // cursor position within input_buffer
    pub dirty: bool,
    pub save_message: Option<String>,
    pub save_message_at: Option<std::time::Instant>,
    pub confirm_close: Option<bool>,
    pub scroll_offset: usize,
    pub standalone: bool,
}

pub enum SettingItem {
    Toggle {
        label: String,
        key: String,
        value: bool,
    },
    Choice {
        label: String,
        key: String,
        options: Vec<String>,
        selected: usize,
    },
    Text {
        label: String,
        key: String,
        value: String,
    },
    Number {
        label: String,
        key: String,
        value: u32,
    },
    Separator {
        label: Option<String>,
    },
    Action {
        label: String,
    },
}
```

- [ ] **Step 4: Implement `SettingsState::from_config()` and `to_config()`**

Add `impl SettingsState` block in `src/types.rs`:

```rust
impl SettingsState {
    pub fn from_config(config: &crate::config::Config) -> Self {
        use crate::config::*;
        let items = vec![
            SettingItem::Separator { label: Some("General".into()) },
            SettingItem::Text {
                label: "Output Directory".into(),
                key: "output_dir".into(),
                value: config.output_dir.clone().unwrap_or_else(|| DEFAULT_OUTPUT_DIR.into()),
            },
            SettingItem::Text {
                label: "Device".into(),
                key: "device".into(),
                value: config.device.clone().unwrap_or_else(|| DEFAULT_DEVICE.into()),
            },
            SettingItem::Toggle {
                label: "Eject After Rip".into(),
                key: "eject".into(),
                value: config.eject.unwrap_or(false),
            },
            SettingItem::Toggle {
                label: "Max Read Speed".into(),
                key: "max_speed".into(),
                value: config.max_speed.unwrap_or(true),
            },
            SettingItem::Number {
                label: "Min Duration (secs)".into(),
                key: "min_duration".into(),
                value: config.min_duration.unwrap_or(DEFAULT_MIN_DURATION),
            },
            SettingItem::Separator { label: Some("Naming".into()) },
            SettingItem::Choice {
                label: "Preset".into(),
                key: "preset".into(),
                options: vec!["(none)".into(), "default".into(), "plex".into(), "jellyfin".into()],
                selected: match config.preset.as_deref() {
                    Some("default") => 1,
                    Some("plex") => 2,
                    Some("jellyfin") => 3,
                    _ => 0,
                },
            },
            SettingItem::Text {
                label: "TV Format".into(),
                key: "tv_format".into(),
                value: config.tv_format.clone().unwrap_or_else(|| DEFAULT_TV_FORMAT.into()),
            },
            SettingItem::Text {
                label: "Movie Format".into(),
                key: "movie_format".into(),
                value: config.movie_format.clone().unwrap_or_else(|| DEFAULT_MOVIE_FORMAT.into()),
            },
            SettingItem::Text {
                label: "Special Format".into(),
                key: "special_format".into(),
                value: config.special_format.clone().unwrap_or_else(|| DEFAULT_SPECIAL_FORMAT.into()),
            },
            SettingItem::Toggle {
                label: "Show Filtered".into(),
                key: "show_filtered".into(),
                value: config.show_filtered.unwrap_or(false),
            },
            SettingItem::Separator { label: Some("TMDb".into()) },
            SettingItem::Text {
                label: "API Key".into(),
                key: "tmdb_api_key".into(),
                value: config.tmdb_api_key.clone().unwrap_or_default(),
            },
            SettingItem::Separator { label: None },
            SettingItem::Action {
                label: "Save to Config (Ctrl+S)".into(),
            },
        ];

        // Find first non-separator item for initial cursor
        let cursor = items.iter().position(|i| !matches!(i, SettingItem::Separator { .. })).unwrap_or(0);

        SettingsState {
            cursor,
            items,
            editing: None,
            input_buffer: String::new(),
            cursor_pos: 0,
            dirty: false,
            save_message: None,
            save_message_at: None,
            confirm_close: None,
            scroll_offset: 0,
            standalone: false,
        }
    }

    pub fn to_config(&self) -> crate::config::Config {
        use crate::config::*;
        let mut config = crate::config::Config::default();

        for item in &self.items {
            match item {
                SettingItem::Text { key, value, .. } => match key.as_str() {
                    "output_dir" if value != DEFAULT_OUTPUT_DIR => config.output_dir = Some(value.clone()),
                    "device" if value != DEFAULT_DEVICE => config.device = Some(value.clone()),
                    "tv_format" if value != DEFAULT_TV_FORMAT => config.tv_format = Some(value.clone()),
                    "movie_format" if value != DEFAULT_MOVIE_FORMAT => config.movie_format = Some(value.clone()),
                    "special_format" if value != DEFAULT_SPECIAL_FORMAT => config.special_format = Some(value.clone()),
                    "tmdb_api_key" if !value.is_empty() => config.tmdb_api_key = Some(value.clone()),
                    _ => {}
                },
                SettingItem::Toggle { key, value, .. } => match key.as_str() {
                    "eject" if *value => config.eject = Some(true),
                    "max_speed" if !*value => config.max_speed = Some(false),
                    "show_filtered" if *value => config.show_filtered = Some(true),
                    _ => {}
                },
                SettingItem::Number { key, value, .. } => match key.as_str() {
                    "min_duration" if *value != DEFAULT_MIN_DURATION => config.min_duration = Some(*value),
                    _ => {}
                },
                SettingItem::Choice { key, options, selected, .. } => match key.as_str() {
                    "preset" => {
                        let val = &options[*selected];
                        if val != "(none)" {
                            config.preset = Some(val.clone());
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        config
    }

    /// Returns true if the item at the given index is a separator
    pub fn is_separator(&self, idx: usize) -> bool {
        matches!(self.items.get(idx), Some(SettingItem::Separator { .. }))
    }

    /// Move cursor to the next non-separator item
    pub fn move_cursor_down(&mut self) {
        let mut next = self.cursor + 1;
        while next < self.items.len() && self.is_separator(next) {
            next += 1;
        }
        if next < self.items.len() {
            self.cursor = next;
        }
    }

    /// Move cursor to the previous non-separator item
    pub fn move_cursor_up(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut prev = self.cursor - 1;
        while prev > 0 && self.is_separator(prev) {
            prev -= 1;
        }
        if !self.is_separator(prev) {
            self.cursor = prev;
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -- test_settings_state 2>&1`
Expected: All 3 PASS

- [ ] **Step 6: Commit**

```
feat(types): add Overlay, SettingsState, SettingItem types
```

---

### Task 5: Settings Overlay — Rendering

**Files:**
- Create: `src/tui/settings.rs`
- Modify: `src/tui/mod.rs` (add `pub mod settings;`)

- [ ] **Step 1: Create `src/tui/settings.rs` with render function**

```rust
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::types::{SettingItem, SettingsState};

pub fn render(f: &mut Frame, state: &SettingsState) {
    let area = f.area();

    // Calculate popup dimensions
    let popup_width = 60.min(area.width.saturating_sub(4));
    let content_height = state.items.len() as u16 + 2; // +2 for borders
    let popup_height = content_height.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(area.x + x, area.y + y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let title = if state.dirty {
        " Settings (modified) "
    } else {
        " Settings "
    };

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    // Calculate visible range for scrolling
    let visible_rows = inner.height as usize;
    let scroll_offset = if state.cursor >= state.scroll_offset + visible_rows {
        state.cursor - visible_rows + 1
    } else if state.cursor < state.scroll_offset {
        state.cursor
    } else {
        state.scroll_offset
    };

    let label_width = 22;

    for (i, item) in state.items.iter().enumerate().skip(scroll_offset).take(visible_rows) {
        let row_y = inner.y + (i - scroll_offset) as u16;
        if row_y >= inner.y + inner.height {
            break;
        }
        let row_area = Rect::new(inner.x, row_y, inner.width, 1);
        let is_selected = i == state.cursor;

        match item {
            SettingItem::Separator { label } => {
                if let Some(lbl) = label {
                    let span = Span::styled(
                        format!("  {}", lbl),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    );
                    f.render_widget(Paragraph::new(Line::from(span)), row_area);
                }
            }
            SettingItem::Toggle { label, value, .. } => {
                let val_str = if *value { "[ON]" } else { "[OFF]" };
                let val_color = if *value { Color::Green } else { Color::Red };
                let line = Line::from(vec![
                    Span::raw(format!("  {:width$}", label, width = label_width)),
                    Span::styled(val_str, Style::default().fg(val_color)),
                ]);
                let style = if is_selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                f.render_widget(Paragraph::new(line).style(style), row_area);
            }
            SettingItem::Choice { label, options, selected, .. } => {
                let val_str = format!("[{}]", options[*selected]);
                let line = Line::from(vec![
                    Span::raw(format!("  {:width$}", label, width = label_width)),
                    Span::styled(val_str, Style::default().fg(Color::Cyan)),
                ]);
                let style = if is_selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                f.render_widget(Paragraph::new(line).style(style), row_area);
            }
            SettingItem::Text { label, key, value, .. } => {
                let is_editing = state.editing == Some(i);
                // Check if dimmed (tv_format/movie_format when preset is set)
                let is_dimmed = (key == "tv_format" || key == "movie_format") && is_preset_active(state);
                let display_val = if is_editing {
                    render_edit_buffer(&state.input_buffer, state.cursor_pos, inner.width as usize - label_width - 4)
                } else if key == "tmdb_api_key" && !value.is_empty() {
                    mask_api_key(value)
                } else {
                    truncate(value, inner.width as usize - label_width - 4)
                };
                let val_style = if is_editing {
                    Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)
                } else if is_dimmed {
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
                } else {
                    Style::default()
                };
                let line = Line::from(vec![
                    Span::raw(format!("  {:width$}", label, width = label_width)),
                    Span::styled(display_val, val_style),
                ]);
                let style = if is_selected && !is_editing {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                f.render_widget(Paragraph::new(line).style(style), row_area);
            }
            SettingItem::Number { label, value, .. } => {
                let is_editing = state.editing == Some(i);
                let display_val = if is_editing {
                    render_edit_buffer(&state.input_buffer, state.cursor_pos, inner.width as usize - label_width - 4)
                } else {
                    value.to_string()
                };
                let val_style = if is_editing {
                    Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)
                } else {
                    Style::default()
                };
                let line = Line::from(vec![
                    Span::raw(format!("  {:width$}", label, width = label_width)),
                    Span::styled(display_val, val_style),
                ]);
                let style = if is_selected && !is_editing {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                f.render_widget(Paragraph::new(line).style(style), row_area);
            }
            SettingItem::Action { label, .. } => {
                let mut spans = vec![Span::styled(
                    format!("  {}", label),
                    Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
                )];
                if let Some(ref msg) = state.save_message {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(msg, Style::default().fg(Color::Green)));
                }
                let style = if is_selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                f.render_widget(Paragraph::new(Line::from(spans)).style(style), row_area);
            }
        }
    }

    // Hint bar at bottom of popup (if room)
    if popup_height > 3 {
        let hint_y = popup_area.y + popup_height - 1;
        if hint_y < area.y + area.height {
            let hint_area = Rect::new(popup_area.x + 1, hint_y, popup_width - 2, 1);
            let hint = if state.editing.is_some() {
                "Enter: Confirm  Esc: Cancel"
            } else if state.confirm_close.is_some() {
                "Save before closing? [y] Yes  [n] No  [Esc] Cancel"
            } else {
                "Ctrl+S: Save  Esc: Close  Enter/Space: Edit"
            };
            f.render_widget(
                Paragraph::new(hint)
                    .style(Style::default().fg(Color::DarkGray))
                    .alignment(Alignment::Center),
                hint_area,
            );
        }
    }
}

fn is_preset_active(state: &SettingsState) -> bool {
    state.items.iter().any(|item| {
        matches!(item, SettingItem::Choice { key, selected, .. } if key == "preset" && *selected != 0)
    })
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len > 3 {
        format!("{}...", &s[..max_len - 3])
    } else {
        s[..max_len].to_string()
    }
}

fn mask_api_key(key: &str) -> String {
    if key.len() <= 4 {
        "*".repeat(key.len())
    } else {
        format!("{}...{}", "*".repeat(key.len() - 4), &key[key.len() - 4..])
    }
}

fn render_edit_buffer(buf: &str, cursor_pos: usize, max_len: usize) -> String {
    if buf.len() <= max_len {
        buf.to_string()
    } else if cursor_pos >= max_len {
        // Show the area around cursor
        let start = cursor_pos.saturating_sub(max_len) + 1;
        buf[start..].to_string()
    } else {
        buf[..max_len].to_string()
    }
}
```

- [ ] **Step 2: Add `pub mod settings;` to `src/tui/mod.rs`**

Add after the existing module declarations at the top:

```rust
pub mod settings;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1`
Expected: Compiles successfully (may have unused warnings, that's fine)

- [ ] **Step 4: Commit**

```
feat(tui): add settings overlay rendering
```

---

### Task 6: Settings Overlay — Input Handling

**Files:**
- Modify: `src/tui/settings.rs`

- [ ] **Step 1: Add input handler to `src/tui/settings.rs`**

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Returns true if the overlay should be closed
pub fn handle_input(state: &mut SettingsState, key: KeyEvent) -> SettingsAction {
    // Handle confirm_close prompt (--settings standalone mode)
    if let Some(_focused) = state.confirm_close {
        return match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                state.confirm_close = None;
                SettingsAction::SaveAndClose
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                state.confirm_close = None;
                SettingsAction::Close
            }
            KeyCode::Esc => {
                state.confirm_close = None;
                SettingsAction::None
            }
            _ => SettingsAction::None,
        };
    }

    // Ctrl+S always saves (even during editing)
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        // Confirm current edit first if editing
        if let Some(idx) = state.editing {
            confirm_edit(state, idx);
        }
        return SettingsAction::Save;
    }

    // Handle editing mode
    if let Some(idx) = state.editing {
        return handle_edit_input(state, key, idx);
    }

    // Navigation and actions
    match key.code {
        KeyCode::Up => {
            state.move_cursor_up();
            SettingsAction::None
        }
        KeyCode::Down => {
            state.move_cursor_down();
            SettingsAction::None
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            handle_activate(state)
        }
        KeyCode::Left => {
            handle_cycle(state, false);
            SettingsAction::None
        }
        KeyCode::Right => {
            handle_cycle(state, true);
            SettingsAction::None
        }
        KeyCode::Esc => {
            if state.standalone && state.dirty {
                state.confirm_close = Some(true);
                SettingsAction::None
            } else {
                SettingsAction::Close
            }
        }
        _ => SettingsAction::None,
    }
}

pub enum SettingsAction {
    None,
    Close,
    Save,
    SaveAndClose,
}

fn handle_activate(state: &mut SettingsState) -> SettingsAction {
    let idx = state.cursor;
    match &state.items[idx] {
        SettingItem::Toggle { .. } => {
            if let SettingItem::Toggle { value, .. } = &mut state.items[idx] {
                *value = !*value;
                state.dirty = true;
            }
            SettingsAction::None
        }
        SettingItem::Choice { .. } => {
            handle_cycle(state, true);
            SettingsAction::None
        }
        SettingItem::Text { key, value, .. } => {
            // Don't allow editing tv_format/movie_format when preset is active
            if (key == "tv_format" || key == "movie_format") && is_preset_active(state) {
                return SettingsAction::None;
            }
            state.input_buffer = value.clone();
            state.cursor_pos = state.input_buffer.len();
            state.editing = Some(idx);
            SettingsAction::None
        }
        SettingItem::Number { value, .. } => {
            state.input_buffer = value.to_string();
            state.cursor_pos = state.input_buffer.len();
            state.editing = Some(idx);
            SettingsAction::None
        }
        SettingItem::Action { .. } => SettingsAction::Save,
        SettingItem::Separator { .. } => SettingsAction::None,
    }
}

fn handle_cycle(state: &mut SettingsState, forward: bool) {
    let idx = state.cursor;
    if let SettingItem::Choice { options, selected, .. } = &mut state.items[idx] {
        let len = options.len();
        *selected = if forward {
            (*selected + 1) % len
        } else {
            (*selected + len - 1) % len
        };
        state.dirty = true;
    }
}

fn handle_edit_input(state: &mut SettingsState, key: KeyEvent, idx: usize) -> SettingsAction {
    let is_number = matches!(state.items[idx], SettingItem::Number { .. });

    match key.code {
        KeyCode::Enter => {
            confirm_edit(state, idx);
            SettingsAction::None
        }
        KeyCode::Esc => {
            state.editing = None;
            state.input_buffer.clear();
            state.cursor_pos = 0;
            SettingsAction::None
        }
        KeyCode::Backspace => {
            if state.cursor_pos > 0 {
                state.input_buffer.remove(state.cursor_pos - 1);
                state.cursor_pos -= 1;
            }
            SettingsAction::None
        }
        KeyCode::Delete => {
            if state.cursor_pos < state.input_buffer.len() {
                state.input_buffer.remove(state.cursor_pos);
            }
            SettingsAction::None
        }
        KeyCode::Left => {
            state.cursor_pos = state.cursor_pos.saturating_sub(1);
            SettingsAction::None
        }
        KeyCode::Right => {
            if state.cursor_pos < state.input_buffer.len() {
                state.cursor_pos += 1;
            }
            SettingsAction::None
        }
        KeyCode::Home => {
            state.cursor_pos = 0;
            SettingsAction::None
        }
        KeyCode::End => {
            state.cursor_pos = state.input_buffer.len();
            SettingsAction::None
        }
        KeyCode::Char(c) => {
            // Number fields reject non-digit characters
            if is_number && !c.is_ascii_digit() {
                return SettingsAction::None;
            }
            state.input_buffer.insert(state.cursor_pos, c);
            state.cursor_pos += 1;
            SettingsAction::None
        }
        _ => SettingsAction::None,
    }
}

fn confirm_edit(state: &mut SettingsState, idx: usize) {
    let new_value = state.input_buffer.clone();
    match &mut state.items[idx] {
        SettingItem::Text { value, .. } => {
            *value = new_value;
            state.dirty = true;
        }
        SettingItem::Number { value, .. } => {
            if let Ok(n) = new_value.parse::<u32>() {
                if n > 0 {
                    *value = n;
                    state.dirty = true;
                }
            }
            // Invalid/zero: silently cancel, previous value restored
        }
        _ => {}
    }
    state.editing = None;
    state.input_buffer.clear();
    state.cursor_pos = 0;
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```
feat(tui): add settings overlay input handling
```

---

### Task 7: TUI — Overlay Integration (App, Input Routing, Render Dispatch)

**Files:**
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Add overlay field to App and open_settings helper**

In `src/tui/mod.rs`, add to the `App` struct (after `disc_detected_label`):

```rust
pub overlay: Option<crate::types::Overlay>,
pub config_path: std::path::PathBuf,
```

In `App::new()`, initialize:

```rust
overlay: None,
config_path: std::path::PathBuf::new(),
```

Add helper method on `impl App`:

```rust
pub fn open_settings(&mut self) {
    let state = crate::types::SettingsState::from_config(&self.config);
    self.overlay = Some(crate::types::Overlay::Settings(state));
}
```

- [ ] **Step 2: Add overlay rendering in the draw call**

In `run_app()`, after the existing `terminal.draw(...)` block, add the overlay render inside the draw closure (move the overlay render INTO the draw closure, after the screen match):

Replace the draw block with:

```rust
terminal.draw(|f| {
    match app.screen {
        Screen::Scanning => wizard::render_scanning(f, &app),
        Screen::TmdbSearch => wizard::render_tmdb_search(f, &app),
        Screen::Season => wizard::render_season(f, &app),
        Screen::PlaylistManager => wizard::render_playlist_manager(f, &app),
        Screen::Confirm => wizard::render_confirm(f, &app),
        Screen::Ripping => dashboard::render(f, &app),
        Screen::Done => dashboard::render_done(f, &app),
    }
    // Render overlay on top
    if let Some(crate::types::Overlay::Settings(ref state)) = app.overlay {
        settings::render(f, state);
    }
})?;
```

- [ ] **Step 3: Reorder global input handlers and add overlay routing**

In `run_app()`, restructure the key event handling. The current code checks `q` before `Ctrl+C`. Reorder so `Ctrl+C` is first, then overlay routing (which blocks all other globals), then existing globals:

```rust
if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
    app.quit = true;
    continue;
}

// Route ALL input to overlay when active (blocks q, Ctrl+E, Ctrl+R, Ctrl+S)
if app.overlay.is_some() {
    let action = {
        let state = match app.overlay {
            Some(crate::types::Overlay::Settings(ref mut s)) => s,
            _ => unreachable!(),
        };
        if state.save_message.is_some() {
            state.save_message = None;
            state.save_message_at = None;
        }
        settings::handle_input(state, key)
    };
    match action {
        settings::SettingsAction::Save => {
            handle_settings_save(&mut app);
        }
        settings::SettingsAction::SaveAndClose => {
            handle_settings_save(&mut app);
            app.overlay = None;
            // In standalone mode, quit after save+close
            // (standalone is handled in run_settings, not here)
        }
        settings::SettingsAction::Close => {
            app.overlay = None;
        }
        settings::SettingsAction::None => {
            // Apply toggle/choice changes to session immediately
            apply_settings_to_session(&mut app);
        }
    }
    continue;
}

// Global quit (not during ripping -- dashboard handles its own q)
if key.code == KeyCode::Char('q')
    && !input_active
    && app.screen != Screen::Ripping
{
    app.quit = true;
    continue;
}

// ... existing Ctrl+E, Ctrl+R handlers unchanged ...
```

Add a helper function in `src/tui/mod.rs` (outside `run_app`):

```rust
fn handle_settings_save(app: &mut App) {
    let new_config = match app.overlay {
        Some(crate::types::Overlay::Settings(ref state)) => state.to_config(),
        _ => return,
    };
    match new_config.save(&app.config_path) {
        Ok(()) => {
            app.config = new_config;
            app.eject = app.config.should_eject(app.args.cli_eject());
            // Update args for device/output_dir
            if let Some(ref dir) = app.config.output_dir {
                app.args.output = std::path::PathBuf::from(dir);
            }
            if let Some(ref dev) = app.config.device {
                if dev != crate::config::DEFAULT_DEVICE {
                    app.args.device = Some(std::path::PathBuf::from(dev));
                }
            }
            if let Some(crate::types::Overlay::Settings(ref mut state)) = app.overlay {
                state.save_message = Some("Saved!".into());
                state.save_message_at = Some(std::time::Instant::now());
                state.dirty = false;
            }
            // Reset workflow if not ripping
            if app.screen != Screen::Ripping && app.screen != Screen::Done {
                app.reset_for_rescan();
                app.tmdb.api_key = crate::tmdb::get_api_key(&app.config);
                start_disc_scan(app);
            }
        }
        Err(e) => {
            if let Some(crate::types::Overlay::Settings(ref mut state)) = app.overlay {
                state.save_message = Some(format!("Error: {}", e));
                state.save_message_at = Some(std::time::Instant::now());
            }
        }
    }
}
```

Also add a function that applies toggle/choice changes to the session without saving to disk:

```rust
fn apply_settings_to_session(app: &mut App) {
    let new_config = match app.overlay {
        Some(crate::types::Overlay::Settings(ref state)) => state.to_config(),
        _ => return,
    };
    app.config = new_config;
    app.eject = app.config.should_eject(app.args.cli_eject());
}
```

**Important:** The `q` quit handler must be moved AFTER the overlay routing block (it was previously before `Ctrl+C`). The reordered flow is:
1. `Ctrl+C` → quit (always)
2. Overlay routing → if overlay active, handle and `continue` (blocks everything else)
3. `q` → quit (existing, unchanged)
4. `Ctrl+E` → eject (existing, unchanged)
5. `Ctrl+R` → rescan (existing, unchanged)
6. `Ctrl+S` → open settings (new, see Step 4)
7. Per-screen dispatch

- [ ] **Step 4: Add `Ctrl+S` to global handlers**

After the `Ctrl+R` handler block, add:

```rust
// Global Ctrl+S: open settings (not during text input or rip confirmations)
if key.code == KeyCode::Char('s')
    && key.modifiers.contains(KeyModifiers::CONTROL)
    && !input_active
    && !app.rip.confirm_abort
    && !app.rip.confirm_rescan
{
    app.open_settings();
    continue;
}
```

- [ ] **Step 5: Add save message timeout check**

In the event loop, after the spinner frame increment, add:

```rust
// Clear save message after 2 seconds
if let Some(crate::types::Overlay::Settings(ref mut state)) = app.overlay {
    if let Some(at) = state.save_message_at {
        if at.elapsed() > Duration::from_secs(2) {
            state.save_message = None;
            state.save_message_at = None;
        }
    }
}
```

- [ ] **Step 6: Pass config_path into App in run_app**

Update `run_app` to accept and set `config_path`. In `run()` and `run_app()`:

```rust
pub fn run(args: &Args, config: &crate::config::Config, config_path: std::path::PathBuf) -> Result<()> {
    // ... existing setup ...
    let result = run_app(&mut terminal, args, config, config_path);
    // ...
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    args: &Args,
    config: &crate::config::Config,
    config_path: std::path::PathBuf,
) -> Result<()> {
    let mut app = App::new(args.clone());
    app.config = config.clone();
    app.config_path = config_path;
    // ... rest unchanged
}
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo build 2>&1`
Expected: Compiles (fix any issues)

- [ ] **Step 8: Commit**

```
feat(tui): integrate settings overlay with input routing and rendering
```

---

### Task 8: Main — CLI Flags, Config Path, --settings Mode

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add --settings and --config flags to Args**

```rust
/// Open settings panel without starting a rip
#[arg(long, conflicts_with_all = ["dry_run", "no_tui"])]
settings: bool,

/// Path to config file
#[arg(long)]
config: Option<PathBuf>,
```

- [ ] **Step 2: Update main() to use config path resolution and handle --settings**

```rust
fn main() -> anyhow::Result<()> {
    let mut args = Args::parse();

    let config_path = config::resolve_config_path(args.config.clone());
    let config = config::load_from(&config_path);

    // --settings mode: open settings panel without disc/dependency checks
    if args.settings {
        if !atty_stdout() {
            anyhow::bail!("--settings requires a terminal (stdout is not a TTY)");
        }
        return tui::run_settings(&config, config_path);
    }

    // Apply config defaults to args
    if args.device.is_none() {
        if let Some(ref dev) = config.device {
            if dev != "auto-detect" {
                args.device = Some(PathBuf::from(dev));
            }
        }
    }
    if args.output == PathBuf::from(".") {
        if let Some(ref dir) = config.output_dir {
            args.output = PathBuf::from(dir);
        }
    }

    if args.device.is_none() {
        let drives = disc::detect_optical_drives();
        args.device = Some(drives[0].clone());
    }

    disc::check_dependencies()?;

    let use_tui = !args.no_tui && atty_stdout();

    if use_tui {
        tui::run(&args, &config, config_path)
    } else {
        cli::run(&args, &config)
    }
}
```

- [ ] **Step 3: Update `load_config` call sites**

Replace the existing `config::load_config()` call with `config::load_from(&config_path)`. Keep the existing `load_config()` function for backwards compatibility or remove it if no longer used.

- [ ] **Step 4: Add `run_settings()` to `src/tui/mod.rs`**

```rust
pub fn run_settings(config: &crate::config::Config, config_path: std::path::PathBuf) -> Result<()> {
    enable_raw_mode()?;
    io::stdout().execute(SetTitle("bluback — Settings"))?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(Args::parse_from(["bluback"]));
    app.config = config.clone();
    app.config_path = config_path;
    let mut state = crate::types::SettingsState::from_config(config);
    state.standalone = true;
    app.overlay = Some(crate::types::Overlay::Settings(state));

    // Simple event loop — just render settings overlay on blank background
    loop {
        terminal.draw(|f| {
            // Blank background
            let block = ratatui::widgets::Block::default()
                .title("bluback")
                .borders(ratatui::widgets::Borders::ALL);
            f.render_widget(block, f.area());
            if let Some(crate::types::Overlay::Settings(ref state)) = app.overlay {
                settings::render(f, state);
            }
        })?;

        if app.quit || app.overlay.is_none() {
            break;
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                if let Some(crate::types::Overlay::Settings(ref mut state)) = app.overlay {
                    if state.save_message.is_some() {
                        state.save_message = None;
                        state.save_message_at = None;
                    }
                    match settings::handle_input(state, key) {
                        settings::SettingsAction::Save => {
                            let new_config = state.to_config();
                            match new_config.save(&app.config_path) {
                                Ok(()) => {
                                    state.save_message = Some("Saved!".into());
                                    state.save_message_at = Some(std::time::Instant::now());
                                    state.dirty = false;
                                }
                                Err(e) => {
                                    state.save_message = Some(format!("Error: {}", e));
                                    state.save_message_at = Some(std::time::Instant::now());
                                }
                            }
                        }
                        settings::SettingsAction::SaveAndClose => {
                            let new_config = state.to_config();
                            let _ = new_config.save(&app.config_path);
                            app.overlay = None;
                        }
                        settings::SettingsAction::Close => {
                            app.overlay = None;
                        }
                        settings::SettingsAction::None => {}
                    }
                }
            }
        }

        // Clear save message after 2 seconds
        if let Some(crate::types::Overlay::Settings(ref mut state)) = app.overlay {
            if let Some(at) = state.save_message_at {
                if at.elapsed() > Duration::from_secs(2) {
                    state.save_message = None;
                    state.save_message_at = None;
                }
            }
        }
    }

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    io::stdout().execute(SetTitle(""))?;
    Ok(())
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build 2>&1`
Expected: Compiles successfully

- [ ] **Step 6: Commit**

```
feat: add --settings standalone mode and --config flag
```

---

### Task 9: Hint Text Updates

**Files:**
- Modify: `src/tui/wizard.rs`
- Modify: `src/tui/dashboard.rs`

- [ ] **Step 1: Add Ctrl+S to wizard hint text**

In `src/tui/wizard.rs`, update the hint strings that mention keybindings to include `Ctrl+S: Settings`. For example:

- Line 183: `"q: Quit | Ctrl+E: Eject | Ctrl+R: Rescan"` → `"q: Quit | Ctrl+S: Settings | Ctrl+E: Eject | Ctrl+R: Rescan"`
- Line 576: Add `Ctrl+S: Settings` to season screen hints
- Lines 1190/1192: Add `Ctrl+S: Settings` to confirm screen hints

- [ ] **Step 2: Add Ctrl+S to dashboard hint text**

In `src/tui/dashboard.rs`:

- Line 141: `"[q] Abort  [Ctrl+R] Rescan"` → `"[q] Abort  [Ctrl+S] Settings  [Ctrl+R] Rescan"`
- Line 231: `"[Enter/Ctrl+R] Rescan  [Ctrl+E] Eject  [any other key] Exit"` → `"[Enter/Ctrl+R] Rescan  [Ctrl+S] Settings  [Ctrl+E] Eject  [any other key] Exit"`

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```
feat(tui): add Ctrl+S settings hint to all screens
```

---

### Task 10: Integration Testing and Polish

**Files:**
- All modified files

- [ ] **Step 1: Run all tests**

Run: `cargo test 2>&1`
Expected: All tests pass (existing 136 + new config/types tests)

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1`
Expected: No errors (warnings acceptable if pre-existing)

- [ ] **Step 3: Manual testing checklist**

If a disc/drive is available, test:
- [ ] `Ctrl+S` opens settings from Scanning screen
- [ ] `Ctrl+S` opens settings from TMDb Search screen
- [ ] Up/Down navigates, skips separators
- [ ] Enter toggles bools, cycles choices
- [ ] Enter on text field enters edit mode, Esc cancels, Enter confirms
- [ ] Number field rejects non-digit input
- [ ] TV/Movie format fields are dimmed when preset is set
- [ ] `Ctrl+S` in overlay saves to `~/.config/bluback/config.toml`
- [ ] Saved file has commented-out defaults and active modified values
- [ ] Save triggers rescan (when not ripping)
- [ ] `Esc` closes overlay
- [ ] `bluback --settings` opens settings-only mode
- [ ] `bluback --config /tmp/test.toml --settings` uses custom path

- [ ] **Step 4: Commit any fixes from testing**

```
fix(settings): polish from integration testing
```

- [ ] **Step 5: Update TODO.md**

Remove the "settings overhaul" item from `TODO.md`.

- [ ] **Step 6: Final commit**

```
docs: remove settings overhaul from TODO
```
