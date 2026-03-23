# Settings Panel Design (v2)

Supersedes `2026-03-16-settings-panel-design.md`.

## Overview

Add a settings panel to the TUI as a popup overlay, accessible via `Ctrl+S` hotkey from any screen or as a standalone mode via `--settings`. Covers all `config.toml` settings plus new settings (`output_dir`, `device`) with per-type controls (toggles, choice cycling, inline text edit). Changes apply to the current session immediately; an explicit save action writes to `config.toml` with commented-out defaults and triggers a workflow reset (rescan) unless mid-rip.

## Config Location Resolution

Priority chain for determining which config file to load and save to:

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

When `app.overlay.is_some()`, **all global key handlers except `Ctrl+C` are skipped**. This means `q` (quit), `Ctrl+E` (eject), `Ctrl+R` (rescan), and `Ctrl+S` (open settings) do NOT fire — all input routes to the overlay handler instead.

Concretely, the event loop structure becomes:

1. `Ctrl+C` — always quits (unconditional)
2. If `app.overlay.is_some()` → route to overlay handler, `continue`
3. All existing global handlers (`q`, `Ctrl+E`, `Ctrl+R`, `Ctrl+S`) — guarded by `app.overlay.is_none()`
4. Per-screen dispatch

The global `Ctrl+S` handler (step 3) opens the overlay. Within the overlay handler (step 2), `Ctrl+S` triggers save.

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
    pub confirm_close: Option<bool>,
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
    Separator,
    Action {
        label: String,
    },
}
```

The `save_message_at` field tracks when the save message was shown. The main event loop clears `save_message` after 2 seconds or on the next input event.

## Settings List

| # | Setting | Key | Type | Control | Default |
|---|---------|-----|------|---------|---------|
| | **General** | | Separator | | |
| 1 | Output Directory | `output_dir` | Text | Inline edit | `.` |
| 2 | Device | `device` | Text | Inline edit | `auto-detect` |
| 3 | Eject After Rip | `eject` | Toggle | Enter/Space | off |
| 4 | Max Read Speed | `max_speed` | Toggle | Enter/Space | on |
| 5 | Min Duration (secs) | `min_duration` | Number | Inline edit | `900` |
| | **Naming** | | Separator | | |
| 6 | Preset | `preset` | Choice | *(none)* / `default` / `plex` / `jellyfin` | *(none)* |
| 7 | TV Format | `tv_format` | Text | Inline edit (dimmed when preset set) | `S{season}E{episode}_{title}.mkv` |
| 8 | Movie Format | `movie_format` | Text | Inline edit (dimmed when preset set) | `{title}_({year}).mkv` |
| 9 | Special Format | `special_format` | Text | Inline edit | `{show} S00E{episode} {title}.mkv` |
| 10 | Show Filtered | `show_filtered` | Toggle | Enter/Space | off |
| | **TMDb** | | Separator | | |
| 11 | API Key | `tmdb_api_key` | Text | Inline edit (masked display) | *(empty)* |
| | | | Separator | | |
| 12 | Save to Config (Ctrl+S) | | Action | Execute save | |

## Popup Rendering

- Centered bordered box, width clamped to `min(60, terminal_width - 4)`, height clamped to `min(content_height + 2, terminal_height - 2)`
- If the terminal height is less than the content height plus borders, the settings list scrolls vertically, keeping the cursor row visible
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
| Number | `Min Duration (secs)       900` — inline edit on Enter |
| Action | `Save to Config (Ctrl+S)` — bold or accent color |
| Separator | Blank line with optional category label |

Highlighted row uses reversed colors (standard ratatui selection pattern).

## Keybindings

### Opening

- `Ctrl+S` on any screen (except during text input on wizard screens, and except during rip abort/rescan confirmation prompts — `confirm_abort` / `confirm_rescan`) opens the overlay. The underlying screen continues normally (e.g., disc scanning continues in its background thread).
- `--settings` CLI flag opens the overlay immediately on a minimal background.

### Within the Overlay

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate settings list (skips separators) |
| `Enter` | Toggle (flip), Choice (cycle forward), Text/Number (enter edit mode), Save (execute save) |
| `Space` | Toggle (flip) |
| `Left/Right` | Choice (cycle backward/forward) |
| `Esc` | Cancel text edit (if editing), otherwise close overlay |
| `Ctrl+S` | Save to config (works from anywhere in the list, including during text edit — confirms the edit first, then saves) |
| `Ctrl+C` | Quit app |

### Text/Number Edit Mode

| Key | Action |
|-----|--------|
| Typing | Modify value |
| `Enter` | Confirm edit, apply to session |
| `Esc` | Cancel edit, restore previous value |
| `Backspace` | Delete character |
| `Left/Right` | Move cursor within text |
| `Home/End` | Move cursor to start/end |

On entering edit mode, the cursor is placed at the end of the existing value.

Number fields reject non-digit characters during editing. On confirm, if the parsed value is 0 or unparseable, the edit is cancelled and the previous value is restored.

## Applying Changes

- Toggle and Choice changes apply to `app.config` immediately (session effect)
- Text/Number edits apply on confirm (`Enter`), not on every keystroke
- Format changes (preset/tv_format/movie_format) take effect on the next rip cycle

## Format Conflict Handling

- When preset is set to anything other than *(none)*, TV Format and Movie Format text fields show the preset's template in dimmed/italic style and are non-editable
- Cycling preset to *(none)* re-enables the text fields
- Special Format is always editable (not affected by preset)

## Save to Config

### Writing

- Writes to the resolved config path (from `--config` / `BLUBACK_CONFIG` / default)
- Creates file and parent directories if they don't exist
- **All settings are written to the file.** Settings at their default value are commented out; modified settings are written as active TOML. This makes the config file self-documenting.

Example output:
```toml
# output_dir = "."
# device = "auto-detect"
eject = true
# max_speed = true
min_duration = 600
# preset = "default"
# tv_format = "S{season}E{episode}_{title}.mkv"
# movie_format = "{title}_({year}).mkv"
# special_format = "{show} S00E{episode} {title}.mkv"
# show_filtered = false
# tmdb_api_key = ""
```

### Generating the config file

The save function generates the TOML manually (not via serde Serialize) to support the commented-out-defaults pattern. A helper function compares each field against its default and emits either `key = value` or `# key = default_value`.

Saving overwrites the entire config file. Unknown keys from a newer version of bluback will be lost. User comments are not preserved.

### After Save

- Shows "Saved!" next to the Save action item for 2 seconds
- Sets `dirty: false`
- If NOT on the Ripping screen: resets workflow (calls `reset_for_rescan` + `start_disc_scan`) to pick up any changed settings (device, output dir, min_duration, etc.)
- If on the Ripping screen: applies what it can without disrupting the active rip (eject, format settings apply to future rips)

## `--settings` Standalone Mode

- New clap flag: `--settings` (conflicts with `--dry-run`, `--no-tui`)
- If `--settings` is specified and stdout is not a TTY, exit with an error message
- Dispatch happens in `main()` BEFORE device detection and dependency checking, since neither ffmpeg nor an optical drive is needed for editing config
- Enters TUI with overlay immediately open on a minimal/blank background
- On close (`Esc`), if `dirty`, prompts inline: "Unsaved changes. Save before closing? (y/n/Esc)" — this `confirm_close` prompt only applies in `--settings` standalone mode. In normal TUI mode, closing the overlay with unsaved changes does NOT prompt, because changes are already applied to the current session (just not persisted to disk).
- After close, app exits (no wizard flow)

## `--config` CLI Flag

- New clap flag: `--config <PATH>` — path to config TOML file
- Also checks `BLUBACK_CONFIG` env var as fallback before the default path
- Affects both loading and saving
- The resolved path is stored as `config_path: PathBuf` on `App`

## New Config Fields

Two new fields added to `Config`:

```rust
pub output_dir: Option<String>,
pub device: Option<String>,
```

These are applied during `App` initialization:
- `output_dir` sets `args.output` if not overridden by CLI `-o`
- `device` sets `args.device` if not overridden by CLI `-d`

When these settings are changed via the settings panel, the handler also updates `app.args.output` / `app.args.device` directly (in addition to `app.config`), so that `start_disc_scan` and rip output paths use the new values immediately.

## Files Affected

- `src/main.rs` — Add `--settings` and `--config` flags, env var resolution, dispatch to settings-only mode, apply new config fields to args
- `src/config.rs` — Accept custom path in `load()`, add `save()` method with commented-defaults output, env var lookup, new fields, default constants
- `src/types.rs` — Add `Overlay`, `SettingsState`, `SettingItem` types
- `src/tui/mod.rs` — Add `overlay` field to `App`, input routing logic, overlay render dispatch, rescan-after-save logic
- `src/tui/settings.rs` — New file: render and input handler for settings overlay
- `src/tui/wizard.rs` — Minor: hint text update to show Ctrl+S
- `src/tui/dashboard.rs` — Minor: hint text update to show Ctrl+S

## Known Limitations

- **`min_duration` sentinel**: The current `Config::min_duration()` method uses `cli_min_duration != 900` to detect explicit CLI override, because clap's `default_value = "900"` is indistinguishable from an explicit `--min-duration 900`. This means a config-panel change to `min_duration` will override an explicit `--min-duration 900` CLI flag after rescan. Acceptable for now; a future fix would change `Args.min_duration` to `Option<u32>`.

## Testing

- `config.rs` — Config path resolution priority (flag > env > default), save/load roundtrip, commented-defaults output format, new field parsing
- `types.rs` — `SettingItem` construction from `Config`, `SettingsState` initialization
- No hardware or TUI rendering tests needed
