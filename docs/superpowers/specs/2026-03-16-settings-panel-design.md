# Settings Panel Design

## Overview

Add a settings panel to the TUI as a popup overlay, accessible via `Ctrl+S` hotkey from any screen or as a standalone mode via `--settings`. Covers all `config.toml` settings with polished per-type controls (toggles, choice cycling, inline text edit). Changes are session-only by default with explicit save-to-config action.

## Config Location Resolution

New priority chain for determining which config file to load and save to:

1. `--config <PATH>` CLI flag (highest priority)
2. `BLUBACK_CONFIG` environment variable
3. `~/.config/bluback/config.toml` (default)

The resolved path is stored as `config_path: PathBuf` on `App` and used for both loading and saving.

## Overlay System

### Architecture

New field on `App`:

```rust
pub overlay: Option<Overlay>,
```

```rust
pub enum Overlay {
    Settings(SettingsState),
}
```

This pattern supports future overlays (help screen, keybinding reference) without structural changes.

### Input Routing

When `app.overlay.is_some()`, all input routes to the overlay handler first. Exceptions:

- `Ctrl+C` — always quits
- All other keys go to the overlay, not the underlying screen

`Ctrl+R` (rescan) is blocked while the overlay is open.

Note: `Ctrl+S` is handled by the global input handler in `tui/mod.rs` (same pattern as `Ctrl+R` and `Ctrl+E`). When the overlay is open, input routing sends `Ctrl+S` to the overlay handler where it triggers save. When the overlay is closed, the global handler opens the overlay. This means the same physical key does different things based on state — no collision.

### Rendering

The main render loop draws the current screen first, then conditionally draws the overlay on top. The overlay clears its background area before drawing content.

## Settings State

```rust
pub struct SettingsState {
    pub cursor: usize,
    pub items: Vec<SettingItem>,
    pub editing: Option<usize>,
    pub input_buffer: String,
    pub dirty: bool,
    pub save_message: Option<String>,
    pub save_message_at: Option<std::time::Instant>,
    pub confirm_close: Option<bool>, // None = no prompt, Some = showing prompt (true = save focused)
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
    Separator,
    Action {
        label: String,
    },
}
```

The `save_message_at` field tracks when the save message was shown. The main event loop checks this each tick and clears `save_message` after 2 seconds have elapsed, or on the next input event — whichever comes first.

## Settings List

| # | Setting | Type | Control |
|---|---------|------|---------|
| 1 | Preset | Choice | *(none)* / `default` / `plex` / `jellyfin` — cycles on Enter/Left/Right. *(none)* maps to `Option::None` in the data model (field omitted from config on save), not a literal `"none"` string |
| 2 | TV Format | Text | Custom template string — inline edit on Enter. Dimmed and non-editable when preset is not `none` |
| 3 | Movie Format | Text | Same behavior as TV Format |
| 4 | TMDb API Key | Text | Inline edit on Enter |
| 5 | Eject After Rip | Toggle | Flip on Enter/Space |
| 6 | Max Read Speed | Toggle | Flip on Enter/Space |
| — | *(separator)* | | |
| 7 | Save to Config (Ctrl+S) | Action | Writes to resolved config path |

## Popup Rendering

- Centered bordered box, width clamped to `min(50, terminal_width - 4)`, height fits content (~12 lines)
- `Block::bordered()` with title `" Settings "`
- When `dirty: true`, title becomes `" Settings (modified) "`
- Background area cleared before drawing content

### Item Display

| Type | Rendering |
|------|-----------|
| Toggle | `Eject After Rip          [ON]` — value in green (ON) or red (OFF) |
| Choice | `Preset               [jellyfin]` — shows current selection |
| Text | `TV Format    S{season}E{episode}_{ti...` — truncated to fit |
| Text (editing) | Value area becomes inline input with cursor |
| Action | `Save to Config (Ctrl+S)` — bold or accent color |

Highlighted row uses reversed colors (standard ratatui selection pattern).

## Keybindings

### Opening

- `Ctrl+S` on any screen including Scanning (except during text input on wizard screens, and except during rip confirmation prompts) opens the overlay. The underlying screen continues normally (e.g., disc scanning continues in its background thread).
- `--settings` CLI flag opens the overlay immediately on a minimal background

### Within the Overlay

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate settings list |
| `Enter` | Toggle (flip), Choice (cycle forward), Text (enter edit mode), Save (execute save) |
| `Space` | Toggle (flip) |
| `Left/Right` | Choice (cycle backward/forward) |
| `Esc` | Cancel text edit (if editing), otherwise close overlay |
| `Ctrl+S` | Save to config (works from anywhere in the list, including during text edit — confirms the edit first, then saves) |
| `Ctrl+C` | Quit app |

### Text Edit Mode

| Key | Action |
|-----|--------|
| Typing | Modify value |
| `Enter` | Confirm edit, apply to session |
| `Esc` | Cancel edit, restore previous value |
| `Backspace` | Delete character |

## Applying Changes

- Toggle and Choice changes apply to `app.config` immediately (session effect)
- Text edits apply on confirm (`Enter`), not on every keystroke
- Format changes (preset/tv_format/movie_format) take effect on the next rip cycle, not retroactively on already-computed filenames

## Format Conflict Handling

- When preset is set to anything other than `none`, TV Format and Movie Format text fields show the preset's template in dimmed/italic style and are non-editable
- Cycling preset to `none` re-enables the text fields
- This matches the existing format resolution priority chain

## Save to Config

- Writes to the resolved config path (from `--config` / `BLUBACK_CONFIG` / default)
- Creates file and parent directories if they don't exist
- Only writes fields that differ from defaults (keeps config minimal)
- Uses the `toml` crate to serialize. `Config` must derive `Serialize` (currently only derives `Deserialize`). All `Option<T>` fields use `#[serde(skip_serializing_if = "Option::is_none")]` so absent/default values are omitted from the file rather than written as empty strings.
- On success, shows "Saved!" next to the Save action item. Cleared after 2 seconds (tracked via `save_message_at: Option<Instant>`, checked each event loop tick) or on next input — whichever comes first.
- Sets `dirty: false` after save

## `--settings` Standalone Mode

- New clap flag: `--settings` (conflicts with `--dry-run`, `--no-tui`)
- `--settings` dispatch must happen in `main()` BEFORE device detection and dependency checking (`check_dependencies`, `detect_optical_drives`), since neither ffmpeg/ffprobe nor an optical drive is needed for editing config
- Enters TUI with overlay immediately open on a minimal/blank background
- Loads existing config values into `SettingsState`
- On close (`Esc`), if `dirty`, prompts inline at the bottom of the overlay: "Unsaved changes. Save before closing? (y/n/Esc)" where `y` saves then exits, `n` discards and exits, `Esc` cancels the close and returns to the settings panel
- After close, app exits (no wizard flow)

## `--config` CLI Flag

- New clap flag: `--config <PATH>` — path to config TOML file
- Also checks `BLUBACK_CONFIG` env var as fallback before the default path
- Affects both loading and saving
- The resolved path is stored as `config_path: PathBuf` on `App`

## Files Affected

- `src/main.rs` — Add `--settings` and `--config` flags, env var resolution, dispatch to settings-only mode
- `src/config.rs` — Accept custom path in `load()`, add `save()` method, env var lookup
- `src/types.rs` — Add `Overlay`, `SettingsState`, `SettingItem` types
- `src/tui/mod.rs` — Add `overlay` field to `App`, input routing logic, overlay render dispatch
- `src/tui/settings.rs` — New file: render and input handler for settings popup
- `src/tui/dashboard.rs` — No changes needed (`Ctrl+S` handled by global handler in mod.rs)

## Testing

- `config.rs` — Config path resolution priority (flag > env > default), save/load roundtrip, default-only serialization
- `types.rs` — `SettingItem` construction from `Config`, `SettingsState` initialization
- No hardware or TUI rendering tests needed
