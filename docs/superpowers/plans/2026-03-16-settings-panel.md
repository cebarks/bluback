# Settings Panel Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a TUI settings popup overlay accessible via Ctrl+S and --settings standalone mode, with config file save/load support and custom config path resolution.

**Architecture:** Overlay system (`App.overlay: Option<Overlay>`) with input routing that intercepts all keys when overlay is open. Settings state tracks items (toggles, choices, text fields), edit mode, dirty flag, and save status. Config path resolved from --config flag > BLUBACK_CONFIG env > default path.

**Tech Stack:** Rust, ratatui, crossterm, serde (Serialize + Deserialize), toml, clap

**Spec:** `docs/superpowers/specs/2026-03-16-settings-panel-design.md`

---

## Chunk 1: Config Path Resolution & Save Support

### Task 1: Add Serialize to Config and config path resolution

**Files:**
- Modify: `src/config.rs:1-42` (imports, Config struct, load_config)

- [ ] **Step 1: Write failing tests for config path resolution and save/load roundtrip**

Add these tests to the existing `#[cfg(test)] mod tests` block in `src/config.rs`:

```rust
#[test]
fn test_resolve_config_path_flag_highest_priority() {
    let path = resolve_config_path(Some("/custom/config.toml"), None);
    assert_eq!(path, PathBuf::from("/custom/config.toml"));
}

#[test]
fn test_resolve_config_path_env_fallback() {
    let path = resolve_config_path(None, Some("/env/config.toml"));
    assert_eq!(path, PathBuf::from("/env/config.toml"));
}

#[test]
fn test_resolve_config_path_default() {
    let path = resolve_config_path(None, None);
    assert!(path.ends_with(".config/bluback/config.toml"));
}

#[test]
fn test_save_roundtrip() {
    let dir = std::env::temp_dir().join("bluback_test_save");
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("config.toml");

    let config = Config {
        tmdb_api_key: Some("key123".into()),
        preset: Some("plex".into()),
        tv_format: None,
        movie_format: None,
        eject: Some(true),
        max_speed: None,
    };

    config.save(&path).unwrap();
    let loaded = load_config_from(&path);
    assert_eq!(loaded.tmdb_api_key.as_deref(), Some("key123"));
    assert_eq!(loaded.preset.as_deref(), Some("plex"));
    assert!(loaded.tv_format.is_none());
    assert!(loaded.movie_format.is_none());
    assert_eq!(loaded.eject, Some(true));
    assert!(loaded.max_speed.is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_save_creates_parent_dirs() {
    let dir = std::env::temp_dir().join("bluback_test_mkdir/nested");
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    let path = dir.join("config.toml");

    let config = Config::default();
    config.save(&path).unwrap();
    assert!(path.exists());

    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn test_save_omits_none_fields() {
    let dir = std::env::temp_dir().join("bluback_test_omit");
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("config.toml");

    let config = Config {
        preset: Some("jellyfin".into()),
        ..Default::default()
    };
    config.save(&path).unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("preset"));
    assert!(!contents.contains("tmdb_api_key"));
    assert!(!contents.contains("tv_format"));
    assert!(!contents.contains("eject"));

    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests -- --test-threads=1`
Expected: FAIL — `resolve_config_path`, `load_config_from`, `Config::save` don't exist yet.

- [ ] **Step 3: Implement config path resolution and save**

In `src/config.rs`, make these changes:

1. Add `Serialize` to imports and Config derive:

```rust
use serde::{Deserialize, Serialize};
```

```rust
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmdb_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tv_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub movie_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eject: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_speed: Option<bool>,
}
```

2. Add `resolve_config_path` function:

```rust
pub fn resolve_config_path(cli_path: Option<&str>, env_path: Option<&str>) -> PathBuf {
    if let Some(p) = cli_path {
        return PathBuf::from(p);
    }
    if let Some(p) = env_path {
        return PathBuf::from(p);
    }
    config_dir().join("config.toml")
}
```

3. Add `load_config_from` function (loads from a specific path):

```rust
pub fn load_config_from(path: &std::path::Path) -> Config {
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

4. Update existing `load_config` to delegate:

```rust
pub fn load_config() -> Config {
    load_config_from(&config_dir().join("config.toml"))
}
```

5. Add `save` method to Config impl:

```rust
pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let contents = toml::to_string_pretty(self)?;
    fs::write(path, contents)?;
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config::tests -- --test-threads=1`
Expected: All pass.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy`
Expected: No new warnings.

- [ ] **Step 6: Commit**

```
feat: add config path resolution and save support

Add Serialize derive to Config with skip_serializing_if for Option
fields. Add resolve_config_path (--config flag > env var > default),
load_config_from (path-specific loading), and Config::save method.
```

---

### Task 2: Add --config and --settings CLI flags and early dispatch

**Files:**
- Modify: `src/main.rs:10-108` (Args struct, main function)

- [ ] **Step 1: Add CLI flags to Args struct**

In `src/main.rs`, add these fields to the `Args` struct after the `no_max_speed` field:

```rust
    /// Open settings panel and exit
    #[arg(long, conflicts_with_all = ["dry_run", "no_tui"])]
    pub settings: bool,

    /// Path to config file
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,
```

- [ ] **Step 2: Update main() for config path resolution and --settings early dispatch**

Replace the `main()` function body with:

```rust
fn main() -> anyhow::Result<()> {
    let mut args = Args::parse();

    let env_config = std::env::var("BLUBACK_CONFIG").ok();
    let config_path = crate::config::resolve_config_path(
        args.config.as_ref().map(|p| p.to_str().unwrap()),
        env_config.as_deref(),
    );
    let config = crate::config::load_config_from(&config_path);

    // --settings mode: skip device detection and dependency check
    if args.settings {
        return tui::run_settings(&config, &config_path);
    }

    if args.device.is_none() {
        let drives = disc::detect_optical_drives();
        args.device = Some(drives[0].clone());
    }

    disc::check_dependencies()?;

    let use_tui = !args.no_tui && atty_stdout();

    if use_tui {
        tui::run(&args, &config, &config_path)
    } else {
        cli::run(&args, &config)
    }
}
```

Note: `tui::run` and `tui::run_settings` signatures will be updated in Task 4. This won't compile yet — that's expected, we'll fix the signatures in the TUI tasks.

- [ ] **Step 3: Verify it compiles (after TUI changes in Task 4)**

This task will compile after Task 4. Mark as pending until then.

- [ ] **Step 4: Commit**

```
feat: add --config and --settings CLI flags with early dispatch

--config <PATH> and BLUBACK_CONFIG env var control config file location.
--settings opens settings panel without requiring disc or ffmpeg.
```

---

## Chunk 2: Types & Settings State

### Task 3: Add Overlay and SettingsState types

**Files:**
- Modify: `src/types.rs:1-5` (imports)
- Modify: `src/types.rs` (add new types after `BackgroundResult`)

- [ ] **Step 1: Write failing tests for SettingsState initialization from Config**

Add to the `#[cfg(test)] mod tests` block in `src/types.rs`:

```rust
#[test]
fn test_settings_state_from_config_defaults() {
    let config = crate::config::Config::default();
    let state = SettingsState::from_config(&config);

    assert_eq!(state.cursor, 0);
    assert!(!state.dirty);
    assert!(state.editing.is_none());
    assert!(state.save_message.is_none());

    // Check preset is Choice with "none" selected (index 0)
    match &state.items[0] {
        SettingItem::Choice { selected, options, .. } => {
            assert_eq!(*selected, 0);
            assert_eq!(options[0], "(none)");
        }
        _ => panic!("Expected Choice for preset"),
    }

    // Check eject is Toggle defaulting to false
    match &state.items[4] {
        SettingItem::Toggle { value, .. } => assert!(!value),
        _ => panic!("Expected Toggle for eject"),
    }

    // Check max_speed is Toggle defaulting to true
    match &state.items[5] {
        SettingItem::Toggle { value, .. } => assert!(*value),
        _ => panic!("Expected Toggle for max_speed"),
    }
}

#[test]
fn test_settings_state_from_config_with_values() {
    let config = crate::config::Config {
        tmdb_api_key: Some("mykey".into()),
        preset: Some("plex".into()),
        tv_format: Some("custom_tv".into()),
        movie_format: Some("custom_movie".into()),
        eject: Some(true),
        max_speed: Some(false),
    };
    let state = SettingsState::from_config(&config);

    // Preset should select "plex" (index 2)
    match &state.items[0] {
        SettingItem::Choice { selected, options, .. } => {
            assert_eq!(options[*selected], "plex");
        }
        _ => panic!("Expected Choice"),
    }

    // TV format
    match &state.items[1] {
        SettingItem::Text { value, .. } => assert_eq!(value, "custom_tv"),
        _ => panic!("Expected Text"),
    }

    // Movie format
    match &state.items[2] {
        SettingItem::Text { value, .. } => assert_eq!(value, "custom_movie"),
        _ => panic!("Expected Text"),
    }

    // TMDb API key
    match &state.items[3] {
        SettingItem::Text { value, .. } => assert_eq!(value, "mykey"),
        _ => panic!("Expected Text"),
    }

    // Eject
    match &state.items[4] {
        SettingItem::Toggle { value, .. } => assert!(*value),
        _ => panic!("Expected Toggle"),
    }

    // Max speed
    match &state.items[5] {
        SettingItem::Toggle { value, .. } => assert!(!*value),
        _ => panic!("Expected Toggle"),
    }
}

#[test]
fn test_settings_state_to_config() {
    let config = crate::config::Config {
        tmdb_api_key: Some("mykey".into()),
        preset: Some("jellyfin".into()),
        tv_format: None,
        movie_format: None,
        eject: Some(true),
        max_speed: Some(false),
    };
    let state = SettingsState::from_config(&config);
    let result = state.to_config();

    assert_eq!(result.tmdb_api_key.as_deref(), Some("mykey"));
    assert_eq!(result.preset.as_deref(), Some("jellyfin"));
    assert!(result.tv_format.is_none());
    assert!(result.movie_format.is_none());
    assert_eq!(result.eject, Some(true));
    assert_eq!(result.max_speed, Some(false));
}

#[test]
fn test_settings_state_to_config_none_preset() {
    let config = crate::config::Config::default();
    let state = SettingsState::from_config(&config);
    let result = state.to_config();
    assert!(result.preset.is_none());
}

#[test]
fn test_settings_item_count() {
    let config = crate::config::Config::default();
    let state = SettingsState::from_config(&config);
    // 6 settings + 1 separator + 1 action = 8 items
    assert_eq!(state.items.len(), 8);
    assert!(matches!(state.items[6], SettingItem::Separator));
    assert!(matches!(state.items[7], SettingItem::Action { .. }));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib types::tests`
Expected: FAIL — types don't exist yet.

- [ ] **Step 3: Implement the types**

Add to `src/types.rs` after the `BackgroundResult` enum (before `#[cfg(test)]`):

```rust
pub enum Overlay {
    Settings(SettingsState),
}

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
    Separator,
    Action {
        label: String,
    },
}

const PRESET_OPTIONS: &[&str] = &["(none)", "default", "plex", "jellyfin"];

impl SettingsState {
    pub fn from_config(config: &crate::config::Config) -> Self {
        let preset_idx = config
            .preset
            .as_deref()
            .and_then(|p| PRESET_OPTIONS.iter().position(|&o| o == p))
            .unwrap_or(0);

        let items = vec![
            SettingItem::Choice {
                label: "Preset".into(),
                key: "preset".into(),
                options: PRESET_OPTIONS.iter().map(|s| s.to_string()).collect(),
                selected: preset_idx,
            },
            SettingItem::Text {
                label: "TV Format".into(),
                key: "tv_format".into(),
                value: config.tv_format.clone().unwrap_or_default(),
            },
            SettingItem::Text {
                label: "Movie Format".into(),
                key: "movie_format".into(),
                value: config.movie_format.clone().unwrap_or_default(),
            },
            SettingItem::Text {
                label: "TMDb API Key".into(),
                key: "tmdb_api_key".into(),
                value: config.tmdb_api_key.clone().unwrap_or_default(),
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
            SettingItem::Separator,
            SettingItem::Action {
                label: "Save to Config (Ctrl+S)".into(),
            },
        ];

        Self {
            cursor: 0,
            items,
            editing: None,
            input_buffer: String::new(),
            dirty: false,
            save_message: None,
            save_message_at: None,
            confirm_close: None,
        }
    }

    pub fn to_config(&self) -> crate::config::Config {
        let mut config = crate::config::Config::default();
        for item in &self.items {
            match item {
                SettingItem::Choice { key, options, selected, .. } => {
                    if key == "preset" {
                        let val = &options[*selected];
                        config.preset = if val == "(none)" {
                            None
                        } else {
                            Some(val.clone())
                        };
                    }
                }
                SettingItem::Text { key, value, .. } => {
                    let opt = if value.is_empty() { None } else { Some(value.clone()) };
                    match key.as_str() {
                        "tv_format" => config.tv_format = opt,
                        "movie_format" => config.movie_format = opt,
                        "tmdb_api_key" => config.tmdb_api_key = opt,
                        _ => {}
                    }
                }
                SettingItem::Toggle { key, value, .. } => match key.as_str() {
                    "eject" => config.eject = Some(*value),
                    "max_speed" => config.max_speed = Some(*value),
                    _ => {}
                },
                _ => {}
            }
        }
        config
    }

    /// Returns true if the cursor is on a navigable item (not a separator)
    pub fn cursor_on_navigable(&self) -> bool {
        !matches!(self.items.get(self.cursor), Some(SettingItem::Separator))
    }

    /// Move cursor up, skipping separators
    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            if matches!(self.items.get(self.cursor), Some(SettingItem::Separator)) && self.cursor > 0 {
                self.cursor -= 1;
            }
        }
    }

    /// Move cursor down, skipping separators
    pub fn move_down(&mut self) {
        if self.cursor < self.items.len() - 1 {
            self.cursor += 1;
            if matches!(self.items.get(self.cursor), Some(SettingItem::Separator))
                && self.cursor < self.items.len() - 1
            {
                self.cursor += 1;
            }
        }
    }

    /// Whether the preset is set (format text fields should be dimmed)
    pub fn preset_active(&self) -> bool {
        matches!(&self.items[0], SettingItem::Choice { selected, .. } if *selected != 0)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib types::tests`
Expected: All pass.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy`
Expected: No new warnings.

- [ ] **Step 6: Commit**

```
feat: add Overlay, SettingsState, and SettingItem types

SettingsState::from_config builds the settings list from Config.
SettingsState::to_config converts back. Supports Toggle, Choice,
Text, Separator, and Action item types with cursor navigation.
```

---

## Chunk 3: TUI Overlay Integration

### Task 4: Add overlay field to App and update TUI plumbing

**Files:**
- Modify: `src/tui/mod.rs:1-2` (module declarations)
- Modify: `src/tui/mod.rs:32-91` (App struct)
- Modify: `src/tui/mod.rs:93-136` (App::new)
- Modify: `src/tui/mod.rs:233-245` (run function signature)
- Modify: `src/tui/mod.rs:247-390` (run_app — render + input routing)

- [ ] **Step 1: Add `pub mod settings;` to module declarations**

At the top of `src/tui/mod.rs`, after `pub mod wizard;`:

```rust
pub mod settings;
```

- [ ] **Step 2: Add overlay and config_path fields to App**

Add to the `App` struct, after the `pending_rx` field:

```rust
    // Overlay
    pub overlay: Option<crate::types::Overlay>,

    // Config path (for saving)
    pub config_path: std::path::PathBuf,
```

Update `App::new` to initialize them:

```rust
            overlay: None,
            config_path: std::path::PathBuf::new(),
```

- [ ] **Step 3: Update `run` function signature to accept config_path**

Change the `run` function signature:

```rust
pub fn run(args: &Args, config: &crate::config::Config, config_path: &std::path::Path) -> Result<()> {
```

And pass it to `run_app`:

```rust
    let result = run_app(&mut terminal, args, config, config_path);
```

Update `run_app` signature:

```rust
fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    args: &Args,
    config: &crate::config::Config,
    config_path: &std::path::Path,
) -> Result<()> {
```

Set `config_path` on app after construction:

```rust
    app.config_path = config_path.to_path_buf();
```

- [ ] **Step 4: Add `run_settings` function for standalone --settings mode**

Add to `src/tui/mod.rs`:

```rust
pub fn run_settings(config: &crate::config::Config, config_path: &std::path::Path) -> Result<()> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_settings_app(&mut terminal, config, config_path);

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_settings_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config: &crate::config::Config,
    config_path: &std::path::Path,
) -> Result<()> {
    let overlay = crate::types::Overlay::Settings(
        crate::types::SettingsState::from_config(config),
    );

    let mut quit = false;
    let mut overlay = Some(overlay);
    let config_path = config_path.to_path_buf();

    loop {
        terminal.draw(|f| {
            // Blank background
            let area = f.area();
            f.render_widget(
                ratatui::widgets::Block::default().style(Style::default().bg(Color::Black)),
                area,
            );
            // Render overlay on top
            if let Some(crate::types::Overlay::Settings(ref state)) = overlay {
                settings::render(f, state, area);
            }
        })?;

        if quit {
            break;
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                if let Some(crate::types::Overlay::Settings(ref mut state)) = overlay {
                    let action = settings::handle_input(state, key);
                    match action {
                        settings::SettingsAction::None => {}
                        settings::SettingsAction::Close => {
                            quit = true;
                        }
                        settings::SettingsAction::Save => {
                            let new_config = state.to_config();
                            if let Err(e) = new_config.save(&config_path) {
                                state.save_message = Some(format!("Error: {}", e));
                            } else {
                                state.save_message = Some("Saved!".into());
                                state.save_message_at = Some(std::time::Instant::now());
                                state.dirty = false;
                            }
                        }
                        settings::SettingsAction::SaveAndClose => {
                            let new_config = state.to_config();
                            let _ = new_config.save(&config_path);
                            quit = true;
                        }
                    }
                }
            }
        }

        // Clear save message after 2 seconds
        if let Some(crate::types::Overlay::Settings(ref mut state)) = overlay {
            if let Some(at) = state.save_message_at {
                if at.elapsed() > Duration::from_secs(2) {
                    state.save_message = None;
                    state.save_message_at = None;
                }
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 5: Update render loop to draw overlay on top**

In `run_app`, change the `terminal.draw` call to:

```rust
        terminal.draw(|f| {
            match app.screen {
                Screen::Scanning => wizard::render_scanning(f, &app),
                Screen::TmdbSearch => wizard::render_tmdb_search(f, &app),
                Screen::ShowSelect => wizard::render_show_select(f, &app),
                Screen::SeasonEpisode => wizard::render_season_episode(f, &app),
                Screen::EpisodeMapping => wizard::render_episode_mapping(f, &app),
                Screen::PlaylistSelect => wizard::render_playlist_select(f, &app),
                Screen::Confirm => wizard::render_confirm(f, &app),
                Screen::Ripping => dashboard::render(f, &app),
                Screen::Done => dashboard::render_done(f, &app),
            }
            // Draw overlay on top if present
            if let Some(crate::types::Overlay::Settings(ref state)) = app.overlay {
                settings::render(f, state, f.area());
            }
        })?;
```

- [ ] **Step 6: Add Ctrl+S global handler and overlay input routing**

In `run_app`, in the input handling section, add overlay routing BEFORE the global quit handlers. Insert this right after `if let Event::Key(key) = event::read()? {`:

```rust
                // Overlay input routing — overlay consumes all keys except Ctrl+C
                if app.overlay.is_some() {
                    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        app.quit = true;
                        continue;
                    }
                    if let Some(crate::types::Overlay::Settings(ref mut state)) = app.overlay {
                        // Clear save message on any input
                        if state.save_message.is_some() {
                            state.save_message = None;
                            state.save_message_at = None;
                        }
                        let action = settings::handle_input(state, key);
                        match action {
                            settings::SettingsAction::None => {}
                            settings::SettingsAction::Close => {
                                // Apply settings to app.config before closing
                                let new_config = state.to_config();
                                app.config = new_config;
                                app.overlay = None;
                            }
                            settings::SettingsAction::Save => {
                                let new_config = state.to_config();
                                if let Err(e) = new_config.save(&app.config_path) {
                                    state.save_message = Some(format!("Error: {}", e));
                                } else {
                                    state.save_message = Some("Saved!".into());
                                    state.save_message_at = Some(std::time::Instant::now());
                                    state.dirty = false;
                                    app.config = new_config;
                                }
                            }
                            settings::SettingsAction::SaveAndClose => {
                                let new_config = state.to_config();
                                let _ = new_config.save(&app.config_path);
                                app.config = new_config;
                                app.overlay = None;
                            }
                        }
                    }
                    continue;
                }
```

Add the `Ctrl+S` handler after the `Ctrl+R` handler (before `// Handle rescan confirmation`):

```rust
                // Global Ctrl+S: open settings overlay
                if key.code == KeyCode::Char('s')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !app.input_active
                    && !app.confirm_abort
                    && !app.confirm_rescan
                {
                    app.overlay = Some(crate::types::Overlay::Settings(
                        crate::types::SettingsState::from_config(&app.config),
                    ));
                    continue;
                }
```

- [ ] **Step 7: Add save message timeout check in event loop**

After `poll_background(&mut app);` in the main event loop, add:

```rust
        // Clear overlay save message after 2 seconds
        if let Some(crate::types::Overlay::Settings(ref mut state)) = app.overlay {
            if let Some(at) = state.save_message_at {
                if at.elapsed() > Duration::from_secs(2) {
                    state.save_message = None;
                    state.save_message_at = None;
                }
            }
        }
```

- [ ] **Step 8: Create stub `src/tui/settings.rs`**

Create `src/tui/settings.rs` with stub functions so the project compiles:

```rust
use crossterm::event::KeyEvent;
use ratatui::prelude::*;

use crate::types::SettingsState;

pub enum SettingsAction {
    None,
    Close,
    Save,
    SaveAndClose,
}

pub fn render(_f: &mut Frame, _state: &SettingsState, _area: Rect) {
    // Stub — implemented in Task 5
}

pub fn handle_input(_state: &mut SettingsState, _key: KeyEvent) -> SettingsAction {
    // Stub — implemented in Task 6
    SettingsAction::None
}
```

- [ ] **Step 9: Verify everything compiles**

Run: `cargo build`
Expected: Compiles successfully.

- [ ] **Step 10: Run all existing tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 11: Commit**

```
feat: add overlay system to TUI with Ctrl+S settings hotkey

Overlay field on App routes input when present. Ctrl+S opens
settings popup from any screen. run_settings provides standalone
--settings mode. Settings stub module for render/input.
```

---

## Chunk 4: Settings Panel Render & Input

### Task 5: Implement settings popup rendering

**Files:**
- Modify: `src/tui/settings.rs` (replace render stub)

- [ ] **Step 1: Implement the render function**

Replace the `render` stub in `src/tui/settings.rs`:

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::types::{SettingItem, SettingsState};

pub enum SettingsAction {
    None,
    Close,
    Save,
    SaveAndClose,
}

pub fn render(f: &mut Frame, state: &SettingsState, area: Rect) {
    let popup_width = 54u16.min(area.width.saturating_sub(4));
    let popup_height = (state.items.len() as u16 + 4).min(area.height.saturating_sub(2));

    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    // Clear background
    f.render_widget(Clear, popup_area);

    let title = if state.dirty {
        " Settings (modified) "
    } else {
        " Settings "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_alignment(Alignment::Center)
        .border_style(Style::default().fg(Color::Cyan));

    f.render_widget(block, popup_area);

    let inner = Rect::new(
        popup_area.x + 2,
        popup_area.y + 1,
        popup_area.width.saturating_sub(4),
        popup_area.height.saturating_sub(2),
    );

    let preset_active = state.preset_active();

    for (i, item) in state.items.iter().enumerate() {
        if i >= inner.height as usize {
            break;
        }
        let row_area = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        let is_selected = i == state.cursor;

        match item {
            SettingItem::Toggle { label, value, .. } => {
                let val_str = if *value { "[ON]" } else { "[OFF]" };
                let val_style = if *value {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Red)
                };
                let label_width = inner.width.saturating_sub(6) as usize;
                let line = format!("{:<width$}", label, width = label_width);

                let base_style = if is_selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };

                let spans = Line::from(vec![
                    Span::styled(line, base_style),
                    Span::styled(val_str, if is_selected { base_style } else { val_style }),
                ]);
                f.render_widget(Paragraph::new(spans), row_area);
            }
            SettingItem::Choice { label, options, selected, .. } => {
                let val_str = format!("[{}]", options[*selected]);
                let label_width = inner.width.saturating_sub(val_str.len() as u16 + 1) as usize;
                let line = format!("{:<width$}", label, width = label_width);

                let base_style = if is_selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };

                let spans = Line::from(vec![
                    Span::styled(line, base_style),
                    Span::styled(val_str, base_style.fg(Color::Yellow)),
                ]);
                f.render_widget(Paragraph::new(spans), row_area);
            }
            SettingItem::Text { label, key, value, .. } => {
                let is_format_field = key == "tv_format" || key == "movie_format";
                let dimmed = is_format_field && preset_active;
                let is_editing = state.editing == Some(i);

                let display_val = if is_editing {
                    format!("{}_", &state.input_buffer)
                } else if dimmed {
                    let preset_val = get_preset_format_preview(
                        &state.items[0],
                        key == "movie_format",
                    );
                    preset_val
                } else if value.is_empty() {
                    "(empty)".into()
                } else {
                    value.clone()
                };

                let label_width = (inner.width / 3) as usize;
                let val_width = inner.width as usize - label_width - 1;
                let truncated = if display_val.len() > val_width {
                    format!("{}...", &display_val[..val_width.saturating_sub(3)])
                } else {
                    display_val
                };

                let label_str = format!("{:<width$} ", label, width = label_width);

                let base_style = if is_selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };

                let val_style = if dimmed {
                    base_style.add_modifier(Modifier::DIM | Modifier::ITALIC)
                } else if is_editing {
                    base_style.fg(Color::Yellow)
                } else {
                    base_style
                };

                let spans = Line::from(vec![
                    Span::styled(label_str, base_style),
                    Span::styled(truncated, val_style),
                ]);
                f.render_widget(Paragraph::new(spans), row_area);
            }
            SettingItem::Separator => {
                let line = "─".repeat(inner.width as usize);
                let style = Style::default().fg(Color::DarkGray);
                f.render_widget(Paragraph::new(Span::styled(line, style)), row_area);
            }
            SettingItem::Action { label } => {
                let base_style = if is_selected {
                    Style::default()
                        .add_modifier(Modifier::REVERSED | Modifier::BOLD)
                        .fg(Color::Cyan)
                } else {
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(Color::Cyan)
                };

                let mut text = label.clone();
                if let Some(ref msg) = state.save_message {
                    text = format!("{} — {}", label, msg);
                }

                f.render_widget(
                    Paragraph::new(Span::styled(text, base_style)),
                    row_area,
                );
            }
        }
    }

    // Confirm close prompt
    if state.confirm_close.is_some() {
        let prompt_y = popup_area.y + popup_area.height.saturating_sub(1);
        let prompt_area = Rect::new(popup_area.x + 1, prompt_y, popup_area.width - 2, 1);
        f.render_widget(Clear, prompt_area);
        f.render_widget(
            Paragraph::new(Span::styled(
                " Unsaved changes. Save? (y/n/Esc) ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
            prompt_area,
        );
    }
}

fn get_preset_format_preview(preset_item: &SettingItem, is_movie: bool) -> String {
    if let SettingItem::Choice { options, selected, .. } = preset_item {
        let preset = &options[*selected];
        match (preset.as_str(), is_movie) {
            ("plex", false) => crate::config::PLEX_TV_FORMAT.into(),
            ("plex", true) => crate::config::PLEX_MOVIE_FORMAT.into(),
            ("jellyfin", false) => crate::config::JELLYFIN_TV_FORMAT.into(),
            ("jellyfin", true) => crate::config::JELLYFIN_MOVIE_FORMAT.into(),
            (_, false) => crate::config::DEFAULT_TV_FORMAT.into(),
            (_, true) => crate::config::DEFAULT_MOVIE_FORMAT.into(),
        }
    } else {
        String::new()
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: Compiles.

- [ ] **Step 3: Commit**

```
feat: implement settings popup rendering

Centered bordered popup with Toggle (ON/OFF with color), Choice
(cycling with yellow highlight), Text (inline editing, dimmed when
preset active), Separator, and Action items. Dirty indicator in
title. Confirm-close prompt at bottom.
```

---

### Task 6: Implement settings input handling

**Files:**
- Modify: `src/tui/settings.rs` (replace handle_input stub)

- [ ] **Step 1: Implement handle_input**

Replace the `handle_input` stub:

```rust
pub fn handle_input(state: &mut SettingsState, key: KeyEvent) -> SettingsAction {
    // Handle confirm close prompt
    if let Some(_) = state.confirm_close {
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

    // Ctrl+S: save (confirm text edit first if active)
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if state.editing.is_some() {
            confirm_text_edit(state);
        }
        return SettingsAction::Save;
    }

    // Text editing mode
    if let Some(edit_idx) = state.editing {
        return match key.code {
            KeyCode::Enter => {
                confirm_text_edit(state);
                SettingsAction::None
            }
            KeyCode::Esc => {
                state.editing = None;
                state.input_buffer.clear();
                SettingsAction::None
            }
            KeyCode::Backspace => {
                state.input_buffer.pop();
                SettingsAction::None
            }
            KeyCode::Char(c) => {
                state.input_buffer.push(c);
                SettingsAction::None
            }
            _ => SettingsAction::None,
        };
    }

    // Normal navigation mode
    match key.code {
        KeyCode::Up => {
            state.move_up();
            SettingsAction::None
        }
        KeyCode::Down => {
            state.move_down();
            SettingsAction::None
        }
        KeyCode::Esc => {
            if state.dirty {
                state.confirm_close = Some(true);
                SettingsAction::None
            } else {
                SettingsAction::Close
            }
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            handle_item_activate(state, key.code == KeyCode::Char(' '))
        }
        KeyCode::Left => {
            handle_item_cycle(state, false);
            SettingsAction::None
        }
        KeyCode::Right => {
            handle_item_cycle(state, true);
            SettingsAction::None
        }
        _ => SettingsAction::None,
    }
}

fn confirm_text_edit(state: &mut SettingsState) {
    if let Some(idx) = state.editing {
        if let Some(SettingItem::Text { value, .. }) = state.items.get_mut(idx) {
            *value = state.input_buffer.clone();
            state.dirty = true;
        }
        state.editing = None;
        state.input_buffer.clear();
    }
}

fn handle_item_activate(state: &mut SettingsState, space_only: bool) -> SettingsAction {
    let idx = state.cursor;
    match state.items.get_mut(idx) {
        Some(SettingItem::Toggle { value, .. }) => {
            *value = !*value;
            state.dirty = true;
            SettingsAction::None
        }
        Some(SettingItem::Choice { options, selected, .. }) if !space_only => {
            *selected = (*selected + 1) % options.len();
            state.dirty = true;
            SettingsAction::None
        }
        Some(SettingItem::Text { key, value, .. }) if !space_only => {
            let is_format = key == "tv_format" || key == "movie_format";
            if is_format && state.preset_active() {
                // Can't edit format fields when preset is active
                return SettingsAction::None;
            }
            state.editing = Some(idx);
            state.input_buffer = value.clone();
            SettingsAction::None
        }
        Some(SettingItem::Action { .. }) if !space_only => SettingsAction::Save,
        _ => SettingsAction::None,
    }
}

fn handle_item_cycle(state: &mut SettingsState, forward: bool) {
    let idx = state.cursor;
    if let Some(SettingItem::Choice { options, selected, .. }) = state.items.get_mut(idx) {
        if forward {
            *selected = (*selected + 1) % options.len();
        } else {
            *selected = if *selected == 0 {
                options.len() - 1
            } else {
                *selected - 1
            };
        }
        state.dirty = true;
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: Compiles.

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy`
Expected: No new warnings.

- [ ] **Step 5: Commit**

```
feat: implement settings popup input handling

Navigation (Up/Down), Toggle (Enter/Space), Choice (Enter/Left/Right
cycling), Text (inline edit with Enter/Esc/Backspace), Save action,
Ctrl+S save shortcut, confirm-close prompt on Esc with unsaved changes.
Format fields locked when preset is active.
```

---

## Chunk 5: Final Integration & Testing

### Task 7: Integration verification and manual testing

**Files:**
- No new files

- [ ] **Step 1: Build release**

Run: `cargo build --release`
Expected: Compiles.

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy`
Expected: No warnings.

- [ ] **Step 4: Verify --settings standalone mode**

Run: `cargo run -- --settings`
Expected: Opens settings popup on blank background. Can navigate, toggle, cycle, edit text, save, close.

- [ ] **Step 5: Verify --config flag**

Run: `cargo run -- --settings --config /tmp/test-bluback.toml`
Expected: Opens settings, saving writes to `/tmp/test-bluback.toml`.

- [ ] **Step 6: Verify BLUBACK_CONFIG env var**

Run: `BLUBACK_CONFIG=/tmp/env-bluback.toml cargo run -- --settings`
Expected: Saves to `/tmp/env-bluback.toml`.

- [ ] **Step 7: Verify Ctrl+S hotkey in normal TUI mode (requires disc)**

If a disc is available, run `cargo run` and press Ctrl+S to verify overlay opens over wizard screens. Otherwise, skip this step.

- [ ] **Step 8: Final commit**

```
feat: settings panel TUI overlay with --settings and --config support
```
