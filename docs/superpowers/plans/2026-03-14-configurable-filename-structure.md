# Configurable Filename Structure Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add configurable filename templates with presets, media metadata placeholders, and TOML config file support.

**Architecture:** New `config.rs` module handles TOML config loading and format resolution. New `MediaInfo` struct in `types.rs` holds ffprobe-parsed video/audio metadata. Template rendering in `util.rs` does placeholder substitution with bracket cleanup and path sanitization. Existing `probe_streams` is untouched; a separate `probe_media_info` function uses ffprobe JSON output.

**Tech Stack:** Rust, `toml` crate (new dep), `serde`/`serde_json` (existing), `regex` (existing)

**Spec:** `docs/superpowers/specs/2026-03-14-configurable-filename-structure-design.md`

---

## Chunk 1: Foundation (types, config, template engine)

### Task 1: Add `toml` dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add toml to Cargo.toml**

In `Cargo.toml`, add to `[dependencies]`:

```toml
toml = "0.8"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add toml crate for config file parsing"
```

---

### Task 2: Add `MediaInfo` struct to types.rs

**Files:**
- Modify: `src/types.rs`

- [ ] **Step 1: Write tests for MediaInfo::to_vars**

Add at the bottom of `src/types.rs`:

```rust
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
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib types`
Expected: FAIL — `MediaInfo` doesn't exist yet

- [ ] **Step 3: Add MediaInfo struct and impl**

Add before the `pub type EpisodeAssignments` line in `src/types.rs`:

```rust
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
}

impl MediaInfo {
    pub fn to_vars(&self) -> std::collections::HashMap<&str, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("resolution", self.resolution.clone());
        m.insert("width", if self.width > 0 { self.width.to_string() } else { String::new() });
        m.insert("height", if self.height > 0 { self.height.to_string() } else { String::new() });
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib types`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/types.rs
git commit -m "feat: add MediaInfo struct for ffprobe metadata"
```

---

### Task 3: Create config module with TOML parsing and format resolution

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs` (add `mod config;`)

- [ ] **Step 1: Create src/config.rs with implementation and tests**

Create `src/config.rs`:

```rust
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_TV_FORMAT: &str = "S{season}E{episode}_{title}.mkv";
pub const DEFAULT_MOVIE_FORMAT: &str = "{title}_({year}).mkv";

pub const PLEX_TV_FORMAT: &str = "{show}/Season {season}/S{season}E{episode} - {title} [Bluray-{resolution}][{audio} {channels}][{codec}].mkv";
pub const PLEX_MOVIE_FORMAT: &str = "{title} ({year})/Movie [Bluray-{resolution}][{audio} {channels}][{codec}].mkv";

pub const JELLYFIN_TV_FORMAT: &str = "{show}/Season {season}/S{season}E{episode} - {title}.mkv";
pub const JELLYFIN_MOVIE_FORMAT: &str = "{title} ({year})/{title} ({year}).mkv";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    pub tmdb_api_key: Option<String>,
    pub preset: Option<String>,
    pub tv_format: Option<String>,
    pub movie_format: Option<String>,
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
        // 1. TOML config field
        if let Some(ref key) = self.tmdb_api_key {
            if !key.is_empty() {
                return Some(key.clone());
            }
        }
        // 2. Legacy flat file
        let flat_path = config_dir().join("tmdb_api_key");
        if flat_path.exists() {
            if let Ok(contents) = fs::read_to_string(&flat_path) {
                let trimmed = contents.trim().to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }
        // 3. Environment variable
        std::env::var("TMDB_API_KEY").ok()
    }

    /// Resolve the format template for the given mode.
    /// Priority: cli_format > cli_preset > config tv/movie_format > config preset > default
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
        let custom = if is_movie { &self.movie_format } else { &self.tv_format };
        if let Some(ref fmt) = custom {
            return fmt.clone();
        }
        if let Some(ref preset) = self.preset {
            return preset_format(preset, is_movie);
        }
        preset_format("default", is_movie)
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
        assert!(config.tv_format.is_none());
        assert!(config.movie_format.is_none());
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
        assert_eq!(config.resolve_format(false, Some("cli/{title}.mkv"), None), "cli/{title}.mkv");
    }

    #[test]
    fn test_resolve_cli_preset_over_config() {
        let config = Config {
            preset: Some("plex".into()),
            ..Default::default()
        };
        assert_eq!(config.resolve_format(false, None, Some("jellyfin")), JELLYFIN_TV_FORMAT);
    }

    #[test]
    fn test_resolve_config_custom_format_over_preset() {
        let config = Config {
            preset: Some("plex".into()),
            tv_format: Some("custom/{show}/{title}.mkv".into()),
            ..Default::default()
        };
        assert_eq!(config.resolve_format(false, None, None), "custom/{show}/{title}.mkv");
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
        assert_eq!(config.resolve_format(true, None, None), DEFAULT_MOVIE_FORMAT);
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
}
```

Note on `DEFAULT_MOVIE_FORMAT`: The default movie format does NOT include `_pt{part}` because the part suffix is conditionally added only when `part.is_some()`. The default format codepath uses the legacy `make_movie_filename` hardcoded logic (early return) which handles this correctly. Non-default formats that want part numbering should include `{part}` in their template; if `{part}` is empty, `render_template` will produce an empty string and bracket cleanup will handle any surrounding brackets.

- [ ] **Step 2: Add `mod config;` to main.rs**

In `src/main.rs`, add after the existing module declarations (after `mod util;`):

```rust
mod config;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib config`
Expected: PASS (all 9 tests)

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat: add config module with TOML parsing and format resolution"
```

---

### Task 4: Add template rendering to util.rs

**Files:**
- Modify: `src/util.rs`

- [ ] **Step 1: Write tests for sanitize_path_component and render_template**

Add these tests to the existing `mod tests` block in `src/util.rs`:

```rust
    #[test]
    fn test_sanitize_path_component_preserves_spaces() {
        assert_eq!(sanitize_path_component("Hello World"), "Hello World");
    }

    #[test]
    fn test_sanitize_path_component_strips_unsafe() {
        assert_eq!(sanitize_path_component("foo/bar:baz\"qux"), "foobarbazqux");
    }

    #[test]
    fn test_sanitize_path_component_strips_backslash_and_null() {
        assert_eq!(sanitize_path_component("test\\path\0here"), "testpathhere");
    }

    #[test]
    fn test_sanitize_path_component_strips_dotdot() {
        assert_eq!(sanitize_path_component(".."), "");
    }

    #[test]
    fn test_render_template_basic() {
        let mut vars = HashMap::new();
        vars.insert("show", "Stargate Universe".to_string());
        vars.insert("season", "01".to_string());
        vars.insert("episode", "03".to_string());
        vars.insert("title", "Air (Part 1)".to_string());
        assert_eq!(
            render_template("S{season}E{episode}_{title}.mkv", &vars),
            "S01E03_Air (Part 1).mkv"
        );
    }

    #[test]
    fn test_render_template_with_subdirs() {
        let mut vars = HashMap::new();
        vars.insert("show", "Test Show".to_string());
        vars.insert("season", "02".to_string());
        vars.insert("episode", "05".to_string());
        vars.insert("title", "Ep Name".to_string());
        assert_eq!(
            render_template("{show}/Season {season}/S{season}E{episode} - {title}.mkv", &vars),
            "Test Show/Season 02/S02E05 - Ep Name.mkv"
        );
    }

    #[test]
    fn test_render_template_unknown_placeholder_preserved() {
        let vars = HashMap::new();
        assert_eq!(
            render_template("{foo}_{bar}.mkv", &vars),
            "{foo}_{bar}.mkv"
        );
    }

    #[test]
    fn test_render_template_empty_values_bracket_cleanup() {
        let mut vars = HashMap::new();
        vars.insert("resolution", "1080p".to_string());
        vars.insert("audio", String::new());
        vars.insert("channels", String::new());
        vars.insert("codec", "hevc".to_string());
        assert_eq!(
            render_template("Movie [Bluray-{resolution}][{audio} {channels}][{codec}].mkv", &vars),
            "Movie [Bluray-1080p][hevc].mkv"
        );
    }

    #[test]
    fn test_render_template_all_brackets_empty() {
        let mut vars = HashMap::new();
        vars.insert("resolution", String::new());
        vars.insert("audio", String::new());
        vars.insert("channels", String::new());
        vars.insert("codec", String::new());
        assert_eq!(
            render_template("Movie [Bluray-{resolution}][{audio} {channels}][{codec}].mkv", &vars),
            "Movie.mkv"
        );
    }

    #[test]
    fn test_render_template_unsafe_chars_in_values() {
        let mut vars = HashMap::new();
        vars.insert("title", "Spider-Man: No Way Home".to_string());
        assert_eq!(
            render_template("{title}.mkv", &vars),
            "Spider-Man No Way Home.mkv"
        );
    }

    #[test]
    fn test_render_template_path_traversal_stripped() {
        let mut vars = HashMap::new();
        vars.insert("show", "../../etc".to_string());
        vars.insert("title", "passwd".to_string());
        let result = render_template("{show}/{title}.mkv", &vars);
        assert!(!result.contains(".."));
    }

    #[test]
    fn test_render_template_double_space_cleanup() {
        let mut vars = HashMap::new();
        vars.insert("title", "Test".to_string());
        vars.insert("codec", String::new());
        assert_eq!(
            render_template("{title} [{codec}] end.mkv", &vars),
            "Test end.mkv"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib util`
Expected: FAIL — `sanitize_path_component` and `render_template` don't exist

- [ ] **Step 3: Implement sanitize_path_component and render_template**

Add these functions after `sanitize_filename` in `src/util.rs`:

```rust
const UNSAFE_PATH_CHARS: &[char] = &['/', '<', '>', ':', '"', '|', '?', '*', '\\'];

pub fn sanitize_path_component(name: &str) -> String {
    if name == ".." {
        return String::new();
    }
    name.chars()
        .filter(|c| !UNSAFE_PATH_CHARS.contains(c) && *c != '\0')
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn render_template(template: &str, vars: &HashMap<&str, String>) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    static PLACEHOLDER_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\{([a-z_]+)\}").unwrap());
    static EMPTY_BRACKET_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[[^\[\]]*\]").unwrap());
    static MULTI_SPACE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r" {2,}").unwrap());

    // 1. Substitute placeholders
    let result = PLACEHOLDER_RE.replace_all(template, |caps: &regex::Captures| {
        let key = &caps[1];
        match vars.get(key) {
            Some(val) => val.clone(),
            None => caps[0].to_string(),
        }
    });
    let mut result = result.to_string();

    // 2. Bracket cleanup: remove bracket groups whose contents are empty/whitespace/hyphens
    // TODO(debt): This hardcodes "Bluray-" as a known filler prefix. A more general approach
    // would track which bracket groups contained placeholders and whether any resolved non-empty.
    // Current approach works for all built-in presets; custom templates with other prefixes
    // inside brackets (e.g., "[MyPrefix-{codec}]") would leave "[MyPrefix-]" when codec is empty.
    loop {
        let cleaned = EMPTY_BRACKET_RE.replace_all(&result, |caps: &regex::Captures| {
            let full = &caps[0];
            let content = &full[1..full.len() - 1];
            let stripped = content.replace("Bluray-", "").replace("Bluray", "");
            if stripped.trim().is_empty()
                || stripped.trim_matches(|c: char| c == '-' || c == ' ').is_empty()
            {
                String::new()
            } else {
                full.to_string()
            }
        });
        if cleaned == result {
            break;
        }
        result = cleaned.to_string();
    }

    // 3. Clean up double spaces
    result = MULTI_SPACE_RE.replace_all(&result, " ").to_string();

    // 4. Sanitize per path component (preserve /)
    result = result
        .split('/')
        .map(|component| sanitize_path_component(component))
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("/");

    result
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib util`
Expected: PASS (all existing + new tests)

- [ ] **Step 5: Commit**

```bash
git add src/util.rs
git commit -m "feat: add template rendering with bracket cleanup and path sanitization"
```

---

### Task 5: Refactor make_filename and make_movie_filename to use templates

This task changes both functions to accept format template, media info, AND extra vars parameters all at once. All callers are updated in a single pass to avoid signature breakage.

**Files:**
- Modify: `src/util.rs`
- Modify: `src/cli.rs`
- Modify: `src/tui/wizard.rs`

- [ ] **Step 1: Update tests for new signatures**

Replace the existing `make_filename` and `make_movie_filename` tests in `src/util.rs`:

```rust
    #[test]
    fn test_movie_filename_basic() {
        assert_eq!(
            make_movie_filename("The Matrix", "1999", None, None, None, None),
            "The_Matrix_(1999).mkv"
        );
    }

    #[test]
    fn test_movie_filename_no_year() {
        assert_eq!(make_movie_filename("Inception", "", None, None, None, None), "Inception.mkv");
    }

    #[test]
    fn test_movie_filename_with_part() {
        assert_eq!(
            make_movie_filename("Dune", "2021", Some(1), None, None, None),
            "Dune_(2021)_pt1.mkv"
        );
    }

    #[test]
    fn test_movie_filename_special_chars() {
        assert_eq!(
            make_movie_filename("Spider-Man: No Way Home", "2021", None, None, None, None),
            "Spider-Man_No_Way_Home_(2021).mkv"
        );
    }

    #[test]
    fn test_make_filename_with_episode() {
        let ep = Episode { episode_number: 3, name: "The Pilot".into(), runtime: Some(44) };
        assert_eq!(make_filename("00001", Some(&ep), 1, None, None, None), "S01E03_The_Pilot.mkv");
    }

    #[test]
    fn test_make_filename_no_episode() {
        assert_eq!(make_filename("00042", None, 1, None, None, None), "playlist00042.mkv");
    }

    #[test]
    fn test_make_filename_custom_format_with_show() {
        let ep = Episode { episode_number: 3, name: "The Pilot".into(), runtime: Some(44) };
        let mut extra = HashMap::new();
        extra.insert("show", "Test Show".to_string());
        assert_eq!(
            make_filename("00001", Some(&ep), 1, Some("{show}/S{season}E{episode} - {title}.mkv"), None, Some(&extra)),
            "Test Show/S01E03 - The Pilot.mkv"
        );
    }

    #[test]
    fn test_movie_filename_plex_format() {
        use crate::types::MediaInfo;
        let media = MediaInfo {
            resolution: "1080p".into(),
            codec: "hevc".into(),
            audio: "truehd".into(),
            channels: "7.1".into(),
            ..Default::default()
        };
        assert_eq!(
            make_movie_filename(
                "The Matrix", "1999", None,
                Some("{title} ({year})/Movie [Bluray-{resolution}][{audio} {channels}][{codec}].mkv"),
                Some(&media),
                None,
            ),
            "The Matrix (1999)/Movie [Bluray-1080p][truehd 7.1][hevc].mkv"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib util`
Expected: FAIL — signatures don't match

- [ ] **Step 3: Refactor make_filename and make_movie_filename**

Replace the existing functions in `src/util.rs`:

```rust
use crate::types::{Episode, MediaInfo};

pub fn make_movie_filename(
    title: &str,
    year: &str,
    part: Option<u32>,
    format: Option<&str>,
    media_info: Option<&MediaInfo>,
    extra_vars: Option<&HashMap<&str, String>>,
) -> String {
    // Default format: use legacy sanitize_filename (underscores) for backwards compat
    if format.is_none() {
        let name = sanitize_filename(title);
        let year_suffix = if year.is_empty() {
            String::new()
        } else {
            format!("_({})", year)
        };
        let part_suffix = part.map(|p| format!("_pt{}", p)).unwrap_or_default();
        return format!("{}{}{}.mkv", name, year_suffix, part_suffix);
    }

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("title", title.to_string());
    vars.insert("year", year.to_string());
    vars.insert("part", part.map(|p| p.to_string()).unwrap_or_default());
    // {playlist} is populated via extra_vars by callers who have playlist_num

    if let Some(info) = media_info {
        vars.extend(info.to_vars());
    }
    if let Some(extra) = extra_vars {
        for (k, v) in extra {
            vars.insert(k, v.clone());
        }
    }

    render_template(format.unwrap(), &vars)
}

pub fn make_filename(
    playlist_num: &str,
    episode: Option<&Episode>,
    season: u32,
    format: Option<&str>,
    media_info: Option<&MediaInfo>,
    extra_vars: Option<&HashMap<&str, String>>,
) -> String {
    if episode.is_none() {
        return format!("playlist{}.mkv", playlist_num);
    }
    let ep = episode.unwrap();

    // Default format: use legacy sanitize_filename (underscores) for backwards compat
    if format.is_none() {
        return format!(
            "S{:02}E{:02}_{}.mkv",
            season,
            ep.episode_number,
            sanitize_filename(&ep.name)
        );
    }

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("season", format!("{:02}", season));
    vars.insert("episode", format!("{:02}", ep.episode_number));
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

    render_template(format.unwrap(), &vars)
}
```

Update the import at the top of `src/util.rs` — change:
```rust
use crate::types::{Episode, Playlist};
```
to:
```rust
use crate::types::{Episode, MediaInfo};
```

(`Playlist` is used by `assign_episodes` but only needs the struct, which is already available via the function parameter type. Check if `Playlist` is actually needed in the import — if `assign_episodes` takes `&[Playlist]`, the import IS needed. Keep it if so: `use crate::types::{Episode, MediaInfo, Playlist};`)

- [ ] **Step 4: Update all callers to use new 6-parameter signatures**

In `src/cli.rs` line 153, change:
```rust
util::make_movie_filename(title, year, part)
```
to:
```rust
util::make_movie_filename(title, year, part, None, None, None)
```

In `src/cli.rs` line 155, change:
```rust
util::make_filename(&pl.num, episode_assignments.get(&pl.num), season_num.unwrap_or(0))
```
to:
```rust
util::make_filename(&pl.num, episode_assignments.get(&pl.num), season_num.unwrap_or(0), None, None, None)
```

In `src/tui/wizard.rs` line 23, change:
```rust
make_movie_filename(title, year, part)
```
to:
```rust
make_movie_filename(title, year, part, None, None, None)
```

In `src/tui/wizard.rs` lines 25-29, change:
```rust
make_filename(
    &pl.num,
    app.episode_assignments.get(&pl.num),
    app.season_num.unwrap_or(0),
)
```
to:
```rust
make_filename(
    &pl.num,
    app.episode_assignments.get(&pl.num),
    app.season_num.unwrap_or(0),
    None,
    None,
    None,
)
```

- [ ] **Step 5: Verify full compilation and all tests pass**

Run: `cargo test`
Expected: PASS (all tests, no regressions)

- [ ] **Step 6: Commit**

```bash
git add src/util.rs src/cli.rs src/tui/wizard.rs
git commit -m "refactor: make_filename and make_movie_filename accept format, media_info, extra_vars"
```

---

## Chunk 2: ffprobe media info parsing

### Task 6: Add probe_media_info to disc.rs

**Files:**
- Modify: `src/disc.rs`

- [ ] **Step 1: Write tests for parse_media_info_json**

Add to the `mod tests` block in `src/disc.rs`:

```rust
    #[test]
    fn test_parse_media_info_1080p_hevc_truehd() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "hevc",
                    "width": 1920,
                    "height": 1080,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "24000/1001",
                    "bits_per_raw_sample": "10",
                    "profile": "Main 10",
                    "color_transfer": "smpte2084",
                    "side_data_list": []
                },
                {
                    "codec_type": "audio",
                    "codec_name": "truehd",
                    "channel_layout": "7.1",
                    "channels": 8,
                    "tags": { "language": "eng" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.resolution, "1080p");
        assert_eq!(info.width, 1920);
        assert_eq!(info.height, 1080);
        assert_eq!(info.codec, "hevc");
        assert_eq!(info.hdr, "HDR10");
        assert_eq!(info.aspect_ratio, "16:9");
        assert_eq!(info.framerate, "23.976");
        assert_eq!(info.bit_depth, "10");
        assert_eq!(info.profile, "Main 10");
        assert_eq!(info.audio, "truehd");
        assert_eq!(info.channels, "7.1");
        assert_eq!(info.audio_lang, "eng");
    }

    #[test]
    fn test_parse_media_info_sdr() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": 1920,
                    "height": 1080,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "24/1",
                    "bits_per_raw_sample": "8",
                    "profile": "High"
                },
                {
                    "codec_type": "audio",
                    "codec_name": "ac3",
                    "channel_layout": "5.1(side)",
                    "channels": 6,
                    "tags": { "language": "eng" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.codec, "h264");
        assert_eq!(info.hdr, "SDR");
        assert_eq!(info.channels, "5.1");
        assert_eq!(info.framerate, "24.000");
    }

    #[test]
    fn test_parse_media_info_dolby_vision() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "hevc",
                    "width": 3840,
                    "height": 2160,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "24000/1001",
                    "bits_per_raw_sample": "10",
                    "profile": "Main 10",
                    "color_transfer": "smpte2084",
                    "side_data_list": [
                        { "side_data_type": "DOVI configuration record" }
                    ]
                },
                {
                    "codec_type": "audio",
                    "codec_name": "truehd",
                    "channel_layout": "7.1",
                    "channels": 8,
                    "tags": { "language": "eng" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.resolution, "2160p");
        assert_eq!(info.hdr, "DV");
    }

    #[test]
    fn test_parse_media_info_hlg() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "hevc",
                    "width": 3840,
                    "height": 2160,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "50/1",
                    "bits_per_raw_sample": "10",
                    "profile": "Main 10",
                    "color_transfer": "arib-std-b67"
                },
                {
                    "codec_type": "audio",
                    "codec_name": "aac",
                    "channel_layout": "stereo",
                    "channels": 2,
                    "tags": { "language": "jpn" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.hdr, "HLG");
        assert_eq!(info.channels, "2.0");
        assert_eq!(info.audio_lang, "jpn");
    }

    #[test]
    fn test_parse_media_info_hdr10plus() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "hevc",
                    "width": 3840,
                    "height": 2160,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "24000/1001",
                    "bits_per_raw_sample": "10",
                    "profile": "Main 10",
                    "color_transfer": "smpte2084",
                    "side_data_list": [
                        { "side_data_type": "HDR Dynamic Metadata SMPTE2094-40" }
                    ]
                },
                {
                    "codec_type": "audio",
                    "codec_name": "eac3",
                    "channel_layout": "5.1(side)",
                    "channels": 6,
                    "tags": { "language": "eng" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.hdr, "HDR10+");
    }

    #[test]
    fn test_parse_media_info_no_streams() {
        let json = serde_json::json!({ "streams": [] });
        assert!(parse_media_info_json(&json).is_none());
    }

    #[test]
    fn test_parse_media_info_dts_hd_ma() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": 1920,
                    "height": 1080,
                    "display_aspect_ratio": "16:9",
                    "r_frame_rate": "24/1",
                    "bits_per_raw_sample": "8",
                    "profile": "High"
                },
                {
                    "codec_type": "audio",
                    "codec_name": "dts",
                    "profile": "DTS-HD MA",
                    "channel_layout": "5.1(side)",
                    "channels": 6,
                    "tags": { "language": "eng" }
                }
            ]
        });
        let info = parse_media_info_json(&json).unwrap();
        assert_eq!(info.audio, "dts-hd ma");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib disc`
Expected: FAIL — `parse_media_info_json` doesn't exist

- [ ] **Step 3: Implement parse_media_info_json and probe_media_info**

Add these imports at the top of `src/disc.rs`:

```rust
use crate::types::MediaInfo;
```

Add after the existing `probe_streams` function:

```rust
pub fn parse_media_info_json(json: &serde_json::Value) -> Option<MediaInfo> {
    let streams = json.get("streams")?.as_array()?;

    let video = streams.iter().find(|s| {
        s.get("codec_type").and_then(|v| v.as_str()) == Some("video")
    })?;

    let audio = streams.iter().find(|s| {
        s.get("codec_type").and_then(|v| v.as_str()) == Some("audio")
    });

    let width = video.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let height = video.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let resolution = if height > 0 { format!("{}p", height) } else { String::new() };

    let codec = video.get("codec_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let aspect_ratio = video.get("display_aspect_ratio").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let bit_depth = video.get("bits_per_raw_sample").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let profile_str = video.get("profile").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let framerate = video.get("r_frame_rate")
        .and_then(|v| v.as_str())
        .map(|fr| {
            if let Some((num, den)) = fr.split_once('/') {
                let n: f64 = num.parse().unwrap_or(0.0);
                let d: f64 = den.parse().unwrap_or(1.0);
                if d > 0.0 { format!("{:.3}", n / d) } else { fr.to_string() }
            } else {
                fr.to_string()
            }
        })
        .unwrap_or_default();

    // HDR detection
    let color_transfer = video.get("color_transfer").and_then(|v| v.as_str()).unwrap_or("");
    let side_data = video.get("side_data_list").and_then(|v| v.as_array());

    let has_dovi = side_data.map(|sd| {
        sd.iter().any(|entry| {
            entry.get("side_data_type").and_then(|v| v.as_str()) == Some("DOVI configuration record")
        })
    }).unwrap_or(false);

    let has_hdr10plus = side_data.map(|sd| {
        sd.iter().any(|entry| {
            entry.get("side_data_type").and_then(|v| v.as_str()) == Some("HDR Dynamic Metadata SMPTE2094-40")
        })
    }).unwrap_or(false);

    let hdr = if color_transfer == "smpte2084" {
        if has_dovi {
            "DV".to_string()
        } else if has_hdr10plus {
            "HDR10+".to_string()
        } else {
            "HDR10".to_string()
        }
    } else if color_transfer == "arib-std-b67" {
        "HLG".to_string()
    } else {
        "SDR".to_string()
    };

    // Audio info from first audio stream
    let (audio_codec, audio_channels, audio_lang) = if let Some(a) = audio {
        let codec_name = a.get("codec_name").and_then(|v| v.as_str()).unwrap_or("");
        let audio_profile = a.get("profile").and_then(|v| v.as_str()).unwrap_or("");

        let audio_str = if !audio_profile.is_empty() && codec_name == "dts" {
            audio_profile.to_lowercase()
        } else {
            codec_name.to_string()
        };

        let channels = a.get("channel_layout").and_then(|v| v.as_str()).unwrap_or("");
        let channel_count = a.get("channels").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let ch_str = if !channels.is_empty() {
            if channels.starts_with("stereo") {
                "2.0".to_string()
            } else if channels.starts_with("mono") {
                "1.0".to_string()
            } else {
                channels.split('(').next().unwrap_or(channels).to_string()
            }
        } else {
            match channel_count {
                1 => "1.0".to_string(),
                2 => "2.0".to_string(),
                6 => "5.1".to_string(),
                8 => "7.1".to_string(),
                n if n > 0 => format!("{}", n),
                _ => String::new(),
            }
        };

        let lang = a.get("tags")
            .and_then(|t| t.get("language"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        (audio_str, ch_str, lang)
    } else {
        (String::new(), String::new(), String::new())
    };

    Some(MediaInfo {
        resolution,
        width,
        height,
        codec,
        hdr,
        aspect_ratio,
        framerate,
        bit_depth,
        profile: profile_str,
        audio: audio_codec,
        channels: audio_channels,
        audio_lang,
    })
}

pub fn probe_media_info(device: &str, playlist_num: &str) -> Option<MediaInfo> {
    let output = Command::new("ffprobe")
        .args([
            "-playlist", playlist_num,
            "-print_format", "json",
            "-show_streams",
            "-loglevel", "quiet",
            "-i", &format!("bluray:{}", device),
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    parse_media_info_json(&json)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib disc`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/disc.rs
git commit -m "feat: add probe_media_info for ffprobe JSON metadata extraction"
```

---

## Chunk 3: CLI integration (args, config threading, tmdb migration)

### Task 7: Add CLI flags and load config in main.rs

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add --format and --format-preset to Args, load config**

In `src/main.rs`, add to the `Args` struct:

```rust
    /// Custom filename template
    #[arg(long, group = "format_group")]
    format: Option<String>,

    /// Use a built-in filename preset (default, plex, jellyfin)
    #[arg(long, group = "format_group")]
    format_preset: Option<String>,
```

Update the `main()` function:

```rust
fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    disc::check_dependencies()?;

    let config = config::load_config();
    let use_tui = !args.no_tui && atty_stdout();

    if use_tui {
        tui::run(&args, &config)
    } else {
        cli::run(&args, &config)
    }
}
```

- [ ] **Step 2: This won't compile yet** — `cli::run` and `tui::run` don't accept `&Config`. That's fine; we fix them in the next tasks. Just commit main.rs.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: add --format and --format-preset CLI flags, load config"
```

---

### Task 8: Update tmdb.rs to use Config, update cli.rs and tui/mod.rs signatures

**Files:**
- Modify: `src/tmdb.rs`
- Modify: `src/cli.rs`
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Update tmdb.rs get_api_key**

In `src/tmdb.rs`, replace `get_api_key`:

```rust
use crate::config::Config;

pub fn get_api_key(config: &Config) -> Option<String> {
    config.tmdb_api_key()
}
```

Keep `config_path()` and `save_api_key()` unchanged (save still writes to flat file).

- [ ] **Step 2: Update cli::run signature and get_api_key call**

In `src/cli.rs`, update the function signature:

```rust
pub fn run(args: &Args, config: &crate::config::Config) -> anyhow::Result<()> {
```

Update line 52:
```rust
let mut api_key = tmdb::get_api_key(config);
```

- [ ] **Step 3: Update tui::run and run_app signatures, pass config through**

In `src/tui/mod.rs`:

Update `run`:
```rust
pub fn run(args: &Args, config: &crate::config::Config) -> Result<()> {
    // ... existing setup ...
    let result = run_app(&mut terminal, args, config);
    // ... existing cleanup ...
}
```

Update `run_app`:
```rust
fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, args: &Args, config: &crate::config::Config) -> Result<()> {
```

Update `get_api_key` call inside `run_app`:
```rust
app.api_key = crate::tmdb::get_api_key(config);
```

- [ ] **Step 4: Verify compilation and tests**

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/tmdb.rs src/cli.rs src/tui/mod.rs
git commit -m "refactor: migrate tmdb API key lookup to Config, update runner signatures"
```

---

## Chunk 4: Wire format templates through CLI and TUI

### Task 9: Wire format + media info through CLI mode

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Update prompt_tmdb to return show name**

In `src/cli.rs`, change `prompt_tmdb` return type from:
```rust
fn prompt_tmdb(...) -> anyhow::Result<Option<(Vec<Episode>, u64, u32)>> {
```
to:
```rust
fn prompt_tmdb(...) -> anyhow::Result<Option<(Vec<Episode>, u64, u32, String)>> {
```

At the end of `prompt_tmdb`, change the return from:
```rust
Ok(Some((episodes, show_id, season_num)))
```
to:
```rust
Ok(Some((episodes, show_id, season_num, show.name.clone())))
```

Update the caller at the `if let Some(...)` destructure (around line 75):
```rust
if let Some((episodes, _show_id, sn, tmdb_show_name)) = prompt_tmdb(key, default_query, cli_season)? {
```

Store `tmdb_show_name` in a local variable accessible to the filename generation below.

- [ ] **Step 2: Add format resolution and extra_vars to filename generation**

Replace the `default_names` generation block (around line 149) with:

```rust
    // Resolve filename format
    let format_template = config.resolve_format(
        movie_mode,
        args.format.as_deref(),
        args.format_preset.as_deref(),
    );
    let use_custom_format = args.format.is_some()
        || args.format_preset.is_some()
        || config.tv_format.is_some()
        || config.movie_format.is_some()
        || config.preset.is_some();

    // Build extra vars for {show}, {disc}, {label}
    let show_name_str = if movie_mode {
        movie_title.as_ref().map(|(t, _)| t.clone()).unwrap_or_else(|| "Unknown".to_string())
    } else {
        // Prefer TMDb show name, fallback to label, then "Unknown"
        // tmdb_show_name was captured from prompt_tmdb above; default to label/Unknown
        tmdb_show_name_opt.clone().unwrap_or_else(|| {
            label_info.as_ref().map(|l| l.show.clone()).unwrap_or_else(|| "Unknown".to_string())
        })
    };

    let default_names: Vec<String> = selected.iter().enumerate().map(|(sel_i, &idx)| {
        let pl = episodes_pl[idx];

        let media_info = if use_custom_format {
            disc::probe_media_info(&device, &pl.num)
        } else {
            None
        };

        let fmt = if use_custom_format { Some(format_template.as_str()) } else { None };

        // Build extra vars per playlist (playlist num varies)
        let mut extra_vars: HashMap<&str, String> = HashMap::new();
        extra_vars.insert("show", show_name_str.clone());
        extra_vars.insert("disc", label_info.as_ref().map(|l| l.disc.to_string()).unwrap_or_default());
        extra_vars.insert("label", label.clone());
        extra_vars.insert("playlist", pl.num.clone());

        if let Some((ref title, ref year)) = movie_title {
            let part = if selected.len() > 1 { Some(sel_i as u32 + 1) } else { None };
            util::make_movie_filename(title, year, part, fmt, media_info.as_ref(), Some(&extra_vars))
        } else {
            util::make_filename(
                &pl.num,
                episode_assignments.get(&pl.num),
                season_num.unwrap_or(0),
                fmt,
                media_info.as_ref(),
                Some(&extra_vars),
            )
        }
    }).collect();
```

Note: `tmdb_show_name_opt` is an `Option<String>` that should be declared at the top of `run()` as `let mut tmdb_show_name_opt: Option<String> = None;` and set to `Some(tmdb_show_name)` inside the `prompt_tmdb` success branch.

- [ ] **Step 3: Add create_dir_all for subdirectory templates**

Replace the existing `std::fs::create_dir_all(&args.output)?;` (line 201) with directory creation per output file:

```rust
    // Create output directory and any template subdirectories
    for outfile in &outfiles {
        if let Some(parent) = outfile.parent() {
            std::fs::create_dir_all(parent)?;
        }
    }
```

- [ ] **Step 4: Verify compilation and tests**

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat: wire format templates and media info through CLI mode"
```

---

### Task 10: Wire format + media info through TUI mode

**Files:**
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/wizard.rs`
- Modify: `src/tui/dashboard.rs`

- [ ] **Step 1: Add config and show_name to App state**

In `src/tui/mod.rs`, add to the `App` struct:

```rust
    pub config: crate::config::Config,
    pub show_name: String,
```

Initialize in `App::new`:
```rust
    config: crate::config::Config::default(),
    show_name: String::new(),
```

In `run_app`, after creating the app, set the config:
```rust
    app.config = config.clone();
```

- [ ] **Step 2: Store show name when selected in wizard**

In `src/tui/wizard.rs`, in `handle_show_select_input`, inside the `KeyCode::Enter` handler:

For TV mode (the `else` branch that sets `app.selected_show`), add after `app.selected_show = Some(app.list_cursor);`:
```rust
    app.show_name = show.name.clone();
```

For movie mode, add after `app.selected_movie = Some(app.list_cursor);`:
```rust
    app.show_name = app.movie_results[app.list_cursor].title.clone();
```

- [ ] **Step 3: Update playlist_filename to use config and extra_vars**

Replace the `playlist_filename` function in `src/tui/wizard.rs`:

```rust
fn playlist_filename(app: &App, playlist_index: usize, media_info: Option<&crate::types::MediaInfo>) -> String {
    let pl = &app.episodes_pl[playlist_index];

    let format_template = app.config.resolve_format(
        app.movie_mode,
        app.args.format.as_deref(),
        app.args.format_preset.as_deref(),
    );
    let use_custom = app.args.format.is_some()
        || app.args.format_preset.is_some()
        || app.config.tv_format.is_some()
        || app.config.movie_format.is_some()
        || app.config.preset.is_some();
    let fmt = if use_custom { Some(format_template.as_str()) } else { None };

    // Build extra vars
    let mut extra: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    let show_name = if !app.show_name.is_empty() {
        app.show_name.clone()
    } else {
        app.label_info.as_ref().map(|l| l.show.clone()).unwrap_or_else(|| "Unknown".to_string())
    };
    extra.insert("show", show_name);
    extra.insert("disc", app.label_info.as_ref().map(|l| l.disc.to_string()).unwrap_or_default());
    extra.insert("label", app.label.clone());
    extra.insert("playlist", pl.num.clone());

    if app.movie_mode {
        let movie = app.selected_movie.and_then(|i| app.movie_results.get(i));
        let title = movie.map(|m| m.title.as_str()).unwrap_or("movie");
        let year = movie
            .and_then(|m| m.release_date.as_deref())
            .and_then(|d| d.get(..4))
            .unwrap_or("");
        let part = if app.episodes_pl.len() > 1 {
            Some(playlist_index as u32 + 1)
        } else {
            None
        };
        make_movie_filename(title, year, part, fmt, media_info, Some(&extra))
    } else {
        make_filename(
            &pl.num,
            app.episode_assignments.get(&pl.num),
            app.season_num.unwrap_or(0),
            fmt,
            media_info,
            Some(&extra),
        )
    }
}
```

- [ ] **Step 4: Update all playlist_filename call sites**

In the playlist select render (line 501), pass `None` for media_info (preview only):
```rust
let filename = playlist_filename(app, i, None);
```

In the `handle_playlist_select_input` Enter handler, probe media info per playlist and pass it:
```rust
KeyCode::Enter => {
    app.filenames.clear();
    let device = app.args.device.to_string_lossy().to_string();
    for (i, _pl) in app.episodes_pl.iter().enumerate() {
        if !app.playlist_selected.get(i).copied().unwrap_or(false) {
            continue;
        }
        let media_info = crate::disc::probe_media_info(&device, &app.episodes_pl[i].num);
        app.filenames.push(playlist_filename(app, i, media_info.as_ref()));
    }
    // ... rest unchanged
}
```

- [ ] **Step 5: Update dashboard.rs create_dir_all for subdirectory templates**

In `src/tui/dashboard.rs`, in the `tick` function, replace:
```rust
std::fs::create_dir_all(&app.args.output).ok();
```
with:
```rust
if let Some(parent) = outfile.parent() {
    std::fs::create_dir_all(parent).ok();
}
```

(Move the `let outfile = ...` line before this call so `outfile` is in scope.)

Also update the file-exists check and the Done handler's `outfile` construction to use the same full path.

- [ ] **Step 6: Verify compilation and all tests**

Run: `cargo test`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/tui/mod.rs src/tui/wizard.rs src/tui/dashboard.rs
git commit -m "feat: wire format templates through TUI wizard and dashboard"
```

---

## Chunk 5: Polish and final verification

### Task 11: Update sanitize_filename to strip backslash and null byte

**Files:**
- Modify: `src/util.rs`

- [ ] **Step 1: Add test**

```rust
    #[test]
    fn test_sanitize_backslash_and_null() {
        assert_eq!(sanitize_filename("test\\path\0here"), "testpathhere");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib util::tests::test_sanitize_backslash_and_null`
Expected: FAIL

- [ ] **Step 3: Update sanitize_filename to use the same UNSAFE_PATH_CHARS constant**

Replace the `sanitize_filename` function body:

```rust
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !UNSAFE_PATH_CHARS.contains(c) && *c != '\0')
        .collect();
    cleaned.replace(' ', "_")
}
```

This reuses the `UNSAFE_PATH_CHARS` constant defined in Task 4. The only difference from `sanitize_path_component` is that `sanitize_filename` also replaces spaces with underscores.

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/util.rs
git commit -m "fix: strip backslash and null byte in sanitize_filename"
```

---

### Task 12: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings. Fix any that appear (unused imports, etc.)

- [ ] **Step 3: Build release binary**

Run: `cargo build --release`
Expected: Builds successfully

- [ ] **Step 4: Verify help output**

Run: `cargo run -- --help`
Expected: Shows `--format` and `--format-preset` flags with descriptions, showing they are in the same group

- [ ] **Step 5: Verify mutual exclusivity**

Run: `cargo run -- --format "{title}.mkv" --format-preset plex`
Expected: Error from clap about conflicting arguments in `format_group`

- [ ] **Step 6: Commit any final cleanup**

```bash
git add -A
git commit -m "chore: final cleanup for configurable filename structure"
```
