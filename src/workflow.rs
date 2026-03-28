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

pub fn prepare_remux_options(
    device: &str,
    playlist: &Playlist,
    output: &Path,
    mount_point: Option<&str>,
    stream_selection: StreamSelection,
    cancel: Arc<AtomicBool>,
    reserve_index_space_kb: u32,
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
}
