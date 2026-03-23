use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_TV_FORMAT: &str = "S{season}E{episode}_{title}.mkv";
pub const DEFAULT_MOVIE_FORMAT: &str = "{title}_({year}).mkv";
#[allow(dead_code)] // Used by future special episode support
pub const DEFAULT_SPECIAL_FORMAT: &str = "{show} S00E{episode} {title}.mkv";

pub const PLEX_TV_FORMAT: &str = "{show}/Season {season}/S{season}E{episode} - {title} [Bluray-{resolution}][{audio} {channels}][{codec}].mkv";
pub const PLEX_MOVIE_FORMAT: &str =
    "{title} ({year})/Movie [Bluray-{resolution}][{audio} {channels}][{codec}].mkv";

pub const JELLYFIN_TV_FORMAT: &str = "{show}/Season {season}/S{season}E{episode} - {title}.mkv";
pub const JELLYFIN_MOVIE_FORMAT: &str = "{title} ({year})/{title} ({year}).mkv";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    pub tmdb_api_key: Option<String>,
    pub preset: Option<String>,
    pub tv_format: Option<String>,
    pub movie_format: Option<String>,
    #[allow(dead_code)] // Used by future special episode support
    pub special_format: Option<String>,
    pub eject: Option<bool>,
    pub max_speed: Option<bool>,
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

    #[allow(dead_code)] // Used by future special episode support
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

    // TODO(task-2): Uncomment when special_format is implemented
    // #[test]
    // fn test_parse_special_format() {
    //     let config: Config = toml::from_str(r#"special_format = "{show} S00E{episode}.mkv""#).unwrap();
    //     assert_eq!(config.special_format.unwrap(), "{show} S00E{episode}.mkv");
    // }

    // #[test]
    // fn test_resolve_special_format_from_config() {
    //     let config = Config {
    //         special_format: Some("custom/{show} S00E{episode}.mkv".into()),
    //         ..Default::default()
    //     };
    //     assert_eq!(
    //         config.resolve_special_format(None),
    //         "custom/{show} S00E{episode}.mkv"
    //     );
    // }

    // #[test]
    // fn test_resolve_special_format_cli_overrides() {
    //     let config = Config {
    //         special_format: Some("config/{show}.mkv".into()),
    //         ..Default::default()
    //     };
    //     assert_eq!(
    //         config.resolve_special_format(Some("cli/{title}.mkv")),
    //         "cli/{title}.mkv"
    //     );
    // }

    // #[test]
    // fn test_resolve_special_format_default() {
    //     let config = Config::default();
    //     assert_eq!(
    //         config.resolve_special_format(None),
    //         DEFAULT_SPECIAL_FORMAT
    //     );
    // }
}
