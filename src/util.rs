use crate::types::{Episode, MediaInfo, Playlist};
use std::collections::HashMap;

pub fn duration_to_seconds(dur: &str) -> u32 {
    let parts: Vec<&str> = dur.split(':').collect();
    match parts.len() {
        3 => {
            let h: u32 = parts[0].parse().unwrap_or(0);
            let m: u32 = parts[1].parse().unwrap_or(0);
            let s: u32 = parts[2].parse().unwrap_or(0);
            h * 3600 + m * 60 + s
        }
        2 => {
            let m: u32 = parts[0].parse().unwrap_or(0);
            let s: u32 = parts[1].parse().unwrap_or(0);
            m * 60 + s
        }
        _ => 0,
    }
}

pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !UNSAFE_PATH_CHARS.contains(c) && *c != '\0')
        .collect();
    cleaned.replace(' ', "_")
}

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
    static SPACE_BEFORE_DOT_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r" +\.").unwrap());

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
    // TODO(debt): This hardcodes "Bluray-" as a known filler prefix. Custom templates with
    // other prefixes inside brackets would leave stale prefix text when placeholders are empty.
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
        // Also clean up double spaces after bracket removal
        let cleaned = MULTI_SPACE_RE.replace_all(&cleaned, " ").to_string();
        if cleaned == result {
            break;
        }
        result = cleaned;
    }

    // 3. Clean up spaces before dots (e.g., " .mkv" -> ".mkv")
    result = SPACE_BEFORE_DOT_RE.replace_all(&result, ".").to_string();

    // 4. Trim
    result = result.trim().to_string();

    // 5. Sanitize per path component (preserve /)
    result = result
        .split('/')
        .map(sanitize_path_component)
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("/");

    result
}

pub fn parse_selection(text: &str, max_val: usize) -> Option<Vec<usize>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text == "all" {
        return Some((0..max_val).collect());
    }

    let mut indices = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let (start_s, end_s) = part.split_once('-')?;
            let start: usize = start_s.parse().ok()?;
            let end: usize = if end_s.is_empty() {
                max_val
            } else {
                end_s.parse().ok()?
            };
            if start > end || start < 1 || end > max_val {
                return None;
            }
            indices.extend(start - 1..end);
        } else {
            let val: usize = part.parse().ok()?;
            if val < 1 || val > max_val {
                return None;
            }
            indices.push(val - 1);
        }
    }

    if indices.is_empty() { None } else { Some(indices) }
}

pub fn guess_start_episode(disc_number: Option<u32>, episodes_on_disc: usize) -> u32 {
    match disc_number {
        Some(d) if d >= 1 && episodes_on_disc >= 1 => {
            1 + (episodes_on_disc as u32) * (d - 1)
        }
        _ => 1,
    }
}

pub fn assign_episodes(
    playlists: &[Playlist],
    episodes: &[Episode],
    start_episode: u32,
) -> HashMap<String, Episode> {
    let ep_by_num: HashMap<u32, &Episode> = episodes
        .iter()
        .map(|ep| (ep.episode_number, ep))
        .collect();

    let mut assignments = HashMap::new();
    for (i, pl) in playlists.iter().enumerate() {
        let ep_num = start_episode + i as u32;
        if let Some(ep) = ep_by_num.get(&ep_num) {
            assignments.insert(pl.num.clone(), (*ep).clone());
        }
    }
    assignments
}

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

pub fn format_size(bytes: u64) -> String {
    let mut size = bytes as f64;
    for unit in &["B", "KiB", "MiB", "GiB"] {
        if size.abs() < 1024.0 {
            return format!("{:.1} {}", size, unit);
        }
        size /= 1024.0;
    }
    format!("{:.1} TiB", size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_hms() {
        assert_eq!(duration_to_seconds("1:23:45"), 5025);
    }

    #[test]
    fn test_duration_ms() {
        assert_eq!(duration_to_seconds("23:45"), 1425);
    }

    #[test]
    fn test_duration_zeros() {
        assert_eq!(duration_to_seconds("0:00:00"), 0);
    }

    #[test]
    fn test_duration_invalid() {
        assert_eq!(duration_to_seconds(""), 0);
    }

    #[test]
    fn test_sanitize_spaces() {
        assert_eq!(sanitize_filename("Hello World"), "Hello_World");
    }

    #[test]
    fn test_sanitize_special_chars() {
        assert_eq!(sanitize_filename(r#"foo/bar:baz"qux"#), "foobarbazqux");
    }

    #[test]
    fn test_sanitize_preserves_parens() {
        assert_eq!(sanitize_filename("Earth (Part 1)"), "Earth_(Part_1)");
    }

    #[test]
    fn test_sanitize_backslash_and_null() {
        assert_eq!(sanitize_filename("test\\path\0here"), "testpathhere");
    }

    #[test]
    fn test_selection_single() {
        assert_eq!(parse_selection("2", 5), Some(vec![1]));
    }

    #[test]
    fn test_selection_comma() {
        assert_eq!(parse_selection("1,3,5", 5), Some(vec![0, 2, 4]));
    }

    #[test]
    fn test_selection_range() {
        assert_eq!(parse_selection("2-4", 5), Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_selection_mixed() {
        assert_eq!(parse_selection("1,3-5", 5), Some(vec![0, 2, 3, 4]));
    }

    #[test]
    fn test_selection_all() {
        assert_eq!(parse_selection("all", 3), Some(vec![0, 1, 2]));
    }

    #[test]
    fn test_selection_out_of_bounds() {
        assert_eq!(parse_selection("6", 5), None);
    }

    #[test]
    fn test_selection_zero() {
        assert_eq!(parse_selection("0", 5), None);
    }

    #[test]
    fn test_selection_invalid() {
        assert_eq!(parse_selection("abc", 5), None);
    }

    #[test]
    fn test_selection_empty() {
        assert_eq!(parse_selection("", 5), None);
    }

    #[test]
    fn test_selection_reversed_range() {
        assert_eq!(parse_selection("4-2", 5), None);
    }

    #[test]
    fn test_selection_open_ended() {
        assert_eq!(parse_selection("3-", 5), Some(vec![2, 3, 4]));
    }

    #[test]
    fn test_guess_disc_1() {
        assert_eq!(guess_start_episode(Some(1), 5), 1);
    }

    #[test]
    fn test_guess_disc_2() {
        assert_eq!(guess_start_episode(Some(2), 5), 6);
    }

    #[test]
    fn test_guess_no_disc() {
        assert_eq!(guess_start_episode(None, 5), 1);
    }

    #[test]
    fn test_guess_zero_episodes() {
        assert_eq!(guess_start_episode(Some(2), 0), 1);
    }

    #[test]
    fn test_assign_basic() {
        let playlists = vec![
            Playlist { num: "00001".into(), duration: "0:43:00".into(), seconds: 2580 },
            Playlist { num: "00002".into(), duration: "0:44:00".into(), seconds: 2640 },
        ];
        let episodes = vec![
            Episode { episode_number: 1, name: "Pilot".into(), runtime: Some(44) },
            Episode { episode_number: 2, name: "Second".into(), runtime: Some(44) },
        ];
        let result = assign_episodes(&playlists, &episodes, 1);
        assert_eq!(result["00001"].name, "Pilot");
        assert_eq!(result["00002"].name, "Second");
    }

    #[test]
    fn test_assign_offset() {
        let playlists = vec![
            Playlist { num: "00003".into(), duration: "0:43:00".into(), seconds: 2580 },
        ];
        let episodes = vec![
            Episode { episode_number: 1, name: "Pilot".into(), runtime: Some(44) },
            Episode { episode_number: 2, name: "Second".into(), runtime: Some(44) },
            Episode { episode_number: 3, name: "Third".into(), runtime: Some(44) },
        ];
        let result = assign_episodes(&playlists, &episodes, 3);
        assert_eq!(result["00003"].name, "Third");
    }

    #[test]
    fn test_assign_overflow() {
        let playlists = vec![
            Playlist { num: "00001".into(), duration: "0:43:00".into(), seconds: 2580 },
            Playlist { num: "00002".into(), duration: "0:44:00".into(), seconds: 2640 },
        ];
        let episodes = vec![
            Episode { episode_number: 1, name: "Pilot".into(), runtime: Some(44) },
        ];
        let result = assign_episodes(&playlists, &episodes, 1);
        assert_eq!(result["00001"].name, "Pilot");
        assert!(!result.contains_key("00002"));
    }

    #[test]
    fn test_assign_empty() {
        let playlists = vec![
            Playlist { num: "00001".into(), duration: "0:43:00".into(), seconds: 2580 },
        ];
        let result = assign_episodes(&playlists, &[], 1);
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_size(500), "500.0 B");
    }

    #[test]
    fn test_format_kib() {
        assert_eq!(format_size(2048), "2.0 KiB");
    }

    #[test]
    fn test_format_mib() {
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn test_format_gib() {
        assert_eq!(format_size(3 * 1024u64.pow(3)), "3.0 GiB");
    }

    #[test]
    fn test_format_tib() {
        assert_eq!(format_size(2 * 1024u64.pow(4)), "2.0 TiB");
    }

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
}
