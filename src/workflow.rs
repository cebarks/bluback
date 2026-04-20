use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::config::Config;
use crate::media::{RemuxOptions, StreamSelection};
use crate::streams::StreamFilter;
use crate::types::{Episode, LabelInfo, MediaInfo, Playlist};
use crate::util;

/// All context needed to run a remux job in a background thread.
///
/// Collected on the main thread from session state, then moved into the
/// rip thread where it drives open_remux_input, filename resolution,
/// overwrite checks, stream selection, and write_remux.
pub struct RipThreadContext {
    pub device: String,
    pub playlist: Playlist,
    pub output_dir: PathBuf,
    pub episodes: Vec<Episode>,
    pub season: u32,
    pub movie_mode: bool,
    pub is_special: bool,
    pub movie_title: Option<(String, String)>,
    pub show_name: String,
    pub label: String,
    pub label_info: Option<LabelInfo>,
    pub config: Config,
    pub format_override: Option<String>,
    pub format_preset_override: Option<String>,
    pub part: Option<u32>,
    pub cached_track_selection: Option<Vec<usize>>,
    pub stream_filter: StreamFilter,
    pub overwrite: bool,
    pub estimated_size: u64,
}

impl RipThreadContext {
    /// Compute the output filename, optionally using probed MediaInfo for
    /// resolution/codec placeholders in custom format templates.
    pub fn resolve_filename(&self, media_info: Option<&MediaInfo>) -> String {
        build_output_filename(
            &self.playlist,
            &self.episodes,
            self.season,
            self.movie_mode,
            self.is_special,
            self.movie_title
                .as_ref()
                .map(|(t, y)| (t.as_str(), y.as_str())),
            &self.show_name,
            &self.label,
            self.label_info.as_ref(),
            &self.config,
            self.format_override.as_deref(),
            self.format_preset_override.as_deref(),
            media_info,
            self.part,
        )
    }

    /// Resolve stream selection from cached manual track picks, stream filter,
    /// or fall back to All.
    pub fn resolve_stream_selection(
        &self,
        stream_info: &crate::types::StreamInfo,
    ) -> StreamSelection {
        if let Some(ref indices) = self.cached_track_selection {
            StreamSelection::Manual(indices.clone())
        } else if !self.stream_filter.is_empty() {
            StreamSelection::Manual(self.stream_filter.apply(stream_info))
        } else {
            StreamSelection::All
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum OverwriteAction {
    /// File does not exist, proceed with rip
    Proceed,
    /// File exists and overwrite is disabled — skip (carries existing file size)
    Skip(u64),
    /// File exists and overwrite is enabled — file deleted (carries pre-deletion size)
    DeleteAndProceed(u64),
    /// File exists but is a partial rip (<90% of estimated size) — deleted and will re-rip
    PartialReplace(u64),
}

/// Threshold below which an existing file is considered a partial/incomplete rip.
/// Files smaller than 90% of the estimated size are treated as partial.
const PARTIAL_THRESHOLD: f64 = 0.90;

pub fn check_overwrite(
    output: &Path,
    overwrite: bool,
    estimated_size: Option<u64>,
) -> std::io::Result<OverwriteAction> {
    log::debug!("Overwrite check: {}", output.display());
    if !output.exists() {
        return Ok(OverwriteAction::Proceed);
    }
    let size = std::fs::metadata(output)?.len();
    if overwrite {
        std::fs::remove_file(output)?;
        return Ok(OverwriteAction::DeleteAndProceed(size));
    }
    // Detect partial rips: if we have an estimate and the file is significantly
    // smaller, treat it as incomplete rather than skipping
    if let Some(est) = estimated_size {
        if est > 0 && (size as f64) < (est as f64 * PARTIAL_THRESHOLD) {
            log::info!(
                "Partial rip detected: {} is {} but expected ~{} ({}%)",
                output.display(),
                format_size_inline(size),
                format_size_inline(est),
                (size as f64 / est as f64 * 100.0) as u32,
            );
            std::fs::remove_file(output)?;
            return Ok(OverwriteAction::PartialReplace(size));
        }
    }
    Ok(OverwriteAction::Skip(size))
}

fn format_size_inline(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const MB: u64 = 1_048_576;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    }
}

pub fn prepare_remux_options(
    playlist: &Playlist,
    mount_point: Option<&str>,
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
        chapters,
        stream_selection: StreamSelection::All,
        cancel,
        reserve_index_space_kb,
        metadata,
    }
}

const TS_TO_MKV_FACTOR: f64 = 0.97;
const FALLBACK_BYTERATE: u64 = 2_500_000;

/// Estimate output MKV size for a playlist.
///
/// Priority: on-disc clip size (with TS→MKV correction) > probed bitrate > fallback (~20 Mbps).
pub fn estimate_size(
    playlist: &Playlist,
    clip_size: Option<u64>,
    media_info: Option<&MediaInfo>,
) -> u64 {
    clip_size
        .filter(|&sz| sz > 0)
        .map(|sz| (sz as f64 * TS_TO_MKV_FACTOR) as u64)
        .or_else(|| {
            media_info
                .map(|info| info.bitrate_bps / 8)
                .filter(|&br| br > 0)
                .map(|br| playlist.seconds as u64 * br)
        })
        .unwrap_or_else(|| playlist.seconds as u64 * FALLBACK_BYTERATE)
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
            if names.is_empty() {
                None
            } else {
                Some(names.join(" / "))
            }
        } else if let Some(ep) = episodes.first() {
            if ep.name.is_empty() {
                None
            } else {
                Some(ep.name.clone())
            }
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
        let result = check_overwrite(&path, false, None).unwrap();
        assert_eq!(result, OverwriteAction::Proceed);
    }

    #[test]
    fn test_check_overwrite_exists_no_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.mkv");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0u8; 1024]).unwrap();
        let result = check_overwrite(&path, false, None).unwrap();
        assert!(matches!(result, OverwriteAction::Skip(1024)));
        assert!(path.exists());
    }

    #[test]
    fn test_check_overwrite_exists_with_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.mkv");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0u8; 2048]).unwrap();
        let result = check_overwrite(&path, true, None).unwrap();
        assert!(matches!(result, OverwriteAction::DeleteAndProceed(2048)));
        assert!(!path.exists());
    }

    #[test]
    fn test_check_overwrite_partial_file_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("partial.mkv");
        let mut f = std::fs::File::create(&path).unwrap();
        // 4700 bytes existing, estimated 8500 — ~55%, well below 90% threshold
        f.write_all(&vec![0u8; 4700]).unwrap();
        let result = check_overwrite(&path, false, Some(8500)).unwrap();
        assert!(matches!(result, OverwriteAction::PartialReplace(4700)));
        assert!(!path.exists());
    }

    #[test]
    fn test_check_overwrite_complete_file_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("complete.mkv");
        let mut f = std::fs::File::create(&path).unwrap();
        // 9200 bytes existing, estimated 10000 — 92%, above 90% threshold
        f.write_all(&vec![0u8; 9200]).unwrap();
        let result = check_overwrite(&path, false, Some(10000)).unwrap();
        assert!(matches!(result, OverwriteAction::Skip(9200)));
        assert!(path.exists());
    }

    #[test]
    fn test_check_overwrite_partial_with_overwrite_flag_prefers_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("partial.mkv");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&vec![0u8; 4700]).unwrap();
        // overwrite=true takes precedence — returns DeleteAndProceed, not PartialReplace
        let result = check_overwrite(&path, true, Some(8500)).unwrap();
        assert!(matches!(result, OverwriteAction::DeleteAndProceed(4700)));
        assert!(!path.exists());
    }

    #[test]
    fn test_check_overwrite_partial_zero_estimate_skips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.mkv");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0u8; 1024]).unwrap();
        // Zero estimate should not trigger partial detection
        let result = check_overwrite(&path, false, Some(0)).unwrap();
        assert!(matches!(result, OverwriteAction::Skip(1024)));
        assert!(path.exists());
    }

    #[test]
    fn test_prepare_remux_options_no_mount() {
        let playlist = crate::types::Playlist {
            num: "00001".into(),
            duration: "1:23:45".into(),
            seconds: 5025,
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = prepare_remux_options(&playlist, None, cancel, 500, None);
        assert!(opts.chapters.is_empty());
        assert_eq!(opts.reserve_index_space_kb, 500);
        assert!(matches!(opts.stream_selection, StreamSelection::All));
    }

    #[test]
    fn test_prepare_remux_options_bad_mount_swallows_error() {
        let playlist = crate::types::Playlist {
            num: "00001".into(),
            duration: "1:23:45".into(),
            seconds: 5025,
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let opts = prepare_remux_options(&playlist, Some("/nonexistent/mount"), cancel, 500, None);
        assert!(opts.chapters.is_empty());
    }

    #[test]
    fn test_build_output_filename_tv_default() {
        let pl = crate::types::Playlist {
            num: "00001".into(),
            duration: "0:45:00".into(),
            seconds: 2700,
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
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
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
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
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
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
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
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
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
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
            true,
            false,
            Some("Game of Thrones"),
            Some(3),
            &[crate::types::Episode {
                episode_number: 9,
                name: "The Rains of Castamere".into(),
                runtime: None,
            }],
            None,
            Some("2013-06-02"),
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
            true,
            false,
            Some("Show"),
            Some(1),
            &[
                crate::types::Episode {
                    episode_number: 3,
                    name: "Ep Three".into(),
                    runtime: None,
                },
                crate::types::Episode {
                    episode_number: 4,
                    name: "Ep Four".into(),
                    runtime: None,
                },
            ],
            None,
            None,
            &HashMap::new(),
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Ep Three / Ep Four");
        assert_eq!(meta.tags["EPISODE_SORT"], "3");
    }

    #[test]
    fn test_build_metadata_movie() {
        let meta = build_metadata(
            true,
            true,
            None,
            None,
            &[],
            Some("Blade Runner 2049"),
            Some("2017-10-06"),
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
            true,
            false,
            Some("Manual Title"),
            None,
            &[],
            None,
            None,
            &HashMap::new(),
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
            true,
            false,
            Some("Show"),
            Some(1),
            &[crate::types::Episode {
                episode_number: 1,
                name: "Pilot".into(),
                runtime: None,
            }],
            None,
            None,
            &custom,
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
            true,
            false,
            Some("Show"),
            Some(1),
            &[crate::types::Episode {
                episode_number: 1,
                name: "Auto Title".into(),
                runtime: None,
            }],
            None,
            None,
            &custom,
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Custom Title");
    }

    #[test]
    fn test_build_metadata_disabled() {
        let meta = build_metadata(
            false,
            false,
            Some("Show"),
            Some(1),
            &[crate::types::Episode {
                episode_number: 1,
                name: "Pilot".into(),
                runtime: None,
            }],
            None,
            None,
            &HashMap::new(),
        );
        assert!(meta.is_none());
    }

    #[test]
    fn test_build_metadata_no_empty_strings() {
        let meta = build_metadata(
            true,
            false,
            Some("Show"),
            Some(1),
            &[crate::types::Episode {
                episode_number: 1,
                name: String::new(),
                runtime: None,
            }],
            None,
            None,
            &HashMap::new(),
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Show");
        for (k, v) in &meta.tags {
            assert!(!v.is_empty(), "Tag {} has empty value", k);
        }
    }
}
