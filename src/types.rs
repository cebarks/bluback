use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Playlist {
    pub num: String,
    pub duration: String,
    pub seconds: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Episode {
    pub episode_number: u32,
    pub name: String,
    pub runtime: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TmdbShow {
    pub id: u64,
    pub name: String,
    pub first_air_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TmdbMovie {
    pub title: String,
    pub release_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LabelInfo {
    pub show: String,
    pub season: u32,
    pub disc: u32,
}

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub audio_streams: Vec<String>,
    pub sub_count: u32,
}

#[derive(Debug, Clone)]
pub struct ChapterMark {
    pub index: u32,
    pub start_secs: f64,
}

#[derive(Debug, Clone, Default)]
pub struct RipProgress {
    pub frame: u64,
    pub fps: f64,
    pub total_size: u64,
    pub out_time_secs: u32,
    pub bitrate: String,
    pub speed: f64,
}

#[derive(Debug, Clone)]
pub enum PlaylistStatus {
    Pending,
    Ripping(RipProgress),
    Done(u64),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct RipJob {
    pub playlist: Playlist,
    pub episode: Vec<Episode>,
    pub filename: String,
    pub status: PlaylistStatus,
}

#[derive(Debug, Clone, Default)]
pub struct MediaInfo {
    pub resolution: String,
    pub width: u32,
    pub height: u32,
    pub codec: String,
    pub hdr: String,
    pub aspect_ratio: String,
    pub framerate: String,
    pub bit_depth: String,
    pub profile: String,
    pub audio: String,
    pub channels: String,
    pub audio_lang: String,
    /// Overall bitrate in bits/s from ffprobe format section
    pub bitrate_bps: u64,
}

impl MediaInfo {
    pub fn to_vars(&self) -> std::collections::HashMap<&str, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("resolution", self.resolution.clone());
        m.insert(
            "width",
            if self.width > 0 {
                self.width.to_string()
            } else {
                String::new()
            },
        );
        m.insert(
            "height",
            if self.height > 0 {
                self.height.to_string()
            } else {
                String::new()
            },
        );
        m.insert("codec", self.codec.clone());
        m.insert("hdr", self.hdr.clone());
        m.insert("aspect_ratio", self.aspect_ratio.clone());
        m.insert("framerate", self.framerate.clone());
        m.insert("bit_depth", self.bit_depth.clone());
        m.insert("profile", self.profile.clone());
        m.insert("audio", self.audio.clone());
        m.insert("channels", self.channels.clone());
        m.insert("audio_lang", self.audio_lang.clone());
        m
    }
}

pub type EpisodeAssignments = HashMap<String, Vec<Episode>>;

pub struct TmdbLookupResult {
    pub episodes: Vec<Episode>,
    pub season: u32,
    pub show_name: String,
}

/// Result types for background operations in TUI mode
pub enum BackgroundResult {
    /// No disc detected on this device
    WaitingForDisc(String),
    /// Disc found, now scanning playlists
    DiscFound(String),
    /// Disc scan completed: (device, label, playlists)
    DiscScan(anyhow::Result<(String, String, Vec<Playlist>)>),
    /// TMDb show search completed
    ShowSearch(anyhow::Result<Vec<TmdbShow>>),
    /// TMDb movie search completed
    MovieSearch(anyhow::Result<Vec<TmdbMovie>>),
    /// TMDb season fetch completed
    SeasonFetch(anyhow::Result<Vec<Episode>>),
    /// Media info probes completed (one per selected playlist)
    MediaProbe(Vec<Option<MediaInfo>>),
}

pub enum Overlay {
    Settings(SettingsState),
}

pub struct SettingsState {
    pub cursor: usize,
    pub items: Vec<SettingItem>,
    pub editing: Option<usize>,
    pub input_buffer: String,
    pub cursor_pos: usize,
    pub dirty: bool,
    pub save_message: Option<String>,
    pub save_message_at: Option<std::time::Instant>,
    pub confirm_close: Option<bool>,
    pub scroll_offset: usize,
    pub standalone: bool,
    /// Env var overrides detected at open: (env_var_name, key, value)
    pub env_overrides: Vec<(String, String, String)>,
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
        /// For "Custom..." option: stores the user-entered value
        custom_value: Option<String>,
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

impl SettingsState {
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self::from_config_with_drives(config, &[])
    }

    pub fn from_config_with_drives(config: &crate::config::Config, detected_drives: &[String]) -> Self {
        use crate::config::*;

        // Build device options: auto-detect, detected drives, Custom...
        let mut device_options = vec![DEFAULT_DEVICE.to_string()];
        for drive in detected_drives {
            let s = drive.to_string();
            if s != DEFAULT_DEVICE && !device_options.contains(&s) {
                device_options.push(s);
            }
        }
        device_options.push("Custom...".to_string());

        let configured_device = config.device.clone().unwrap_or_else(|| DEFAULT_DEVICE.into());
        let (device_selected, device_custom) = if configured_device == DEFAULT_DEVICE {
            (0, None)
        } else if let Some(pos) = device_options.iter().position(|o| o == &configured_device) {
            (pos, None)
        } else {
            // Custom value not in detected drives
            (device_options.len() - 1, Some(configured_device))
        };

        let items = vec![
            SettingItem::Separator { label: Some("General".into()) },
            SettingItem::Text {
                label: "Output Directory".into(),
                key: "output_dir".into(),
                value: config.output_dir.clone().unwrap_or_else(|| DEFAULT_OUTPUT_DIR.into()),
            },
            SettingItem::Choice {
                label: "Device".into(),
                key: "device".into(),
                options: device_options,
                selected: device_selected,
                custom_value: device_custom,
            },
            SettingItem::Toggle {
                label: "Auto-Eject After Rip".into(),
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
                custom_value: None,
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
            env_overrides: Vec::new(),
        }
    }

    /// Check for BLUBACK_* env vars and apply their values to settings items.
    /// Returns the list of overrides that were applied.
    pub fn apply_env_overrides(&mut self) {
        const ENV_MAPPINGS: &[(&str, &str)] = &[
            ("BLUBACK_OUTPUT_DIR", "output_dir"),
            ("BLUBACK_DEVICE", "device"),
            ("BLUBACK_EJECT", "eject"),
            ("BLUBACK_MAX_SPEED", "max_speed"),
            ("BLUBACK_MIN_DURATION", "min_duration"),
            ("BLUBACK_PRESET", "preset"),
            ("BLUBACK_TV_FORMAT", "tv_format"),
            ("BLUBACK_MOVIE_FORMAT", "movie_format"),
            ("BLUBACK_SPECIAL_FORMAT", "special_format"),
            ("BLUBACK_SHOW_FILTERED", "show_filtered"),
            ("TMDB_API_KEY", "tmdb_api_key"),
        ];

        let mut overrides = Vec::new();

        for &(env_var, config_key) in ENV_MAPPINGS {
            if let Ok(val) = std::env::var(env_var) {
                if val.is_empty() {
                    continue;
                }
                let applied = self.apply_env_value(config_key, &val);
                if applied {
                    overrides.push((env_var.to_string(), config_key.to_string(), val));
                }
            }
        }

        if !overrides.is_empty() {
            self.dirty = true;
            let names: Vec<&str> = overrides.iter().map(|(env, _, _)| env.as_str()).collect();
            self.save_message = Some(format!("Imported from env: {}", names.join(", ")));
            self.save_message_at = Some(std::time::Instant::now());
        }
        self.env_overrides = overrides;
    }

    fn apply_env_value(&mut self, key: &str, val: &str) -> bool {
        for item in &mut self.items {
            match item {
                SettingItem::Text { key: k, value, .. } if k == key => {
                    *value = val.to_string();
                    return true;
                }
                SettingItem::Toggle { key: k, value, .. } if k == key => {
                    match val.to_lowercase().as_str() {
                        "true" | "1" | "yes" => { *value = true; return true; }
                        "false" | "0" | "no" => { *value = false; return true; }
                        _ => return false,
                    }
                }
                SettingItem::Number { key: k, value, .. } if k == key => {
                    if let Ok(n) = val.parse::<u32>() {
                        if n > 0 {
                            *value = n;
                            return true;
                        }
                    }
                    return false;
                }
                SettingItem::Choice { key: k, options, selected, custom_value, .. } if k == key => {
                    if key == "device" {
                        // Check if the value matches a known option
                        if let Some(pos) = options.iter().position(|o| o == val) {
                            *selected = pos;
                        } else {
                            // Set as custom
                            *selected = options.len() - 1; // "Custom..."
                            *custom_value = Some(val.to_string());
                        }
                        return true;
                    } else if key == "preset" {
                        if let Some(pos) = options.iter().position(|o| o == val) {
                            *selected = pos;
                            return true;
                        }
                    }
                    return false;
                }
                _ => {}
            }
        }
        false
    }

    /// Check which env vars are currently set that would override saved config.
    pub fn active_env_var_warnings(&self) -> Vec<String> {
        const ENV_MAPPINGS: &[(&str, &str)] = &[
            ("BLUBACK_OUTPUT_DIR", "output_dir"),
            ("BLUBACK_DEVICE", "device"),
            ("BLUBACK_EJECT", "eject"),
            ("BLUBACK_MAX_SPEED", "max_speed"),
            ("BLUBACK_MIN_DURATION", "min_duration"),
            ("BLUBACK_PRESET", "preset"),
            ("BLUBACK_TV_FORMAT", "tv_format"),
            ("BLUBACK_MOVIE_FORMAT", "movie_format"),
            ("BLUBACK_SPECIAL_FORMAT", "special_format"),
            ("BLUBACK_SHOW_FILTERED", "show_filtered"),
            ("TMDB_API_KEY", "tmdb_api_key"),
        ];

        let mut warnings = Vec::new();
        for &(env_var, _) in ENV_MAPPINGS {
            if let Ok(val) = std::env::var(env_var) {
                if !val.is_empty() {
                    warnings.push(env_var.to_string());
                }
            }
        }
        warnings
    }

    pub fn to_config(&self) -> crate::config::Config {
        use crate::config::*;
        let mut config = crate::config::Config::default();

        for item in &self.items {
            match item {
                SettingItem::Text { key, value, .. } => match key.as_str() {
                    "output_dir" if value != DEFAULT_OUTPUT_DIR => config.output_dir = Some(value.clone()),
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
                SettingItem::Choice { key, options, selected, custom_value, .. } => match key.as_str() {
                    "preset" => {
                        let val = &options[*selected];
                        if val != "(none)" {
                            config.preset = Some(val.clone());
                        }
                    }
                    "device" => {
                        let val = &options[*selected];
                        if val == "Custom..." {
                            if let Some(ref cv) = custom_value {
                                if !cv.is_empty() && cv != DEFAULT_DEVICE {
                                    config.device = Some(cv.clone());
                                }
                            }
                        } else if val != DEFAULT_DEVICE {
                            config.device = Some(val.clone());
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        config
    }

    pub fn is_separator(&self, idx: usize) -> bool {
        matches!(self.items.get(idx), Some(SettingItem::Separator { .. }))
    }

    pub fn move_cursor_down(&mut self) {
        let mut next = self.cursor + 1;
        while next < self.items.len() && self.is_separator(next) {
            next += 1;
        }
        if next < self.items.len() {
            self.cursor = next;
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_info_to_vars_all_fields() {
        let info = MediaInfo {
            resolution: "1080p".into(),
            width: 1920,
            height: 1080,
            codec: "hevc".into(),
            hdr: "HDR10".into(),
            aspect_ratio: "16:9".into(),
            framerate: "23.976".into(),
            bit_depth: "10".into(),
            profile: "Main 10".into(),
            audio: "truehd".into(),
            channels: "7.1".into(),
            audio_lang: "eng".into(),
            bitrate_bps: 22587000,
        };
        let vars = info.to_vars();
        assert_eq!(vars["resolution"], "1080p");
        assert_eq!(vars["width"], "1920");
        assert_eq!(vars["height"], "1080");
        assert_eq!(vars["codec"], "hevc");
        assert_eq!(vars["hdr"], "HDR10");
        assert_eq!(vars["aspect_ratio"], "16:9");
        assert_eq!(vars["framerate"], "23.976");
        assert_eq!(vars["bit_depth"], "10");
        assert_eq!(vars["profile"], "Main 10");
        assert_eq!(vars["audio"], "truehd");
        assert_eq!(vars["channels"], "7.1");
        assert_eq!(vars["audio_lang"], "eng");
    }

    #[test]
    fn test_media_info_default_is_empty() {
        let info = MediaInfo::default();
        let vars = info.to_vars();
        assert_eq!(vars["resolution"], "");
        assert_eq!(vars["codec"], "");
        assert_eq!(vars["hdr"], "");
    }

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
        let eject = state.items.iter().find(|i| matches!(i, SettingItem::Toggle { key, .. } if key == "eject"));
        assert!(matches!(eject, Some(SettingItem::Toggle { value: true, .. })));
        let min_dur = state.items.iter().find(|i| matches!(i, SettingItem::Number { key, .. } if key == "min_duration"));
        assert!(matches!(min_dur, Some(SettingItem::Number { value: 600, .. })));
    }

    #[test]
    fn test_settings_device_with_detected_drives() {
        let config = crate::config::Config::default();
        let drives = vec!["/dev/sr0".to_string(), "/dev/sr1".to_string()];
        let state = SettingsState::from_config_with_drives(&config, &drives);
        let device = state.items.iter().find(|i| matches!(i, SettingItem::Choice { key, .. } if key == "device")).unwrap();
        if let SettingItem::Choice { options, selected, .. } = device {
            assert_eq!(options[0], "auto-detect");
            assert!(options.contains(&"/dev/sr0".to_string()));
            assert!(options.contains(&"/dev/sr1".to_string()));
            assert_eq!(options.last().unwrap(), "Custom...");
            assert_eq!(*selected, 0); // default is auto-detect
        }
    }

    #[test]
    fn test_settings_device_known_drive_selected() {
        let config = crate::config::Config {
            device: Some("/dev/sr1".into()),
            ..Default::default()
        };
        let drives = vec!["/dev/sr0".to_string(), "/dev/sr1".to_string()];
        let state = SettingsState::from_config_with_drives(&config, &drives);
        let device = state.items.iter().find(|i| matches!(i, SettingItem::Choice { key, .. } if key == "device")).unwrap();
        if let SettingItem::Choice { options, selected, .. } = device {
            assert_eq!(options[*selected], "/dev/sr1");
        }
    }

    #[test]
    fn test_settings_device_custom_value() {
        let config = crate::config::Config {
            device: Some("/dev/custom0".into()),
            ..Default::default()
        };
        let drives = vec!["/dev/sr0".to_string()];
        let state = SettingsState::from_config_with_drives(&config, &drives);
        let device = state.items.iter().find(|i| matches!(i, SettingItem::Choice { key, .. } if key == "device")).unwrap();
        if let SettingItem::Choice { options, selected, custom_value, .. } = device {
            assert_eq!(options[*selected], "Custom...");
            assert_eq!(custom_value.as_deref(), Some("/dev/custom0"));
        }
    }

    #[test]
    fn test_settings_device_to_config_detected() {
        let config = crate::config::Config::default();
        let drives = vec!["/dev/sr0".to_string()];
        let mut state = SettingsState::from_config_with_drives(&config, &drives);
        // Select /dev/sr0
        let device_idx = state.items.iter().position(|i| matches!(i, SettingItem::Choice { key, .. } if key == "device")).unwrap();
        if let SettingItem::Choice { selected, .. } = &mut state.items[device_idx] {
            *selected = 1; // /dev/sr0
        }
        let restored = state.to_config();
        assert_eq!(restored.device.as_deref(), Some("/dev/sr0"));
    }

    #[test]
    fn test_settings_device_to_config_custom() {
        let config = crate::config::Config {
            device: Some("/dev/custom0".into()),
            ..Default::default()
        };
        let drives = vec!["/dev/sr0".to_string()];
        let state = SettingsState::from_config_with_drives(&config, &drives);
        let restored = state.to_config();
        assert_eq!(restored.device.as_deref(), Some("/dev/custom0"));
    }

    #[test]
    fn test_env_override_toggle() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        // Simulate BLUBACK_EJECT=true
        assert!(state.apply_env_value("eject", "true"));
        let eject = state.items.iter().find(|i| matches!(i, SettingItem::Toggle { key, .. } if key == "eject")).unwrap();
        assert!(matches!(eject, SettingItem::Toggle { value: true, .. }));
    }

    #[test]
    fn test_env_override_toggle_false() {
        let config = crate::config::Config { max_speed: Some(true), ..Default::default() };
        let mut state = SettingsState::from_config(&config);
        assert!(state.apply_env_value("max_speed", "false"));
        let ms = state.items.iter().find(|i| matches!(i, SettingItem::Toggle { key, .. } if key == "max_speed")).unwrap();
        assert!(matches!(ms, SettingItem::Toggle { value: false, .. }));
    }

    #[test]
    fn test_env_override_number() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        assert!(state.apply_env_value("min_duration", "600"));
        let md = state.items.iter().find(|i| matches!(i, SettingItem::Number { key, .. } if key == "min_duration")).unwrap();
        assert!(matches!(md, SettingItem::Number { value: 600, .. }));
    }

    #[test]
    fn test_env_override_number_invalid() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        assert!(!state.apply_env_value("min_duration", "abc"));
        assert!(!state.apply_env_value("min_duration", "0"));
    }

    #[test]
    fn test_env_override_text() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        assert!(state.apply_env_value("output_dir", "/tmp/rips"));
        let od = state.items.iter().find(|i| matches!(i, SettingItem::Text { key, .. } if key == "output_dir")).unwrap();
        assert!(matches!(od, SettingItem::Text { value, .. } if value == "/tmp/rips"));
    }

    #[test]
    fn test_env_override_preset() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        assert!(state.apply_env_value("preset", "plex"));
        let preset = state.items.iter().find(|i| matches!(i, SettingItem::Choice { key, .. } if key == "preset")).unwrap();
        assert!(matches!(preset, SettingItem::Choice { selected: 2, .. })); // plex is index 2
    }

    #[test]
    fn test_env_override_preset_invalid() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        assert!(!state.apply_env_value("preset", "nonexistent"));
    }

    #[test]
    fn test_settings_cursor_skips_separators_down() {
        let state = SettingsState::from_config(&crate::config::Config::default());
        // First item is a separator (General), cursor should start at first non-separator
        assert!(!state.is_separator(state.cursor));
    }

    #[test]
    fn test_settings_cursor_move_down_skips_separator() {
        let mut state = SettingsState::from_config(&crate::config::Config::default());
        // Move to the last item before a separator, then down should skip it
        // Find "Min Duration" (last in General group), next is Separator(Naming)
        let min_dur_idx = state.items.iter().position(|i| matches!(i, SettingItem::Number { key, .. } if key == "min_duration")).unwrap();
        state.cursor = min_dur_idx;
        state.move_cursor_down();
        // Should have skipped the Naming separator
        assert!(!state.is_separator(state.cursor));
        assert!(state.cursor > min_dur_idx + 1);
    }

    #[test]
    fn test_settings_cursor_move_up_skips_separator() {
        let mut state = SettingsState::from_config(&crate::config::Config::default());
        // Find "Preset" (first in Naming group), going up should skip the Separator
        let preset_idx = state.items.iter().position(|i| matches!(i, SettingItem::Choice { key, .. } if key == "preset")).unwrap();
        state.cursor = preset_idx;
        state.move_cursor_up();
        assert!(!state.is_separator(state.cursor));
        assert!(state.cursor < preset_idx - 1);
    }

    #[test]
    fn test_settings_cursor_stays_at_bounds() {
        let mut state = SettingsState::from_config(&crate::config::Config::default());
        // Move to first non-separator
        let first = state.items.iter().position(|i| !matches!(i, SettingItem::Separator { .. })).unwrap();
        state.cursor = first;
        state.move_cursor_up();
        // Should not go past the first non-separator
        assert_eq!(state.cursor, first);

        // Move to last item
        let last = state.items.len() - 1;
        state.cursor = last;
        state.move_cursor_down();
        assert_eq!(state.cursor, last);
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
}
