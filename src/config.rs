use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_TV_FORMAT: &str = "S{season}E{episode}_{title}.mkv";
pub const DEFAULT_MOVIE_FORMAT: &str = "{title}_({year}).mkv";
pub const DEFAULT_SPECIAL_FORMAT: &str = "{show} S00E{episode} {title}.mkv";

pub const PLEX_TV_FORMAT: &str = "{show}/Season {season}/S{season}E{episode} - {title} [Bluray-{resolution}][{audio} {channels}][{codec}].mkv";
pub const PLEX_MOVIE_FORMAT: &str =
    "{title} ({year})/Movie [Bluray-{resolution}][{audio} {channels}][{codec}].mkv";

pub const JELLYFIN_TV_FORMAT: &str = "{show}/Season {season}/S{season}E{episode} - {title}.mkv";
pub const JELLYFIN_MOVIE_FORMAT: &str = "{title} ({year})/{title} ({year}).mkv";

pub const DEFAULT_OUTPUT_DIR: &str = ".";
pub const DEFAULT_DEVICE: &str = "auto-detect";
pub const DEFAULT_MIN_DURATION: u32 = 900;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    pub tmdb_api_key: Option<String>,
    pub preset: Option<String>,
    pub tv_format: Option<String>,
    pub movie_format: Option<String>,
    pub special_format: Option<String>,
    pub eject: Option<bool>,
    pub max_speed: Option<bool>,
    pub min_duration: Option<u32>,
    pub show_filtered: Option<bool>,
    pub output_dir: Option<String>,
    pub device: Option<String>,
}

fn config_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    home.join(".config").join("bluback")
}

pub fn load_config() -> Config {
    let path = config_dir().join("config.toml");
    if path.exists() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        Config::default()
    }
}

impl Config {
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

    pub fn tmdb_api_key(&self) -> Option<String> {
        if let Some(ref key) = self.tmdb_api_key {
            if !key.is_empty() {
                return Some(key.clone());
            }
        }
        let flat_path = config_dir().join("tmdb_api_key");
        if flat_path.exists() {
            if let Ok(contents) = fs::read_to_string(&flat_path) {
                let trimmed = contents.trim().to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }
        std::env::var("TMDB_API_KEY").ok()
    }

    pub fn resolve_format(
        &self,
        is_movie: bool,
        cli_format: Option<&str>,
        cli_preset: Option<&str>,
    ) -> String {
        if let Some(fmt) = cli_format {
            return fmt.to_string();
        }
        if let Some(preset) = cli_preset {
            return preset_format(preset, is_movie);
        }
        let custom = if is_movie {
            &self.movie_format
        } else {
            &self.tv_format
        };
        if let Some(ref fmt) = custom {
            return fmt.clone();
        }
        if let Some(ref preset) = self.preset {
            return preset_format(preset, is_movie);
        }
        preset_format("default", is_movie)
    }

    pub fn resolve_special_format(&self, cli_format: Option<&str>) -> String {
        if let Some(fmt) = cli_format {
            return fmt.to_string();
        }
        if let Some(ref fmt) = self.special_format {
            return fmt.clone();
        }
        DEFAULT_SPECIAL_FORMAT.to_string()
    }

    pub fn should_eject(&self, cli_eject: Option<bool>) -> bool {
        cli_eject.unwrap_or_else(|| self.eject.unwrap_or(false))
    }

    pub fn should_max_speed(&self, cli_no_max_speed: bool) -> bool {
        if cli_no_max_speed {
            return false;
        }
        self.max_speed.unwrap_or(true)
    }

    pub fn min_duration(&self, cli_min_duration: u32) -> u32 {
        if cli_min_duration != 900 {
            return cli_min_duration; // CLI explicitly set, takes priority
        }
        self.min_duration.unwrap_or(900)
    }

    pub fn show_filtered(&self) -> bool {
        self.show_filtered.unwrap_or(false)
    }
}

fn preset_format(name: &str, is_movie: bool) -> String {
    match (name, is_movie) {
        ("plex", false) => PLEX_TV_FORMAT.to_string(),
        ("plex", true) => PLEX_MOVIE_FORMAT.to_string(),
        ("jellyfin", false) => JELLYFIN_TV_FORMAT.to_string(),
        ("jellyfin", true) => JELLYFIN_MOVIE_FORMAT.to_string(),
        (_, false) => DEFAULT_TV_FORMAT.to_string(),
        (_, true) => DEFAULT_MOVIE_FORMAT.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_config() {
        let toml_str = r#"
            tmdb_api_key = "test123"
            preset = "plex"
            tv_format = "custom/{show}.mkv"
            movie_format = "movies/{title}.mkv"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tmdb_api_key.unwrap(), "test123");
        assert_eq!(config.preset.unwrap(), "plex");
        assert_eq!(config.tv_format.unwrap(), "custom/{show}.mkv");
        assert_eq!(config.movie_format.unwrap(), "movies/{title}.mkv");
    }

    #[test]
    fn test_parse_minimal_config() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.tmdb_api_key.is_none());
        assert!(config.preset.is_none());
    }

    #[test]
    fn test_parse_partial_config() {
        let config: Config = toml::from_str(r#"preset = "jellyfin""#).unwrap();
        assert!(config.tmdb_api_key.is_none());
        assert_eq!(config.preset.unwrap(), "jellyfin");
    }

    #[test]
    fn test_resolve_cli_format_highest_priority() {
        let config = Config {
            preset: Some("plex".into()),
            tv_format: Some("config/{show}.mkv".into()),
            ..Default::default()
        };
        assert_eq!(
            config.resolve_format(false, Some("cli/{title}.mkv"), None),
            "cli/{title}.mkv"
        );
    }

    #[test]
    fn test_resolve_cli_preset_over_config() {
        let config = Config {
            preset: Some("plex".into()),
            ..Default::default()
        };
        assert_eq!(
            config.resolve_format(false, None, Some("jellyfin")),
            JELLYFIN_TV_FORMAT
        );
    }

    #[test]
    fn test_resolve_config_custom_format_over_preset() {
        let config = Config {
            preset: Some("plex".into()),
            tv_format: Some("custom/{show}/{title}.mkv".into()),
            ..Default::default()
        };
        assert_eq!(
            config.resolve_format(false, None, None),
            "custom/{show}/{title}.mkv"
        );
    }

    #[test]
    fn test_resolve_config_preset() {
        let config = Config {
            preset: Some("plex".into()),
            ..Default::default()
        };
        assert_eq!(config.resolve_format(false, None, None), PLEX_TV_FORMAT);
        assert_eq!(config.resolve_format(true, None, None), PLEX_MOVIE_FORMAT);
    }

    #[test]
    fn test_resolve_default_fallback() {
        let config = Config::default();
        assert_eq!(config.resolve_format(false, None, None), DEFAULT_TV_FORMAT);
        assert_eq!(
            config.resolve_format(true, None, None),
            DEFAULT_MOVIE_FORMAT
        );
    }

    #[test]
    fn test_resolve_movie_vs_tv_independent() {
        let config = Config {
            tv_format: Some("tv/{title}.mkv".into()),
            movie_format: Some("movie/{title}.mkv".into()),
            ..Default::default()
        };
        assert_eq!(config.resolve_format(false, None, None), "tv/{title}.mkv");
        assert_eq!(config.resolve_format(true, None, None), "movie/{title}.mkv");
    }

    #[test]
    fn test_unknown_preset_falls_back_to_default() {
        let config = Config {
            preset: Some("nonexistent".into()),
            ..Default::default()
        };
        assert_eq!(config.resolve_format(false, None, None), DEFAULT_TV_FORMAT);
    }

    #[test]
    fn test_parse_eject_true() {
        let config: Config = toml::from_str("eject = true").unwrap();
        assert_eq!(config.eject, Some(true));
    }

    #[test]
    fn test_parse_eject_false() {
        let config: Config = toml::from_str("eject = false").unwrap();
        assert_eq!(config.eject, Some(false));
    }

    #[test]
    fn test_parse_eject_absent() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.eject.is_none());
    }

    #[test]
    fn test_should_eject_cli_true_overrides_config() {
        let config = Config {
            eject: Some(false),
            ..Default::default()
        };
        assert!(config.should_eject(Some(true)));
    }

    #[test]
    fn test_should_eject_cli_false_overrides_config() {
        let config = Config {
            eject: Some(true),
            ..Default::default()
        };
        assert!(!config.should_eject(Some(false)));
    }

    #[test]
    fn test_should_eject_no_cli_uses_config() {
        let config = Config {
            eject: Some(true),
            ..Default::default()
        };
        assert!(config.should_eject(None));
    }

    #[test]
    fn test_should_eject_no_cli_no_config_defaults_false() {
        let config = Config::default();
        assert!(!config.should_eject(None));
    }

    #[test]
    fn test_max_speed_defaults_true() {
        let config = Config::default();
        assert!(config.should_max_speed(false));
    }

    #[test]
    fn test_max_speed_cli_disables() {
        let config = Config {
            max_speed: Some(true),
            ..Default::default()
        };
        assert!(!config.should_max_speed(true));
    }

    #[test]
    fn test_max_speed_config_disables() {
        let config = Config {
            max_speed: Some(false),
            ..Default::default()
        };
        assert!(!config.should_max_speed(false));
    }

    #[test]
    fn test_parse_max_speed() {
        let config: Config = toml::from_str("max_speed = false").unwrap();
        assert_eq!(config.max_speed, Some(false));
    }

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

    #[test]
    fn test_min_duration_default() {
        let config = Config::default();
        assert_eq!(config.min_duration(900), 900);
    }

    #[test]
    fn test_min_duration_config_overrides_default() {
        let config = Config {
            min_duration: Some(600),
            ..Default::default()
        };
        assert_eq!(config.min_duration(900), 600);
    }

    #[test]
    fn test_min_duration_cli_overrides_config() {
        let config = Config {
            min_duration: Some(600),
            ..Default::default()
        };
        assert_eq!(config.min_duration(1200), 1200);
    }

    #[test]
    fn test_parse_min_duration() {
        let config: Config = toml::from_str("min_duration = 600").unwrap();
        assert_eq!(config.min_duration, Some(600));
    }

    #[test]
    fn test_show_filtered_default_false() {
        let config = Config::default();
        assert!(!config.show_filtered());
    }

    #[test]
    fn test_show_filtered_config_true() {
        let config = Config {
            show_filtered: Some(true),
            ..Default::default()
        };
        assert!(config.show_filtered());
    }

    #[test]
    fn test_parse_show_filtered() {
        let config: Config = toml::from_str("show_filtered = true").unwrap();
        assert_eq!(config.show_filtered, Some(true));
    }

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

    #[test]
    fn test_save_default_config_all_commented() {
        let config = Config::default();
        let output = config.to_toml_string();
        for line in output.lines() {
            let trimmed = line.trim();
            assert!(
                trimmed.is_empty() || trimmed.starts_with('#'),
                "Expected comment or blank, got: {}",
                line
            );
        }
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
        assert!(output.contains("eject = true"));
        assert!(output.contains("min_duration = 600"));
        // Make sure modified values don't have # prefix
        for line in output.lines() {
            if line.contains("eject = true") {
                assert!(!line.starts_with('#'), "eject should not be commented");
            }
            if line.contains("min_duration = 600") {
                assert!(!line.starts_with('#'), "min_duration should not be commented");
            }
        }
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
        let reparsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(reparsed.eject, Some(true));
        assert_eq!(reparsed.preset.as_deref(), Some("plex"));
        assert_eq!(reparsed.min_duration, Some(600));
        assert_eq!(reparsed.output_dir.as_deref(), Some("/tmp/rips"));
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
}
