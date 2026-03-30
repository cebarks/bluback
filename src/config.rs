use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_TV_FORMAT: &str = "S{season}E{episode}_{title}.mkv";
pub const DEFAULT_MOVIE_FORMAT: &str = "{title}_({year}).mkv";
pub const DEFAULT_SPECIAL_FORMAT: &str = "{show} S{season}SP{episode} {title}.mkv";

pub const PLEX_TV_FORMAT: &str = "{show}/Season {season}/S{season}E{episode} - {title} [Bluray-{resolution}][{audio} {channels}][{codec}].mkv";
pub const PLEX_MOVIE_FORMAT: &str =
    "{title} ({year})/Movie [Bluray-{resolution}][{audio} {channels}][{codec}].mkv";

pub const JELLYFIN_TV_FORMAT: &str = "{show}/Season {season}/S{season}E{episode} - {title}.mkv";
pub const JELLYFIN_MOVIE_FORMAT: &str = "{title} ({year})/{title} ({year}).mkv";

pub const DEFAULT_OUTPUT_DIR: &str = ".";
pub const DEFAULT_DEVICE: &str = "auto-detect";
pub const DEFAULT_MIN_DURATION: u32 = 900;
pub const DEFAULT_RESERVE_INDEX_SPACE: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AacsBackend {
    Auto,
    Libaacs,
    Libmmbd,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MetadataConfig {
    pub enabled: Option<bool>,
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StreamsConfig {
    pub audio_languages: Option<Vec<String>>,
    pub subtitle_languages: Option<Vec<String>>,
    pub prefer_surround: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HookConfig {
    pub command: Option<String>,
    pub on_failure: Option<bool>,
    pub blocking: Option<bool>,
    pub log_output: Option<bool>,
}

impl HookConfig {
    pub fn on_failure(&self) -> bool {
        self.on_failure.unwrap_or(false)
    }

    pub fn blocking(&self) -> bool {
        self.blocking.unwrap_or(true)
    }

    pub fn log_output(&self) -> bool {
        self.log_output.unwrap_or(true)
    }
}

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
    pub stream_selection: Option<String>,
    pub verbose_libbluray: Option<bool>,
    /// KB of void space reserved after MKV header for the seek index (Cues).
    /// Allows the muxer to write Cues at the front of the file for faster seeking.
    /// If the actual Cues are larger, they fall back to EOF (default behavior).
    pub reserve_index_space: Option<u32>,
    pub overwrite: Option<bool>,
    pub verify: Option<bool>,
    pub verify_level: Option<String>,
    pub aacs_backend: Option<String>,
    pub multi_drive: Option<String>,
    pub log_file: Option<bool>,
    pub log_level: Option<String>,
    pub log_dir: Option<String>,
    pub max_log_files: Option<u32>,
    pub metadata: Option<MetadataConfig>,
    pub streams: Option<StreamsConfig>,
    pub post_rip: Option<HookConfig>,
    pub post_session: Option<HookConfig>,
}

fn config_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    home.join(".config").join("bluback")
}

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
        emit_u32(
            &mut out,
            "min_duration",
            self.min_duration,
            DEFAULT_MIN_DURATION,
        );
        out.push('\n');
        emit_str(&mut out, "preset", &self.preset, "");
        emit_str(&mut out, "tv_format", &self.tv_format, DEFAULT_TV_FORMAT);
        emit_str(
            &mut out,
            "movie_format",
            &self.movie_format,
            DEFAULT_MOVIE_FORMAT,
        );
        emit_str(
            &mut out,
            "special_format",
            &self.special_format,
            DEFAULT_SPECIAL_FORMAT,
        );
        emit_bool(&mut out, "show_filtered", self.show_filtered, false);
        emit_bool(&mut out, "overwrite", self.overwrite, false);
        emit_bool(&mut out, "verify", self.verify, false);
        emit_str(&mut out, "verify_level", &self.verify_level, "quick");
        emit_str(&mut out, "stream_selection", &self.stream_selection, "all");
        emit_u32(
            &mut out,
            "reserve_index_space",
            self.reserve_index_space,
            DEFAULT_RESERVE_INDEX_SPACE,
        );
        emit_bool(&mut out, "verbose_libbluray", self.verbose_libbluray, false);
        emit_str(&mut out, "aacs_backend", &self.aacs_backend, "auto");
        emit_str(&mut out, "multi_drive", &self.multi_drive, "auto");
        out.push('\n');
        emit_bool(&mut out, "log_file", self.log_file, true);
        emit_str(&mut out, "log_level", &self.log_level, "warn");
        emit_str(&mut out, "log_dir", &self.log_dir, "");
        emit_u32(&mut out, "max_log_files", self.max_log_files, 10);

        out.push('\n');
        out.push_str("[metadata]\n");
        let meta_enabled = self.metadata.as_ref().and_then(|m| m.enabled);
        emit_bool(&mut out, "enabled", meta_enabled, true);
        if let Some(ref meta) = self.metadata {
            if let Some(ref tags) = meta.tags {
                if !tags.is_empty() {
                    let pairs: Vec<String> = tags
                        .iter()
                        .map(|(k, v)| format!("{} = {:?}", k, v))
                        .collect();
                    out.push_str(&format!("tags = {{ {} }}\n", pairs.join(", ")));
                }
            }
        }
        if self
            .metadata
            .as_ref()
            .and_then(|m| m.tags.as_ref())
            .is_none_or(|t| t.is_empty())
        {
            out.push_str("# tags = { }\n");
        }

        out.push('\n');
        out.push_str("[streams]\n");
        if let Some(ref streams) = self.streams {
            if let Some(ref langs) = streams.audio_languages {
                if !langs.is_empty() {
                    let quoted: Vec<String> = langs.iter().map(|l| format!("{:?}", l)).collect();
                    out.push_str(&format!("audio_languages = [{}]\n", quoted.join(", ")));
                } else {
                    out.push_str("# audio_languages = []\n");
                }
            } else {
                out.push_str("# audio_languages = []\n");
            }
            if let Some(ref langs) = streams.subtitle_languages {
                if !langs.is_empty() {
                    let quoted: Vec<String> = langs.iter().map(|l| format!("{:?}", l)).collect();
                    out.push_str(&format!("subtitle_languages = [{}]\n", quoted.join(", ")));
                } else {
                    out.push_str("# subtitle_languages = []\n");
                }
            } else {
                out.push_str("# subtitle_languages = []\n");
            }
            emit_bool(&mut out, "prefer_surround", streams.prefer_surround, false);
        } else {
            out.push_str("# audio_languages = []\n");
            out.push_str("# subtitle_languages = []\n");
            out.push_str("# prefer_surround = false\n");
        }

        out.push('\n');
        emit_str(&mut out, "tmdb_api_key", &self.tmdb_api_key, "");

        out.push('\n');
        out.push_str("[post_rip]\n");
        let pr = self.post_rip.as_ref();
        emit_str(&mut out, "command", &pr.and_then(|h| h.command.clone()), "");
        emit_bool(&mut out, "on_failure", pr.and_then(|h| h.on_failure), false);
        emit_bool(&mut out, "blocking", pr.and_then(|h| h.blocking), true);
        emit_bool(&mut out, "log_output", pr.and_then(|h| h.log_output), true);

        out.push('\n');
        out.push_str("[post_session]\n");
        let ps = self.post_session.as_ref();
        emit_str(&mut out, "command", &ps.and_then(|h| h.command.clone()), "");
        emit_bool(&mut out, "on_failure", ps.and_then(|h| h.on_failure), false);
        emit_bool(&mut out, "blocking", ps.and_then(|h| h.blocking), true);
        emit_bool(&mut out, "log_output", ps.and_then(|h| h.log_output), true);

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

    pub fn verbose_libbluray(&self) -> bool {
        self.verbose_libbluray.unwrap_or(false)
    }

    pub fn overwrite(&self) -> bool {
        self.overwrite.unwrap_or(false)
    }

    pub fn verify(&self) -> bool {
        self.verify.unwrap_or(false)
    }

    pub fn verify_level(&self) -> &str {
        self.verify_level.as_deref().unwrap_or("quick")
    }

    pub fn reserve_index_space(&self) -> u32 {
        self.reserve_index_space
            .unwrap_or(DEFAULT_RESERVE_INDEX_SPACE)
    }

    pub fn aacs_backend(&self) -> AacsBackend {
        match self.aacs_backend.as_deref() {
            Some("libaacs") => AacsBackend::Libaacs,
            Some("libmmbd") => AacsBackend::Libmmbd,
            _ => AacsBackend::Auto,
        }
    }

    pub fn multi_drive_mode(&self) -> &str {
        self.multi_drive.as_deref().unwrap_or("auto")
    }

    pub fn log_file_enabled(&self) -> bool {
        self.log_file.unwrap_or(true)
    }

    pub fn log_level(&self) -> &str {
        self.log_level.as_deref().unwrap_or("warn")
    }

    pub fn log_dir(&self) -> PathBuf {
        if let Some(ref dir) = self.log_dir {
            return PathBuf::from(dir);
        }
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"));
        home.join(".local")
            .join("share")
            .join("bluback")
            .join("logs")
    }

    pub fn max_log_files(&self) -> u32 {
        self.max_log_files.unwrap_or(10)
    }

    pub fn resolve_stream_filter(&self) -> crate::streams::StreamFilter {
        // New [streams] section takes priority
        if let Some(ref streams) = self.streams {
            return crate::streams::StreamFilter {
                audio_languages: streams.audio_languages.clone().unwrap_or_default(),
                subtitle_languages: streams.subtitle_languages.clone().unwrap_or_default(),
                prefer_surround: streams.prefer_surround.unwrap_or(false),
            };
        }
        // Fall back to old stream_selection key
        match self.stream_selection.as_deref() {
            Some("prefer_surround") => {
                log::warn!(
                    "Config key 'stream_selection' is deprecated, use [streams] section instead"
                );
                crate::streams::StreamFilter {
                    prefer_surround: true,
                    ..Default::default()
                }
            }
            _ => crate::streams::StreamFilter::default(),
        }
    }

    pub fn metadata_enabled(&self) -> bool {
        self.metadata
            .as_ref()
            .and_then(|m| m.enabled)
            .unwrap_or(true)
    }

    pub fn metadata_tags(&self) -> HashMap<String, String> {
        self.metadata
            .as_ref()
            .and_then(|m| m.tags.clone())
            .unwrap_or_default()
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

const KNOWN_KEYS: &[&str] = &[
    "tmdb_api_key",
    "preset",
    "tv_format",
    "movie_format",
    "special_format",
    "eject",
    "max_speed",
    "min_duration",
    "show_filtered",
    "output_dir",
    "device",
    "stream_selection",
    "verbose_libbluray",
    "reserve_index_space",
    "overwrite",
    "verify",
    "verify_level",
    "aacs_backend",
    "multi_drive",
    "log_file",
    "log_level",
    "log_dir",
    "max_log_files",
    "metadata",
    "metadata.enabled",
    "metadata.tags",
    "streams",
    "streams.audio_languages",
    "streams.subtitle_languages",
    "streams.prefer_surround",
    "post_rip",
    "post_rip.command",
    "post_rip.on_failure",
    "post_rip.blocking",
    "post_rip.log_output",
    "post_session",
    "post_session.command",
    "post_session.on_failure",
    "post_session.blocking",
    "post_session.log_output",
];

pub fn validate_raw_toml(raw: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Ok(table) = raw.parse::<toml::Table>() {
        for key in table.keys() {
            if !KNOWN_KEYS.contains(&key.as_str()) {
                warnings.push(format!("unknown config key '{}' (typo?)", key));
            }
        }
    }
    warnings
}

pub fn validate_config(config: &Config) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(d) = config.min_duration {
        if d == 0 {
            warnings.push("min_duration must be > 0".into());
        }
    }
    if let Some(r) = config.reserve_index_space {
        if r > 10000 {
            warnings.push(format!(
                "reserve_index_space = {} KB seems too large (max recommended: 10000 KB)",
                r
            ));
        }
    }
    if let Some(m) = config.max_log_files {
        if m == 0 {
            warnings.push("max_log_files must be > 0".into());
        }
    }
    if let Some(ref level) = config.log_level {
        if !["error", "warn", "info", "debug", "trace"].contains(&level.as_str()) {
            warnings.push(format!(
                "log_level must be error, warn, info, debug, or trace — got \"{}\"",
                level
            ));
        }
    }
    for (name, fmt) in [
        ("tv_format", &config.tv_format),
        ("movie_format", &config.movie_format),
        ("special_format", &config.special_format),
    ] {
        if let Some(ref f) = fmt {
            let opens = f.chars().filter(|&c| c == '{').count();
            let closes = f.chars().filter(|&c| c == '}').count();
            if opens != closes {
                warnings.push(format!("{} has unmatched braces", name));
            }
        }
    }
    if let Some(ref md) = config.multi_drive {
        if md != "auto" && md != "manual" {
            warnings.push(format!(
                "multi_drive must be \"auto\" or \"manual\", got \"{}\"",
                md
            ));
        }
    }
    if let Some(ref level) = config.verify_level {
        if !["quick", "full"].contains(&level.as_str()) {
            warnings.push(format!(
                "verify_level must be \"quick\" or \"full\", got \"{}\"",
                level
            ));
        }
    }
    warnings
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
        let config: Config =
            toml::from_str(r#"special_format = "{show} S{season}SP{episode}.mkv""#).unwrap();
        assert_eq!(
            config.special_format.unwrap(),
            "{show} S{season}SP{episode}.mkv"
        );
    }

    #[test]
    fn test_resolve_special_format_from_config() {
        let config = Config {
            special_format: Some("custom/{show} S{season}SP{episode}.mkv".into()),
            ..Default::default()
        };
        assert_eq!(
            config.resolve_special_format(None),
            "custom/{show} S{season}SP{episode}.mkv"
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
        assert_eq!(config.resolve_special_format(None), DEFAULT_SPECIAL_FORMAT);
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
                trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('['),
                "Expected comment, blank, or section header, got: {}",
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
                assert!(
                    !line.starts_with('#'),
                    "min_duration should not be commented"
                );
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

    #[test]
    fn test_resolve_config_path_default() {
        let path = resolve_config_path(None);
        assert!(path
            .to_string_lossy()
            .ends_with(".config/bluback/config.toml"));
    }

    #[test]
    fn test_resolve_config_path_explicit() {
        let path = resolve_config_path(Some(std::path::PathBuf::from("/tmp/custom.toml")));
        assert_eq!(path, std::path::PathBuf::from("/tmp/custom.toml"));
    }

    #[test]
    fn test_parse_aacs_backend_auto() {
        let config: Config = toml::from_str(r#"aacs_backend = "auto""#).unwrap();
        assert_eq!(config.aacs_backend.as_deref(), Some("auto"));
    }

    #[test]
    fn test_parse_aacs_backend_libmmbd() {
        let config: Config = toml::from_str(r#"aacs_backend = "libmmbd""#).unwrap();
        assert_eq!(config.aacs_backend.as_deref(), Some("libmmbd"));
    }

    #[test]
    fn test_parse_aacs_backend_absent_defaults_auto() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.aacs_backend.is_none());
    }

    #[test]
    fn test_aacs_backend_accessor() {
        let config = Config {
            aacs_backend: Some("libmmbd".into()),
            ..Default::default()
        };
        assert!(matches!(config.aacs_backend(), AacsBackend::Libmmbd));
        let config = Config {
            aacs_backend: Some("libaacs".into()),
            ..Default::default()
        };
        assert!(matches!(config.aacs_backend(), AacsBackend::Libaacs));
        let config = Config::default();
        assert!(matches!(config.aacs_backend(), AacsBackend::Auto));
    }

    #[test]
    fn test_aacs_backend_serialization_roundtrip() {
        let config = Config {
            aacs_backend: Some("libmmbd".into()),
            ..Default::default()
        };
        let toml_str = config.to_toml_string();
        assert!(toml_str.contains(r#"aacs_backend = "libmmbd""#));
        let reparsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(reparsed.aacs_backend.as_deref(), Some("libmmbd"));
    }

    #[test]
    fn test_stream_selection_from_config() {
        let toml_str = r#"
            stream_selection = "prefer_surround"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.stream_selection.as_deref(), Some("prefer_surround"));
    }

    #[test]
    fn test_stream_selection_default_is_none() {
        let toml_str = "";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.stream_selection.is_none());
    }

    #[test]
    fn test_validate_unknown_keys_warns() {
        let raw = r#"eject = true
unknown_key = "foo"
also_unknown = 42"#;
        let warnings = validate_raw_toml(raw);
        assert!(warnings.iter().any(|w| w.contains("unknown_key")));
        assert!(warnings.iter().any(|w| w.contains("also_unknown")));
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn test_validate_known_keys_no_warnings() {
        let raw = r#"eject = true"#;
        let warnings = validate_raw_toml(raw);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_min_duration_zero_warns() {
        let config = Config {
            min_duration: Some(0),
            ..Default::default()
        };
        let warnings = validate_config(&config);
        assert!(warnings.iter().any(|w| w.contains("min_duration")));
    }

    #[test]
    fn test_validate_reserve_index_space_too_large_warns() {
        let config = Config {
            reserve_index_space: Some(50000),
            ..Default::default()
        };
        let warnings = validate_config(&config);
        assert!(warnings.iter().any(|w| w.contains("reserve_index_space")));
    }

    #[test]
    fn test_validate_unmatched_braces_warns() {
        let config = Config {
            tv_format: Some("{show/{title}.mkv".into()),
            ..Default::default()
        };
        let warnings = validate_config(&config);
        assert!(warnings.iter().any(|w| w.contains("tv_format")));
    }

    #[test]
    fn test_parse_overwrite() {
        let config: Config = toml::from_str("overwrite = true").unwrap();
        assert_eq!(config.overwrite, Some(true));
    }

    #[test]
    fn test_overwrite_default_false() {
        let config = Config::default();
        assert!(!config.overwrite());
    }

    #[test]
    fn test_overwrite_config_true() {
        let config = Config {
            overwrite: Some(true),
            ..Default::default()
        };
        assert!(config.overwrite());
    }

    #[test]
    fn test_multi_drive_config_parsing() {
        let toml_str = r#"multi_drive = "manual""#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.multi_drive.as_deref(), Some("manual"));
    }

    #[test]
    fn test_multi_drive_config_default() {
        let config = Config::default();
        assert_eq!(config.multi_drive, None); // None means "auto" (the default)
    }

    #[test]
    fn test_multi_drive_config_validation() {
        let warnings = validate_config(&Config {
            multi_drive: Some("invalid".into()),
            ..Default::default()
        });
        assert!(warnings.iter().any(|w| w.contains("multi_drive")));
    }

    #[test]
    fn test_parse_log_config() {
        let toml_str = r#"
            log_file = false
            log_level = "debug"
            log_dir = "/tmp/bluback-logs"
            max_log_files = 5
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.log_file, Some(false));
        assert_eq!(config.log_level.as_deref(), Some("debug"));
        assert_eq!(config.log_dir.as_deref(), Some("/tmp/bluback-logs"));
        assert_eq!(config.max_log_files, Some(5));
    }

    #[test]
    fn test_log_config_defaults() {
        let config = Config::default();
        assert!(config.log_file_enabled());
        assert_eq!(config.log_level(), "warn");
        assert_eq!(config.max_log_files(), 10);
        assert!(config.log_dir().to_string_lossy().ends_with("bluback/logs"));
    }

    #[test]
    fn test_validate_max_log_files_zero_warns() {
        let config = Config {
            max_log_files: Some(0),
            ..Default::default()
        };
        let warnings = validate_config(&config);
        assert!(warnings.iter().any(|w| w.contains("max_log_files")));
    }

    #[test]
    fn test_validate_invalid_log_level_warns() {
        let config = Config {
            log_level: Some("verbose".into()),
            ..Default::default()
        };
        let warnings = validate_config(&config);
        assert!(warnings.iter().any(|w| w.contains("log_level")));
    }

    #[test]
    fn test_log_config_serialization_roundtrip() {
        let config = Config {
            log_file: Some(false),
            log_level: Some("debug".into()),
            max_log_files: Some(5),
            ..Default::default()
        };
        let toml_str = config.to_toml_string();
        assert!(toml_str.contains("log_file = false"));
        assert!(toml_str.contains(r#"log_level = "debug""#));
        assert!(toml_str.contains("max_log_files = 5"));
        let reparsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(reparsed.log_file, Some(false));
        assert_eq!(reparsed.log_level.as_deref(), Some("debug"));
        assert_eq!(reparsed.max_log_files, Some(5));
    }

    #[test]
    fn test_parse_metadata_section() {
        let toml_str = r#"
            [metadata]
            enabled = false
            tags = { STUDIO = "HBO", COLLECTION = "My Blu-rays" }
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let meta = config.metadata.unwrap();
        assert_eq!(meta.enabled, Some(false));
        assert_eq!(meta.tags.as_ref().unwrap()["STUDIO"], "HBO");
        assert_eq!(meta.tags.as_ref().unwrap()["COLLECTION"], "My Blu-rays");
    }

    #[test]
    fn test_parse_missing_metadata_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.metadata.is_none());
        assert!(config.metadata_enabled());
        assert!(config.metadata_tags().is_empty());
    }

    #[test]
    fn test_metadata_config_roundtrip() {
        let toml_str = r#"
            [metadata]
            enabled = false
            tags = { STUDIO = "HBO" }
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let output = config.to_toml_string();
        assert!(output.contains("[metadata]"));
        assert!(output.contains("enabled = false"));
        let reparsed: Config = toml::from_str(&output).unwrap();
        let meta = reparsed.metadata.unwrap();
        assert_eq!(meta.enabled, Some(false));
    }

    #[test]
    fn test_parse_post_rip_config() {
        let toml_str = r#"
            [post_rip]
            command = "notify-send '{filename}'"
            on_failure = true
            blocking = false
            log_output = false
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let hook = config.post_rip.unwrap();
        assert_eq!(hook.command.as_deref(), Some("notify-send '{filename}'"));
        assert_eq!(hook.on_failure, Some(true));
        assert_eq!(hook.blocking, Some(false));
        assert_eq!(hook.log_output, Some(false));
    }

    #[test]
    fn test_parse_post_session_config() {
        let toml_str = r#"
            [post_session]
            command = "echo done"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let hook = config.post_session.unwrap();
        assert_eq!(hook.command.as_deref(), Some("echo done"));
        assert!(hook.on_failure.is_none());
        assert!(hook.blocking.is_none());
    }

    #[test]
    fn test_parse_missing_hooks_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.post_rip.is_none());
        assert!(config.post_session.is_none());
    }

    #[test]
    fn test_hook_config_accessors() {
        let hook = HookConfig {
            command: Some("echo hi".into()),
            on_failure: None,
            blocking: None,
            log_output: None,
        };
        assert!(!hook.on_failure());
        assert!(hook.blocking());
        assert!(hook.log_output());

        let hook2 = HookConfig {
            command: Some("echo hi".into()),
            on_failure: Some(true),
            blocking: Some(false),
            log_output: Some(false),
        };
        assert!(hook2.on_failure());
        assert!(!hook2.blocking());
        assert!(!hook2.log_output());
    }

    #[test]
    fn test_hook_config_serialization_roundtrip() {
        let config = Config {
            post_rip: Some(HookConfig {
                command: Some("echo '{filename}'".into()),
                on_failure: Some(true),
                blocking: Some(false),
                log_output: Some(false),
            }),
            ..Default::default()
        };
        let toml_str = config.to_toml_string();
        assert!(toml_str.contains("[post_rip]"));
        assert!(toml_str.contains(r#"command = "echo '{filename}'"#));
        assert!(toml_str.contains("on_failure = true"));
        assert!(toml_str.contains("blocking = false"));
        assert!(toml_str.contains("log_output = false"));
        let reparsed: Config = toml::from_str(&toml_str).unwrap();
        let hook = reparsed.post_rip.unwrap();
        assert_eq!(hook.command.as_deref(), Some("echo '{filename}'"));
        assert_eq!(hook.on_failure, Some(true));
    }

    #[test]
    fn test_hook_config_default_serialization_commented() {
        let config = Config::default();
        let toml_str = config.to_toml_string();
        assert!(toml_str.contains("[post_rip]"));
        assert!(toml_str.contains("# command = \"\""));
        assert!(toml_str.contains("[post_session]"));
    }

    #[test]
    fn test_validate_hook_known_keys() {
        let raw = r#"
            [post_rip]
            command = "echo hi"
            on_failure = true
            blocking = false
            log_output = true
        "#;
        let warnings = validate_raw_toml(raw);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_parse_verify_config() {
        let toml_str = r#"
            verify = true
            verify_level = "full"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.verify, Some(true));
        assert_eq!(config.verify_level.as_deref(), Some("full"));
    }

    #[test]
    fn test_verify_config_defaults() {
        let config = Config::default();
        assert!(!config.verify());
        assert_eq!(config.verify_level(), "quick");
    }

    #[test]
    fn test_verify_config_serialization_roundtrip() {
        let config = Config {
            verify: Some(true),
            verify_level: Some("full".into()),
            ..Default::default()
        };
        let toml_str = config.to_toml_string();
        assert!(toml_str.contains("verify = true"));
        assert!(toml_str.contains(r#"verify_level = "full""#));
        let reparsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(reparsed.verify, Some(true));
        assert_eq!(reparsed.verify_level.as_deref(), Some("full"));
    }

    #[test]
    fn test_validate_invalid_verify_level_warns() {
        let config = Config {
            verify_level: Some("deep".into()),
            ..Default::default()
        };
        let warnings = validate_config(&config);
        assert!(warnings.iter().any(|w| w.contains("verify_level")));
    }

    #[test]
    fn test_streams_config_parsing() {
        let toml = r#"
[streams]
audio_languages = ["eng", "jpn"]
subtitle_languages = ["eng"]
prefer_surround = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        let streams = config.streams.unwrap();
        assert_eq!(
            streams.audio_languages,
            Some(vec!["eng".to_string(), "jpn".to_string()])
        );
        assert_eq!(streams.subtitle_languages, Some(vec!["eng".to_string()]));
        assert_eq!(streams.prefer_surround, Some(true));
    }

    #[test]
    fn test_streams_config_empty() {
        let toml = "";
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.streams.is_none());
    }

    #[test]
    fn test_resolve_stream_filter_from_streams() {
        let toml = r#"
[streams]
audio_languages = ["eng"]
prefer_surround = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        let filter = config.resolve_stream_filter();
        assert_eq!(filter.audio_languages, vec!["eng"]);
        assert!(filter.prefer_surround);
        assert!(filter.subtitle_languages.is_empty());
    }

    #[test]
    fn test_resolve_stream_filter_from_old_key() {
        let toml = r#"stream_selection = "prefer_surround""#;
        let config: Config = toml::from_str(toml).unwrap();
        let filter = config.resolve_stream_filter();
        assert!(filter.prefer_surround);
    }

    #[test]
    fn test_resolve_stream_filter_new_overrides_old() {
        let toml = r#"
stream_selection = "prefer_surround"
[streams]
prefer_surround = false
audio_languages = ["fra"]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        let filter = config.resolve_stream_filter();
        assert!(!filter.prefer_surround);
        assert_eq!(filter.audio_languages, vec!["fra"]);
    }

    #[test]
    fn test_streams_config_serialization_roundtrip() {
        let config = Config {
            streams: Some(StreamsConfig {
                audio_languages: Some(vec!["eng".into(), "jpn".into()]),
                subtitle_languages: Some(vec!["eng".into()]),
                prefer_surround: Some(true),
            }),
            ..Default::default()
        };
        let toml_str = config.to_toml_string();
        assert!(toml_str.contains("[streams]"));
        assert!(toml_str.contains(r#"audio_languages = ["eng", "jpn"]"#));
        assert!(toml_str.contains(r#"subtitle_languages = ["eng"]"#));
        assert!(toml_str.contains("prefer_surround = true"));
        let reparsed: Config = toml::from_str(&toml_str).unwrap();
        let streams = reparsed.streams.unwrap();
        assert_eq!(
            streams.audio_languages,
            Some(vec!["eng".to_string(), "jpn".to_string()])
        );
        assert_eq!(streams.subtitle_languages, Some(vec!["eng".to_string()]));
        assert_eq!(streams.prefer_surround, Some(true));
    }

    #[test]
    fn test_streams_config_default_serialization_commented() {
        let config = Config::default();
        let toml_str = config.to_toml_string();
        assert!(toml_str.contains("[streams]"));
        assert!(toml_str.contains("# audio_languages = []"));
        assert!(toml_str.contains("# subtitle_languages = []"));
        assert!(toml_str.contains("# prefer_surround = false"));
    }

    #[test]
    fn test_streams_config_partial_audio_only() {
        let toml = r#"
[streams]
audio_languages = ["eng"]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        let filter = config.resolve_stream_filter();
        assert_eq!(filter.audio_languages, vec!["eng"]);
        assert!(filter.subtitle_languages.is_empty());
        assert!(!filter.prefer_surround);
    }

    #[test]
    fn test_streams_config_partial_prefer_surround_only() {
        let toml = r#"
[streams]
prefer_surround = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        let filter = config.resolve_stream_filter();
        assert!(filter.prefer_surround);
        assert!(filter.audio_languages.is_empty());
    }

    #[test]
    fn test_resolve_stream_filter_old_key_non_prefer_surround() {
        // Old key with value "all" should return default filter
        let toml = r#"stream_selection = "all""#;
        let config: Config = toml::from_str(toml).unwrap();
        let filter = config.resolve_stream_filter();
        assert!(!filter.prefer_surround);
        assert!(filter.audio_languages.is_empty());
    }
}
