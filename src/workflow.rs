use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::config::Config;
use crate::media::{RemuxOptions, StreamSelection};
use crate::types::{Episode, LabelInfo, MediaInfo, Playlist};
use crate::util;

#[derive(Debug, PartialEq)]
pub enum OverwriteAction {
    /// File does not exist, proceed with rip
    Proceed,
    /// File exists and overwrite is disabled — skip (carries existing file size)
    Skip(u64),
    /// File exists and overwrite is enabled — file deleted (carries pre-deletion size)
    DeleteAndProceed(u64),
}

pub fn check_overwrite(output: &Path, overwrite: bool) -> std::io::Result<OverwriteAction> {
    log::debug!("Overwrite check: {}", output.display());
    if !output.exists() {
        return Ok(OverwriteAction::Proceed);
    }
    let size = std::fs::metadata(output)?.len();
    if overwrite {
        std::fs::remove_file(output)?;
        Ok(OverwriteAction::DeleteAndProceed(size))
    } else {
        Ok(OverwriteAction::Skip(size))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_remux_options(
    device: &str,
    playlist: &Playlist,
    output: &Path,
    mount_point: Option<&str>,
    stream_selection: StreamSelection,
    cancel: Arc<AtomicBool>,
    reserve_index_space_kb: u32,
    metadata: Option<crate::types::MkvMetadata>,
) -> RemuxOptions {
    let chapters = mount_point
        .and_then(|mount| {
            crate::chapters::extract_chapters(std::path::Path::new(mount), &playlist.num)
        })
        .unwrap_or_default();

    RemuxOptions {
        device: device.to_string(),
        playlist: playlist.num.clone(),
        output: output.to_path_buf(),
        chapters,
        stream_selection,
        cancel,
        reserve_index_space_kb,
        metadata,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_output_filename(
    playlist: &Playlist,
    episodes: &[Episode],
    season: u32,
    is_movie: bool,
    is_special: bool,
    movie_title: Option<(&str, &str)>,
    show_name: &str,
    label: &str,
    label_info: Option<&LabelInfo>,
    config: &Config,
    cli_format: Option<&str>,
    cli_preset: Option<&str>,
    media_info: Option<&MediaInfo>,
    part: Option<u32>,
) -> String {
    let mut extra_vars: HashMap<&str, String> = HashMap::new();
    extra_vars.insert("show", show_name.to_string());
    extra_vars.insert(
        "disc",
        label_info.map(|l| l.disc.to_string()).unwrap_or_default(),
    );
    extra_vars.insert("label", label.to_string());
    extra_vars.insert("playlist", playlist.num.clone());

    // Canonical use_custom_format logic — unified across CLI and TUI.
    // Previously, TUI movie mode omitted config.tv_format from this check.
    // The unified check means setting tv_format in config will trigger custom
    // format resolution even in movie mode. This is intentional: any format
    // customization should opt into template rendering.
    let use_custom_format = cli_format.is_some()
        || cli_preset.is_some()
        || config.tv_format.is_some()
        || config.movie_format.is_some()
        || config.preset.is_some();

    let filename = if let Some((title, year)) = movie_title {
        let fmt = if use_custom_format {
            let template = config.resolve_format(true, cli_format, cli_preset);
            Some(template)
        } else {
            None
        };
        util::make_movie_filename(
            title,
            year,
            part,
            fmt.as_deref(),
            media_info,
            Some(&extra_vars),
        )
    } else if is_special {
        let special_fmt = config.resolve_special_format(cli_format);
        util::make_filename(
            &playlist.num,
            episodes,
            season,
            Some(special_fmt.as_str()),
            media_info,
            Some(&extra_vars),
        )
    } else {
        let fmt = if use_custom_format {
            let template = config.resolve_format(is_movie, cli_format, cli_preset);
            Some(template)
        } else {
            None
        };
        util::make_filename(
            &playlist.num,
            episodes,
            season,
            fmt.as_deref(),
            media_info,
            Some(&extra_vars),
        )
    };
    log::debug!("Output filename: {}", filename);
    filename
}

/// Build MKV metadata tags from available context.
/// Returns `None` if metadata is disabled.
#[allow(clippy::too_many_arguments)]
pub fn build_metadata(
    enabled: bool,
    movie_mode: bool,
    show_name: Option<&str>,
    season: Option<u32>,
    episodes: &[Episode],
    movie_title: Option<&str>,
    date_released: Option<&str>,
    custom_tags: &HashMap<String, String>,
) -> Option<crate::types::MkvMetadata> {
    if !enabled {
        return None;
    }

    let mut tags = HashMap::new();

    // Use REMUXED_WITH instead of ENCODER — FFmpeg's Matroska muxer
    // always overwrites ENCODER with its own "Lavf" version string.
    let encoder = format!("bluback v{}", env!("CARGO_PKG_VERSION"));
    tags.insert("REMUXED_WITH".into(), encoder);

    if movie_mode {
        if let Some(title) = movie_title {
            if !title.is_empty() {
                tags.insert("TITLE".into(), title.to_string());
            }
        }
    } else {
        // TV mode: episode name(s) as TITLE, fall back to show name
        let title = if episodes.len() > 1 {
            let names: Vec<&str> = episodes
                .iter()
                .map(|e| e.name.as_str())
                .filter(|n| !n.is_empty())
                .collect();
            if names.is_empty() { None } else { Some(names.join(" / ")) }
        } else if let Some(ep) = episodes.first() {
            if ep.name.is_empty() { None } else { Some(ep.name.clone()) }
        } else {
            None
        };

        let title = title.or_else(|| show_name.filter(|s| !s.is_empty()).map(String::from));
        if let Some(t) = title {
            tags.insert("TITLE".into(), t);
        }

        if let Some(name) = show_name {
            if !name.is_empty() {
                tags.insert("SHOW".into(), name.to_string());
            }
        }
        if let Some(s) = season {
            tags.insert("SEASON_NUMBER".into(), s.to_string());
        }
        if let Some(ep) = episodes.first() {
            tags.insert("EPISODE_SORT".into(), ep.episode_number.to_string());
        }
    }

    if let Some(date) = date_released {
        if !date.is_empty() {
            tags.insert("DATE_RELEASED".into(), date.to_string());
        }
    }

    // Custom tags override auto-generated ones
    for (k, v) in custom_tags {
        if !v.is_empty() {
            tags.insert(k.clone(), v.clone());
        }
    }

    Some(crate::types::MkvMetadata { tags })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::StreamSelection;
    use std::io::Write;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn test_check_overwrite_file_not_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.mkv");
        let result = check_overwrite(&path, false).unwrap();
        assert_eq!(result, OverwriteAction::Proceed);
    }

    #[test]
    fn test_check_overwrite_exists_no_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.mkv");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0u8; 1024]).unwrap();
        let result = check_overwrite(&path, false).unwrap();
        assert!(matches!(result, OverwriteAction::Skip(1024)));
        assert!(path.exists());
    }

    #[test]
    fn test_check_overwrite_exists_with_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.mkv");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0u8; 2048]).unwrap();
        let result = check_overwrite(&path, true).unwrap();
        assert!(matches!(result, OverwriteAction::DeleteAndProceed(2048)));
        assert!(!path.exists());
    }

    #[test]
    fn test_prepare_remux_options_no_mount() {
        let playlist = crate::types::Playlist {
            num: "00001".into(),
            duration: "1:23:45".into(),
            seconds: 5025,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = prepare_remux_options(
            "/dev/sr0",
            &playlist,
            Path::new("/tmp/out.mkv"),
            None,
            StreamSelection::All,
            cancel,
            500,
            None,
        );
        assert_eq!(opts.device, "/dev/sr0");
        assert_eq!(opts.playlist, "00001");
        assert_eq!(opts.output, std::path::PathBuf::from("/tmp/out.mkv"));
        assert!(opts.chapters.is_empty());
        assert_eq!(opts.reserve_index_space_kb, 500);
    }

    #[test]
    fn test_prepare_remux_options_bad_mount_swallows_error() {
        let playlist = crate::types::Playlist {
            num: "00001".into(),
            duration: "1:23:45".into(),
            seconds: 5025,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = prepare_remux_options(
            "/dev/sr0",
            &playlist,
            Path::new("/tmp/out.mkv"),
            Some("/nonexistent/mount"),
            StreamSelection::All,
            cancel,
            500,
            None,
        );
        assert!(opts.chapters.is_empty());
    }

    #[test]
    fn test_build_output_filename_tv_default() {
        let pl = crate::types::Playlist {
            num: "00001".into(),
            duration: "0:45:00".into(),
            seconds: 2700,
        };
        let eps = vec![crate::types::Episode {
            episode_number: 3,
            name: "Test Episode".into(),
            runtime: None,
        }];
        let config = Config::default();
        let result = build_output_filename(
            &pl,
            &eps,
            2,
            false,
            false,
            None,
            "Show Name",
            "LABEL",
            None,
            &config,
            None,
            None,
            None,
            None,
        );
        assert_eq!(result, "S02E03_Test_Episode.mkv");
    }

    #[test]
    fn test_build_output_filename_movie() {
        let pl = crate::types::Playlist {
            num: "00001".into(),
            duration: "2:00:00".into(),
            seconds: 7200,
        };
        let config = Config::default();
        let result = build_output_filename(
            &pl,
            &[],
            0,
            true,
            false,
            Some(("My Movie", "2024")),
            "",
            "LABEL",
            None,
            &config,
            None,
            None,
            None,
            None,
        );
        assert_eq!(result, "My_Movie_(2024).mkv");
    }

    #[test]
    fn test_build_output_filename_special() {
        let pl = crate::types::Playlist {
            num: "00006".into(),
            duration: "0:30:00".into(),
            seconds: 1800,
        };
        let eps = vec![crate::types::Episode {
            episode_number: 1,
            name: String::new(),
            runtime: None,
        }];
        let config = Config::default();
        let result = build_output_filename(
            &pl, &eps, 2, false, true, None, "Show", "LABEL", None, &config, None, None, None, None,
        );
        // DEFAULT_SPECIAL_FORMAT = "{show} S{season}SP{episode} {title}.mkv"
        assert!(result.contains("S02SP01"));
        assert!(result.contains("Show"));
    }

    #[test]
    fn test_build_output_filename_movie_multipart() {
        let pl = crate::types::Playlist {
            num: "00001".into(),
            duration: "1:00:00".into(),
            seconds: 3600,
        };
        let config = Config::default();
        let result = build_output_filename(
            &pl,
            &[],
            0,
            true,
            false,
            Some(("Movie", "2024")),
            "",
            "",
            None,
            &config,
            None,
            None,
            None,
            Some(2),
        );
        assert_eq!(result, "Movie_(2024)_pt2.mkv");
    }

    #[test]
    fn test_build_output_filename_custom_format() {
        let pl = crate::types::Playlist {
            num: "00001".into(),
            duration: "0:45:00".into(),
            seconds: 2700,
        };
        let eps = vec![crate::types::Episode {
            episode_number: 1,
            name: "Pilot".into(),
            runtime: None,
        }];
        let config = Config::default();
        let result = build_output_filename(
            &pl,
            &eps,
            1,
            false,
            false,
            None,
            "Show",
            "LABEL",
            None,
            &config,
            Some("{show}/S{season}E{episode} - {title}.mkv"),
            None,
            None,
            None,
        );
        assert_eq!(result, "Show/S01E01 - Pilot.mkv");
    }

    #[test]
    fn test_build_metadata_tv_full() {
        let meta = build_metadata(
            true, false,
            Some("Game of Thrones"), Some(3),
            &[crate::types::Episode { episode_number: 9, name: "The Rains of Castamere".into(), runtime: None }],
            None, Some("2013-06-02"),
            &HashMap::new(),
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "The Rains of Castamere");
        assert_eq!(meta.tags["SHOW"], "Game of Thrones");
        assert_eq!(meta.tags["SEASON_NUMBER"], "3");
        assert_eq!(meta.tags["EPISODE_SORT"], "9");
        assert_eq!(meta.tags["DATE_RELEASED"], "2013-06-02");
        assert!(meta.tags["REMUXED_WITH"].starts_with("bluback v"));
    }

    #[test]
    fn test_build_metadata_tv_multi_episode() {
        let meta = build_metadata(
            true, false, Some("Show"), Some(1),
            &[
                crate::types::Episode { episode_number: 3, name: "Ep Three".into(), runtime: None },
                crate::types::Episode { episode_number: 4, name: "Ep Four".into(), runtime: None },
            ],
            None, None, &HashMap::new(),
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Ep Three / Ep Four");
        assert_eq!(meta.tags["EPISODE_SORT"], "3");
    }

    #[test]
    fn test_build_metadata_movie() {
        let meta = build_metadata(
            true, true, None, None, &[],
            Some("Blade Runner 2049"), Some("2017-10-06"),
            &HashMap::new(),
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Blade Runner 2049");
        assert_eq!(meta.tags["DATE_RELEASED"], "2017-10-06");
        assert!(meta.tags["REMUXED_WITH"].starts_with("bluback v"));
        assert!(!meta.tags.contains_key("SHOW"));
        assert!(!meta.tags.contains_key("SEASON_NUMBER"));
    }

    #[test]
    fn test_build_metadata_tmdb_skipped() {
        let meta = build_metadata(
            true, false, Some("Manual Title"), None, &[],
            None, None, &HashMap::new(),
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Manual Title");
        assert_eq!(meta.tags["SHOW"], "Manual Title");
        assert!(meta.tags["REMUXED_WITH"].starts_with("bluback v"));
        assert!(!meta.tags.contains_key("DATE_RELEASED"));
    }

    #[test]
    fn test_build_metadata_custom_tags() {
        let mut custom = HashMap::new();
        custom.insert("STUDIO".into(), "HBO".into());
        let meta = build_metadata(
            true, false, Some("Show"), Some(1),
            &[crate::types::Episode { episode_number: 1, name: "Pilot".into(), runtime: None }],
            None, None, &custom,
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["STUDIO"], "HBO");
        assert_eq!(meta.tags["TITLE"], "Pilot");
    }

    #[test]
    fn test_build_metadata_custom_overrides_auto() {
        let mut custom = HashMap::new();
        custom.insert("TITLE".into(), "Custom Title".into());
        let meta = build_metadata(
            true, false, Some("Show"), Some(1),
            &[crate::types::Episode { episode_number: 1, name: "Auto Title".into(), runtime: None }],
            None, None, &custom,
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Custom Title");
    }

    #[test]
    fn test_build_metadata_disabled() {
        let meta = build_metadata(
            false, false, Some("Show"), Some(1),
            &[crate::types::Episode { episode_number: 1, name: "Pilot".into(), runtime: None }],
            None, None, &HashMap::new(),
        );
        assert!(meta.is_none());
    }

    #[test]
    fn test_build_metadata_no_empty_strings() {
        let meta = build_metadata(
            true, false, Some("Show"), Some(1),
            &[crate::types::Episode { episode_number: 1, name: String::new(), runtime: None }],
            None, None, &HashMap::new(),
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Show");
        for (k, v) in &meta.tags {
            assert!(!v.is_empty(), "Tag {} has empty value", k);
        }
    }
}
