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

    const MARKER: char = '\u{200B}';

    static PLACEHOLDER_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\{([a-z_]+)\}").expect("valid regex"));
    static EMPTY_BRACKET_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[[^\[\]]*\]").expect("valid regex"));
    static MULTI_SPACE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r" {2,}").expect("valid regex"));
    static SPACE_BEFORE_DOT_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r" +\.").expect("valid regex"));

    // 1. Substitute placeholders; wrap non-empty values with zero-width space markers
    let result = PLACEHOLDER_RE.replace_all(template, |caps: &regex::Captures| {
        let key = &caps[1];
        match vars.get(key) {
            Some(val) if !val.is_empty() => format!("{}{}{}", MARKER, val, MARKER),
            Some(_) => String::new(),
            None => caps[0].to_string(),
        }
    });
    let mut result = result.to_string();

    // 2. Bracket cleanup: collapse bracket groups that contain no markers
    //    (i.e., all placeholders inside resolved to empty)
    loop {
        let cleaned = EMPTY_BRACKET_RE.replace_all(&result, |caps: &regex::Captures| {
            let full = &caps[0];
            let content = &full[1..full.len() - 1];
            if content.contains(MARKER) {
                full.to_string()
            } else {
                String::new()
            }
        });
        let cleaned = MULTI_SPACE_RE.replace_all(&cleaned, " ").to_string();
        if cleaned == result {
            break;
        }
        result = cleaned;
    }

    // 3. Strip markers
    result = result.replace(MARKER, "");

    // 4. Clean up spaces before dots (e.g., " .mkv" -> ".mkv")
    result = SPACE_BEFORE_DOT_RE.replace_all(&result, ".").to_string();

    // 5. Trim
    result = result.trim().to_string();

    // 6. Sanitize per path component (preserve /)
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

    if indices.is_empty() {
        None
    } else {
        Some(indices)
    }
}

pub fn parse_episode_input(text: &str) -> Option<Vec<u32>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(vec![]);
    }

    let mut episodes = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let (start_s, end_s) = part.split_once('-')?;
            let start: u32 = start_s.trim().parse().ok()?;
            let end: u32 = end_s.trim().parse().ok()?;
            if start == 0 || end == 0 || start > end {
                return None;
            }
            episodes.extend(start..=end);
        } else {
            let val: u32 = part.parse().ok()?;
            if val == 0 {
                return None;
            }
            episodes.push(val);
        }
    }

    Some(episodes)
}

pub fn guess_start_episode(disc_number: Option<u32>, episodes_on_disc: usize) -> u32 {
    match disc_number {
        Some(d) if d >= 1 && episodes_on_disc >= 1 => 1 + (episodes_on_disc as u32) * (d - 1),
        _ => 1,
    }
}

pub fn assign_episodes(
    playlists: &[Playlist],
    episodes: &[Episode],
    start_episode: u32,
) -> HashMap<String, Vec<Episode>> {
    let ep_by_num: HashMap<u32, &Episode> =
        episodes.iter().map(|ep| (ep.episode_number, ep)).collect();

    // Compute median duration for multi-episode detection
    let median_secs = {
        let mut durations: Vec<u32> = playlists.iter().map(|pl| pl.seconds).collect();
        durations.sort();
        if durations.is_empty() {
            0
        } else {
            durations[durations.len() / 2]
        }
    };

    let mut assignments = HashMap::new();
    let mut ep_cursor = start_episode;

    for pl in playlists {
        // Determine how many episodes this playlist likely contains
        let ep_count = if playlists.len() > 1
            && median_secs > 0
            && pl.seconds as f64 >= median_secs as f64 * 1.5
        {
            (pl.seconds / median_secs).max(1)
        } else {
            1
        };

        let mut eps = Vec::new();
        for _ in 0..ep_count {
            if let Some(ep) = ep_by_num.get(&ep_cursor) {
                eps.push((*ep).clone());
            }
            ep_cursor += 1;
        }

        if !eps.is_empty() {
            assignments.insert(pl.num.clone(), eps);
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
    let Some(fmt) = format else {
        // Default format: use legacy sanitize_filename (underscores) for backwards compat
        let name = sanitize_filename(title);
        let year_suffix = if year.is_empty() {
            String::new()
        } else {
            format!("_({})", year)
        };
        let part_suffix = part.map(|p| format!("_pt{}", p)).unwrap_or_default();
        return format!("{}{}{}.mkv", name, year_suffix, part_suffix);
    };

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

    render_template(fmt, &vars)
}

pub fn make_filename(
    playlist_num: &str,
    episodes: &[Episode],
    season: u32,
    format: Option<&str>,
    media_info: Option<&MediaInfo>,
    extra_vars: Option<&HashMap<&str, String>>,
) -> String {
    if episodes.is_empty() {
        return format!("playlist{}.mkv", playlist_num);
    }

    let ep = &episodes[0];

    let episode_str = if episodes.len() > 1 {
        let last = &episodes[episodes.len() - 1];
        format!("{:02}-E{:02}", ep.episode_number, last.episode_number)
    } else {
        format!("{:02}", ep.episode_number)
    };

    let Some(fmt) = format else {
        // Default format: use legacy sanitize_filename (underscores) for backwards compat
        return format!(
            "S{:02}E{}_{}.mkv",
            season,
            episode_str,
            sanitize_filename(&ep.name)
        );
    };

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("season", format!("{:02}", season));
    vars.insert("episode", episode_str);
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

    render_template(fmt, &vars)
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
            Playlist {
                num: "00001".into(),
                duration: "0:43:00".into(),
                seconds: 2580,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
            Playlist {
                num: "00002".into(),
                duration: "0:44:00".into(),
                seconds: 2640,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
        ];
        let episodes = vec![
            Episode {
                episode_number: 1,
                name: "Pilot".into(),
                runtime: Some(44),
            },
            Episode {
                episode_number: 2,
                name: "Second".into(),
                runtime: Some(44),
            },
        ];
        let result = assign_episodes(&playlists, &episodes, 1);
        assert_eq!(result["00001"][0].name, "Pilot");
        assert_eq!(result["00002"][0].name, "Second");
    }

    #[test]
    fn test_assign_offset() {
        let playlists = vec![Playlist {
            num: "00003".into(),
            duration: "0:43:00".into(),
            seconds: 2580,
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
        }];
        let episodes = vec![
            Episode {
                episode_number: 1,
                name: "Pilot".into(),
                runtime: Some(44),
            },
            Episode {
                episode_number: 2,
                name: "Second".into(),
                runtime: Some(44),
            },
            Episode {
                episode_number: 3,
                name: "Third".into(),
                runtime: Some(44),
            },
        ];
        let result = assign_episodes(&playlists, &episodes, 3);
        assert_eq!(result["00003"][0].name, "Third");
    }

    #[test]
    fn test_assign_overflow() {
        let playlists = vec![
            Playlist {
                num: "00001".into(),
                duration: "0:43:00".into(),
                seconds: 2580,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
            Playlist {
                num: "00002".into(),
                duration: "0:44:00".into(),
                seconds: 2640,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
        ];
        let episodes = vec![Episode {
            episode_number: 1,
            name: "Pilot".into(),
            runtime: Some(44),
        }];
        let result = assign_episodes(&playlists, &episodes, 1);
        assert_eq!(result["00001"][0].name, "Pilot");
        assert!(!result.contains_key("00002"));
    }

    #[test]
    fn test_assign_empty() {
        let playlists = vec![Playlist {
            num: "00001".into(),
            duration: "0:43:00".into(),
            seconds: 2580,
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
        }];
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
        assert_eq!(
            make_movie_filename("Inception", "", None, None, None, None),
            "Inception.mkv"
        );
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
    fn test_make_filename_multi_episode_consecutive() {
        let eps = vec![
            Episode {
                episode_number: 3,
                name: "Third".into(),
                runtime: Some(44),
            },
            Episode {
                episode_number: 4,
                name: "Fourth".into(),
                runtime: Some(44),
            },
        ];
        assert_eq!(
            make_filename("00001", &eps, 1, None, None, None),
            "S01E03-E04_Third.mkv"
        );
    }

    #[test]
    fn test_make_filename_multi_episode_non_consecutive() {
        let eps = vec![
            Episode {
                episode_number: 3,
                name: "Third".into(),
                runtime: Some(44),
            },
            Episode {
                episode_number: 5,
                name: "Fifth".into(),
                runtime: Some(44),
            },
        ];
        assert_eq!(
            make_filename("00001", &eps, 1, None, None, None),
            "S01E03-E05_Third.mkv"
        );
    }

    #[test]
    fn test_make_filename_multi_episode_custom_format() {
        let eps = vec![
            Episode {
                episode_number: 3,
                name: "Third".into(),
                runtime: Some(44),
            },
            Episode {
                episode_number: 4,
                name: "Fourth".into(),
                runtime: Some(44),
            },
        ];
        let mut extra = HashMap::new();
        extra.insert("show", "Test Show".to_string());
        assert_eq!(
            make_filename(
                "00001",
                &eps,
                1,
                Some("{show}/S{season}E{episode} - {title}.mkv"),
                None,
                Some(&extra)
            ),
            "Test Show/S01E03-E04 - Third.mkv"
        );
    }

    #[test]
    fn test_make_filename_with_episode() {
        let ep = Episode {
            episode_number: 3,
            name: "The Pilot".into(),
            runtime: Some(44),
        };
        assert_eq!(
            make_filename("00001", &[ep], 1, None, None, None),
            "S01E03_The_Pilot.mkv"
        );
    }

    #[test]
    fn test_make_filename_no_episode() {
        assert_eq!(
            make_filename("00042", &[], 1, None, None, None),
            "playlist00042.mkv"
        );
    }

    #[test]
    fn test_make_filename_custom_format_with_show() {
        let ep = Episode {
            episode_number: 3,
            name: "The Pilot".into(),
            runtime: Some(44),
        };
        let mut extra = HashMap::new();
        extra.insert("show", "Test Show".to_string());
        assert_eq!(
            make_filename(
                "00001",
                &[ep],
                1,
                Some("{show}/S{season}E{episode} - {title}.mkv"),
                None,
                Some(&extra)
            ),
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
                "The Matrix",
                "1999",
                None,
                Some(
                    "{title} ({year})/Movie [Bluray-{resolution}][{audio} {channels}][{codec}].mkv"
                ),
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
            render_template(
                "{show}/Season {season}/S{season}E{episode} - {title}.mkv",
                &vars
            ),
            "Test Show/Season 02/S02E05 - Ep Name.mkv"
        );
    }

    #[test]
    fn test_render_template_unknown_placeholder_preserved() {
        let vars = HashMap::new();
        assert_eq!(render_template("{foo}_{bar}.mkv", &vars), "{foo}_{bar}.mkv");
    }

    #[test]
    fn test_render_template_empty_values_bracket_cleanup() {
        let mut vars = HashMap::new();
        vars.insert("resolution", "1080p".to_string());
        vars.insert("audio", String::new());
        vars.insert("channels", String::new());
        vars.insert("codec", "hevc".to_string());
        assert_eq!(
            render_template(
                "Movie [Bluray-{resolution}][{audio} {channels}][{codec}].mkv",
                &vars
            ),
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
            render_template(
                "Movie [Bluray-{resolution}][{audio} {channels}][{codec}].mkv",
                &vars
            ),
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

    #[test]
    fn test_render_template_custom_prefix_bracket_cleanup() {
        let mut vars = HashMap::new();
        vars.insert("resolution", String::new());
        assert_eq!(
            render_template("Movie [DVD-{resolution}].mkv", &vars),
            "Movie.mkv"
        );
    }

    #[test]
    fn test_parse_episode_input_single() {
        assert_eq!(parse_episode_input("3"), Some(vec![3]));
    }

    #[test]
    fn test_parse_episode_input_range() {
        assert_eq!(parse_episode_input("3-5"), Some(vec![3, 4, 5]));
    }

    #[test]
    fn test_parse_episode_input_comma() {
        assert_eq!(parse_episode_input("3,5"), Some(vec![3, 5]));
    }

    #[test]
    fn test_parse_episode_input_mixed() {
        assert_eq!(parse_episode_input("1,3-5"), Some(vec![1, 3, 4, 5]));
    }

    #[test]
    fn test_parse_episode_input_reversed_range() {
        assert_eq!(parse_episode_input("5-3"), None);
    }

    #[test]
    fn test_parse_episode_input_zero() {
        assert_eq!(parse_episode_input("0"), None);
    }

    #[test]
    fn test_parse_episode_input_empty() {
        assert_eq!(parse_episode_input(""), Some(vec![]));
    }

    #[test]
    fn test_parse_episode_input_non_numeric() {
        assert_eq!(parse_episode_input("abc"), None);
    }

    #[test]
    fn test_parse_episode_input_whitespace() {
        assert_eq!(parse_episode_input(" 3 , 5 "), Some(vec![3, 5]));
    }

    #[test]
    fn test_assign_double_episode() {
        // Three normal playlists + one double-length
        let playlists = vec![
            Playlist {
                num: "00001".into(),
                duration: "0:43:00".into(),
                seconds: 2580,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
            Playlist {
                num: "00002".into(),
                duration: "0:44:00".into(),
                seconds: 2640,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
            Playlist {
                num: "00003".into(),
                duration: "0:45:00".into(),
                seconds: 2700,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
            Playlist {
                num: "00004".into(),
                duration: "1:30:00".into(),
                seconds: 5400,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
        ];
        let episodes: Vec<Episode> = (1..=5)
            .map(|n| Episode {
                episode_number: n,
                name: format!("Episode {}", n),
                runtime: Some(44),
            })
            .collect();
        let result = assign_episodes(&playlists, &episodes, 1);
        // First three get one episode each
        assert_eq!(result["00001"].len(), 1);
        assert_eq!(result["00001"][0].episode_number, 1);
        assert_eq!(result["00002"][0].episode_number, 2);
        assert_eq!(result["00003"][0].episode_number, 3);
        // Double-length playlist gets two episodes
        assert_eq!(result["00004"].len(), 2);
        assert_eq!(result["00004"][0].episode_number, 4);
        assert_eq!(result["00004"][1].episode_number, 5);
    }

    #[test]
    fn test_assign_all_same_length_no_doubles() {
        let playlists = vec![
            Playlist {
                num: "00001".into(),
                duration: "0:43:00".into(),
                seconds: 2580,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
            Playlist {
                num: "00002".into(),
                duration: "0:44:00".into(),
                seconds: 2640,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
        ];
        let episodes: Vec<Episode> = (1..=2)
            .map(|n| Episode {
                episode_number: n,
                name: format!("Episode {}", n),
                runtime: Some(44),
            })
            .collect();
        let result = assign_episodes(&playlists, &episodes, 1);
        assert_eq!(result["00001"].len(), 1);
        assert_eq!(result["00002"].len(), 1);
    }

    #[test]
    fn test_assign_single_playlist_no_double_detect() {
        let playlists = vec![Playlist {
            num: "00001".into(),
            duration: "1:30:00".into(),
            seconds: 5400,
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
        }];
        let episodes: Vec<Episode> = (1..=2)
            .map(|n| Episode {
                episode_number: n,
                name: format!("Episode {}", n),
                runtime: Some(44),
            })
            .collect();
        let result = assign_episodes(&playlists, &episodes, 1);
        // Single playlist — can't detect doubles, gets 1 episode
        assert_eq!(result["00001"].len(), 1);
        assert_eq!(result["00001"][0].episode_number, 1);
    }

    #[test]
    fn test_assign_double_episode_exhausts_episodes() {
        let playlists = vec![
            Playlist {
                num: "00001".into(),
                duration: "0:44:00".into(),
                seconds: 2640,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
            Playlist {
                num: "00002".into(),
                duration: "1:30:00".into(),
                seconds: 5400,
                video_streams: 0,
                audio_streams: 0,
                subtitle_streams: 0,
            },
        ];
        let episodes: Vec<Episode> = (1..=2)
            .map(|n| Episode {
                episode_number: n,
                name: format!("Episode {}", n),
                runtime: Some(44),
            })
            .collect();
        let result = assign_episodes(&playlists, &episodes, 1);
        assert_eq!(result["00001"][0].episode_number, 1);
        // Double wants 2 episodes but only 1 remains
        assert_eq!(result["00002"].len(), 1);
        assert_eq!(result["00002"][0].episode_number, 2);
    }

    #[test]
    fn test_make_filename_special_format() {
        let episodes = vec![Episode {
            episode_number: 1,
            name: String::new(),
            runtime: None,
        }];
        let result = make_filename(
            "00006",
            &episodes,
            2,
            Some("{show} S{season}SP{episode} {title}.mkv"),
            None,
            Some(&{
                let mut m = HashMap::new();
                m.insert("show", "Test Show".to_string());
                m
            }),
        );
        assert_eq!(result, "Test Show S02SP01.mkv");
    }

    #[test]
    fn test_make_filename_special_format_with_season() {
        let episodes = vec![Episode {
            episode_number: 3,
            name: "Behind the Scenes".to_string(),
            runtime: None,
        }];
        let result = make_filename(
            "00010",
            &episodes,
            5,
            Some("S{season}SP{episode}_{title}.mkv"),
            None,
            None,
        );
        assert_eq!(result, "S05SP03_Behind the Scenes.mkv");
    }
}
