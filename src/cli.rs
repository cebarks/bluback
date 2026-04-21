// TODO(multi-drive): Add concurrent CLI support with interleaved output
// and drive-prefixed progress lines (e.g., [sr0] Ripping playlist 1...)
// For now, CLI mode is single-drive only.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::disc;
use crate::rip;
use crate::tmdb;
use crate::types::*;
use crate::util::{self, *};
use crate::workflow;
use crate::Args;

fn format_video_column(info: &crate::types::MediaInfo) -> String {
    let mut parts = Vec::new();
    if !info.codec.is_empty() {
        let codec = match info.codec.as_str() {
            "h264" => "H.264",
            "hevc" => "HEVC",
            "vc1" => "VC-1",
            "mpeg2video" => "MPEG-2",
            other => other,
        };
        parts.push(codec.to_string());
    }
    if !info.resolution.is_empty() {
        parts.push(info.resolution.clone());
    }
    if !info.framerate.is_empty() {
        parts.push(info.framerate.clone());
    }
    parts.join(" ")
}

fn format_audio_column(streams: &[crate::types::AudioStream]) -> String {
    streams
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let codec = s.profile.as_deref().unwrap_or(&s.codec);
            format!("a{}:{} {}", i, codec, s.channel_layout)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_subtitle_column(streams: &[crate::types::SubtitleStream]) -> String {
    streams
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let lang = s.language.as_deref().unwrap_or("und");
            let forced = if s.forced { " FORCED" } else { "" };
            format!("s{}:{}{}", i, lang, forced)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

struct TmdbContext {
    tmdb_id: Option<u64>,
    episode_assignments: EpisodeAssignments,
    episodes: Vec<Episode>,
    start_episode: u32,
    season_num: Option<u32>,
    movie_title: Option<(String, String)>,
    show_name: Option<String>,
    date_released: Option<String>,
}

pub fn list_playlists(args: &Args, config: &crate::config::Config) -> anyhow::Result<()> {
    let device = args.device().to_string_lossy();

    if !args.device().exists() {
        anyhow::bail!("No Blu-ray device found at {}", device);
    }

    if config.should_max_speed(args.no_max_speed) {
        disc::set_max_speed(&device);
    }

    let label = disc::get_volume_label(&device);
    if !label.is_empty() {
        println!("Volume label: {}", label);
    }

    let min_probe_duration = config.min_probe_duration(args.min_probe_duration);

    // Resolve auto_detect setting
    let auto_detect = config.should_auto_detect(if args.auto_detect {
        Some(true)
    } else if args.no_auto_detect {
        Some(false)
    } else {
        None
    });

    eprint!("Scanning disc at {}...", device);
    let (mut playlists, probe_cache, skip_set) = crate::media::scan_playlists_with_progress(
        &device,
        min_probe_duration,
        auto_detect,
        Some(&|elapsed, timeout| {
            eprint!(
                "\rScanning disc at {} (AACS negotiation {}s/{}s)...",
                device, elapsed, timeout
            );
        }),
        Some(&|current, total, num| {
            eprint!("\rProbing playlist {} ({}/{})...", num, current, total);
        }),
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;
    println!();
    if playlists.is_empty() {
        anyhow::bail!("No playlists found. Check libaacs and KEYDB.cfg.");
    }

    // Mount disc for chapter counts, clip sizes, and title order
    let (chapter_counts, _clip_sizes, title_order) = {
        let device_str = device.to_string();
        match disc::ensure_mounted(&device_str) {
            Ok((mount, did_mount)) => {
                let mount_path = std::path::Path::new(&mount);
                let nums: Vec<&str> = playlists.iter().map(|pl| pl.num.as_str()).collect();
                let mpls_info = crate::chapters::collect_mpls_info(mount_path, &nums);
                let counts: std::collections::HashMap<String, usize> = mpls_info
                    .iter()
                    .map(|(k, v)| (k.clone(), v.chapters.len()))
                    .collect();
                let sizes: std::collections::HashMap<String, u64> = mpls_info
                    .into_iter()
                    .map(|(k, v)| (k, v.clip_size))
                    .collect();
                let order = crate::index::parse_title_order(mount_path);
                if did_mount {
                    let _ = disc::unmount_disc(&device_str);
                }
                (counts, sizes, order)
            }
            Err(_) => (
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
                None,
            ),
        }
    };

    // Reorder playlists by title index (or MPLS number fallback)
    crate::index::reorder_playlists(&mut playlists, title_order.as_deref());

    // Resolve auto_detect setting
    let auto_detect = config.should_auto_detect(if args.auto_detect {
        Some(true)
    } else if args.no_auto_detect {
        Some(false)
    } else {
        None
    });

    // Run detection if enabled (only on probed playlists)
    let mut detection_results = if auto_detect {
        let probed_playlists: Vec<Playlist> = playlists
            .iter()
            .filter(|pl| pl.seconds >= min_probe_duration && !skip_set.contains(&pl.num))
            .cloned()
            .collect();
        let mut results = crate::detection::run_detection_with_chapters(
            &probed_playlists,
            None, // No TMDb context in --list-playlists
            None,
            &chapter_counts,
        );

        // Add pre-classified specials as high-confidence detection results
        for pl in &playlists {
            if skip_set.contains(&pl.num) {
                results.push(crate::detection::DetectionResult {
                    playlist_num: pl.num.clone(),
                    suggested_type: crate::detection::SuggestedType::Special,
                    confidence: crate::detection::Confidence::High,
                    reasons: vec!["Pre-classified: duration < 50% of median".into()],
                });
            }
        }

        results
    } else {
        Vec::new()
    };

    let has_ch = !chapter_counts.is_empty();
    let header_ch = if has_ch { "  Ch" } else { "" };
    let header_det = if auto_detect { "  Det  " } else { "" };

    // Build filtered index mapping: episode-length playlists get sequential numbers
    let mut filtered_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut sel_idx = 1usize;
    for pl in &playlists {
        if pl.seconds >= min_probe_duration {
            filtered_index.insert(pl.num.clone(), sel_idx);
            sel_idx += 1;
        }
    }

    // Verbose mode: use cached probe results, only probe on the spot if not cached
    let verbose_info: Vec<Option<(crate::types::MediaInfo, crate::types::StreamInfo)>> =
        if args.verbose {
            playlists
                .iter()
                .map(|pl| {
                    probe_cache
                        .get(&pl.num)
                        .cloned()
                        .or_else(|| crate::media::probe::probe_playlist(&device, &pl.num).ok())
                })
                .collect()
        } else {
            vec![None; playlists.len()]
        };

    // Re-run detection with full probe data when verbose mode probed pre-classified specials
    if auto_detect && args.verbose && !skip_set.is_empty() {
        // Build full playlist set from verbose probe results
        let all_probed: Vec<Playlist> = playlists
            .iter()
            .enumerate()
            .filter(|(i, _)| verbose_info[*i].is_some())
            .map(|(_, pl)| pl.clone())
            .collect();
        detection_results =
            crate::detection::run_detection_with_chapters(&all_probed, None, None, &chapter_counts);
        // Re-add pre-classified entries for any that detection didn't already cover
        // (in case pre-classified playlists failed to probe in verbose mode)
        for pl in &playlists {
            if skip_set.contains(&pl.num)
                && !detection_results.iter().any(|d| d.playlist_num == pl.num)
            {
                detection_results.push(crate::detection::DetectionResult {
                    playlist_num: pl.num.clone(),
                    suggested_type: crate::detection::SuggestedType::Special,
                    confidence: crate::detection::Confidence::High,
                    reasons: vec!["Pre-classified: duration < 50% of median".into()],
                });
            }
        }
    }

    if args.verbose {
        println!(
            "  {:<4}  {:<10}  {:<10}{}{}  {:<18}  Audio  Subtitles  Sel",
            "#", "Playlist", "Duration", header_ch, header_det, "Video"
        );
        println!(
            "  {:<4}  {:<10}  {:<10}{}{}  {:<18}  -----  ---------  ---",
            "---",
            "--------",
            "--------",
            if has_ch { "  --" } else { "" },
            if auto_detect { "  ---  " } else { "" },
            "------------------"
        );
    } else {
        println!(
            "  {:<4}  {:<10}  {:<10}{}{}  Sel",
            "#", "Playlist", "Duration", header_ch, header_det
        );
        println!(
            "  {:<4}  {:<10}  {:<10}{}{}  ---",
            "---",
            "--------",
            "--------",
            if has_ch { "  --" } else { "" },
            if auto_detect { "  ---  " } else { "" },
        );
    }

    for (i, pl) in playlists.iter().enumerate() {
        let ch_str = if has_ch {
            format!(
                "  {:<2}",
                chapter_counts
                    .get(&pl.num)
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            )
        } else {
            String::new()
        };

        let sel_str = if let Some(idx) = filtered_index.get(&pl.num) {
            format!("  {}", idx)
        } else {
            "  *".to_string()
        };

        let det_str = if auto_detect {
            detection_results
                .iter()
                .find(|d| d.playlist_num == pl.num)
                .map(|d| match (d.suggested_type, d.confidence) {
                    (
                        crate::detection::SuggestedType::Special,
                        crate::detection::Confidence::High,
                    ) => " [S!] ",
                    (
                        crate::detection::SuggestedType::Special,
                        crate::detection::Confidence::Medium,
                    ) => " [S?] ",
                    (
                        crate::detection::SuggestedType::Special,
                        crate::detection::Confidence::Low,
                    ) => " [s.] ",
                    (crate::detection::SuggestedType::MultiEpisode, _) => " [M!] ",
                    _ => "      ",
                })
                .unwrap_or("      ")
        } else {
            ""
        };

        if args.verbose {
            let (video_str, audio_str, sub_str) =
                if let Some((ref media, ref streams)) = verbose_info[i] {
                    (
                        format_video_column(media),
                        format_audio_column(&streams.audio_streams),
                        format_subtitle_column(&streams.subtitle_streams),
                    )
                } else {
                    ("".to_string(), "".to_string(), "".to_string())
                };

            println!(
                "  {:<4}  {:<10}  {:<10}{}{}  {:<18}  {}  {}{}",
                i + 1,
                pl.num,
                pl.duration,
                ch_str,
                det_str,
                video_str,
                audio_str,
                sub_str,
                sel_str,
            );
        } else {
            println!(
                "  {:<4}  {:<10}  {:<10}{}{}{}",
                i + 1,
                pl.num,
                pl.duration,
                ch_str,
                det_str,
                sel_str,
            );
        }
    }

    let episode_count = filtered_index.len();
    let short_count = playlists.len() - episode_count;
    println!(
        "\n  {} playlists ({} episode-length, {} short/extras)",
        playlists.len(),
        episode_count,
        short_count,
    );
    println!("  * = below min_probe_duration ({}s)", min_probe_duration);

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    args: &Args,
    config: &crate::config::Config,
    headless: bool,
    stream_filter: &crate::streams::StreamFilter,
    tracks_spec: Option<&str>,
    start_episode_override: Option<u32>,
    skip_eject: bool,
    history: Option<&crate::history::HistoryDb>,
    ignore_history: bool,
    batch_id: Option<&str>,
) -> anyhow::Result<(u32, u32)> {
    let device = args.device().to_string_lossy();

    let (
        label,
        label_info,
        mut all_playlists,
        mut movie_mode,
        probe_cache,
        min_probe_duration,
        skip_set,
    ) = scan_disc(args, config)?;

    // Build episodes_pl from duration threshold (episode-length playlists in disc order)
    let mut episodes_pl: Vec<Playlist> = all_playlists
        .iter()
        .filter(|pl| pl.seconds >= min_probe_duration && !skip_set.contains(&pl.num))
        .cloned()
        .collect();

    // Duplicate detection: check if this disc was previously ripped
    // Only skip if fully completed (all files succeeded). Partial sessions
    // (some files failed/skipped) should not block re-ripping.
    if let (Some(db), false) = (history, ignore_history) {
        if let Ok(matches) = db.find_session_by_label(&label) {
            let fully_completed = matches.iter().find(|s| {
                s.status == crate::history::SessionStatus::Completed
                    && s.files_completed >= s.files_total
                    && s.files_total > 0
            });
            if let Some(prev) = fully_completed {
                let date = prev.started_at.get(..10).unwrap_or(&prev.started_at);
                if headless {
                    eprintln!(
                        "history: {} was previously ripped on {}, skipping",
                        label, date
                    );
                    return Ok((0, 0));
                } else {
                    eprint!(
                        "Disc {} was previously ripped ({}). Continue? [y/N] ",
                        label, date
                    );
                    io::stdout().flush()?;
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    if !input.trim().eq_ignore_ascii_case("y") {
                        return Ok((0, 0));
                    }
                }
            }
        }
    }

    // Mount disc for chapter counts, clip sizes, and title order
    let (chapter_counts, clip_sizes, title_order) = {
        let device_str = device.to_string();
        match disc::ensure_mounted(&device_str) {
            Ok((mount, did_mount)) => {
                let mount_path = std::path::Path::new(&mount);
                let nums: Vec<&str> = all_playlists.iter().map(|pl| pl.num.as_str()).collect();
                let mpls_info = crate::chapters::collect_mpls_info(mount_path, &nums);
                let counts: std::collections::HashMap<String, usize> = mpls_info
                    .iter()
                    .map(|(k, v)| (k.clone(), v.chapters.len()))
                    .collect();
                let sizes: std::collections::HashMap<String, u64> = mpls_info
                    .into_iter()
                    .map(|(k, v)| (k, v.clip_size))
                    .collect();
                let order = crate::index::parse_title_order(mount_path);
                if did_mount {
                    let _ = disc::unmount_disc(&device_str);
                }
                (counts, sizes, order)
            }
            Err(_) => (
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
                None,
            ),
        }
    };

    // Reorder playlists by title index (or MPLS number fallback)
    crate::index::reorder_playlists(&mut all_playlists, title_order.as_deref());
    crate::index::reorder_playlists(&mut episodes_pl, title_order.as_deref());

    // Detection-aware movie_mode override (mirrors TUI logic in session.rs)
    if !args.movie {
        let auto_detect = config.should_auto_detect(if args.auto_detect {
            Some(true)
        } else if args.no_auto_detect {
            Some(false)
        } else {
            None
        });
        if auto_detect {
            let probed_playlists: Vec<&Playlist> = all_playlists
                .iter()
                .filter(|pl| pl.seconds >= min_probe_duration && !skip_set.contains(&pl.num))
                .collect();
            let early_detection = crate::detection::run_detection_with_chapters(
                &probed_playlists
                    .iter()
                    .map(|p| (*p).clone())
                    .collect::<Vec<_>>(),
                None, // no TMDb episodes yet
                None,
                &chapter_counts,
            );
            let episode_count = early_detection
                .iter()
                .filter(|d| {
                    matches!(
                        d.suggested_type,
                        crate::detection::SuggestedType::Episode
                            | crate::detection::SuggestedType::MultiEpisode
                    )
                })
                .count();
            let new_movie_mode = episode_count == 1 && args.season.is_none();
            if new_movie_mode != movie_mode {
                movie_mode = new_movie_mode;
                if movie_mode {
                    println!("  (Single episode detected — using movie mode. Use --season to force TV mode.)");
                }
            }
        }
    }

    // Episode continuation: use history to determine start episode if no explicit override
    let effective_start_override = if start_episode_override.is_some() || ignore_history {
        start_episode_override
    } else if let Some(db) = history {
        // Try TMDb-based continuation first (requires season to be known at this point)
        // Season may come from --season flag or volume label parsing
        let season = args.season.or(label_info.as_ref().map(|l| l.season));
        if let Some(season) = season {
            // We don't have tmdb_id yet (it comes from TMDb lookup), so use label-based lookup
            let label_pattern = label_info
                .as_ref()
                .map(|l| format!("{}%", l.show.replace(' ', "_").to_uppercase()))
                .unwrap_or_else(|| label.clone());
            if let Ok(Some(last)) = db.last_episode_by_label(&label_pattern, season as i32) {
                let next = last as u32 + 1;
                eprintln!("history: continuing from E{:02} (last ripped)", last);
                Some(next)
            } else {
                start_episode_override
            }
        } else {
            start_episode_override
        }
    } else {
        start_episode_override
    };

    let mut tmdb_ctx = lookup_tmdb(
        args,
        config,
        &label_info,
        &episodes_pl,
        movie_mode,
        headless,
        effective_start_override,
    )?;

    let selected = display_and_select(
        &episodes_pl,
        &tmdb_ctx.episode_assignments,
        tmdb_ctx.season_num,
        &chapter_counts,
        args.playlists.as_deref(),
        headless,
    )?;

    // Resolve auto_detect setting
    let auto_detect = config.should_auto_detect(if args.auto_detect {
        Some(true)
    } else if args.no_auto_detect {
        Some(false)
    } else {
        None
    });

    // Resolve --specials flag to playlist numbers, or run auto-detection
    let specials_set: std::collections::HashSet<String> =
        if (auto_detect || args.hide_specials) && !movie_mode && args.specials.is_none() {
            // Extract TMDb episodes from episode_assignments for detection
            let tmdb_episodes: Vec<Episode> = tmdb_ctx
                .episode_assignments
                .values()
                .flatten()
                .cloned()
                .collect();

            let probed_playlists: Vec<Playlist> = all_playlists
                .iter()
                .filter(|pl| pl.seconds >= min_probe_duration && !skip_set.contains(&pl.num))
                .cloned()
                .collect();
            let detection_results = crate::detection::run_detection_with_chapters(
                &probed_playlists,
                if tmdb_episodes.is_empty() {
                    None
                } else {
                    Some(&tmdb_episodes)
                },
                None, // CLI doesn't fetch season 0 (TUI-only feature for now)
                &chapter_counts,
            );

            let mut specials = std::collections::HashSet::new();

            if args.hide_specials {
                // --hide-specials: collect all detected specials (any confidence)
                for det in &detection_results {
                    if det.suggested_type == crate::detection::SuggestedType::Special {
                        eprintln!(
                            "Hiding special: playlist {} ({})",
                            det.playlist_num,
                            det.reasons.join(", ")
                        );
                        specials.insert(det.playlist_num.clone());
                    }
                }
            } else if headless {
                // Auto-apply high-confidence specials only
                for det in &detection_results {
                    if det.suggested_type == crate::detection::SuggestedType::Special
                        && det.confidence == crate::detection::Confidence::High
                    {
                        eprintln!(
                            "Auto-detected: playlist {} as special ({})",
                            det.playlist_num,
                            det.reasons.join(", ")
                        );
                        specials.insert(det.playlist_num.clone());
                    }
                }
            } else {
                // Interactive: show suggestions and prompt
                let suggestions: Vec<&crate::detection::DetectionResult> = detection_results
                    .iter()
                    .filter(|d| {
                        d.suggested_type == crate::detection::SuggestedType::Special
                            && d.confidence >= crate::detection::Confidence::Medium
                    })
                    .collect();

                if !suggestions.is_empty() {
                    println!("\nAuto-detected specials:");
                    for det in &suggestions {
                        let indicator = match det.confidence {
                            crate::detection::Confidence::High => "[S!]",
                            crate::detection::Confidence::Medium => "[S?]",
                            _ => "[s.]",
                        };
                        println!(
                            "  {} playlist {} — {}",
                            indicator,
                            det.playlist_num,
                            det.reasons.join(", ")
                        );
                    }
                    print!("Accept auto-detected specials? [Y/n/edit]: ");
                    io::stdout().flush()?;

                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    let input = input.trim().to_lowercase();

                    match input.as_str() {
                        "" | "y" | "yes" => {
                            for det in &suggestions {
                                specials.insert(det.playlist_num.clone());
                            }
                        }
                        "edit" => {
                            // Fall through - user will need to use --specials manually
                            eprintln!("Use --specials to manually specify specials.");
                        }
                        _ => {} // "n" or anything else - skip
                    }
                }
            }

            specials
        } else if let Some(ref sel_str) = args.specials {
            match parse_selection(sel_str, episodes_pl.len()) {
                Some(indices) => {
                    let selected_set: std::collections::HashSet<usize> =
                        selected.iter().copied().collect();
                    let mut specials = std::collections::HashSet::new();
                    for idx in indices {
                        if selected_set.contains(&idx) {
                            specials.insert(episodes_pl[idx].num.clone());
                        } else {
                            log::warn!(
                                "--specials index {} is not in the selected playlists, skipping",
                                idx + 1
                            );
                        }
                    }
                    specials
                }
                None => {
                    anyhow::bail!(
                        "Invalid --specials value '{}'. Use e.g. '4,5', '4-5' (max {}).",
                        sel_str,
                        episodes_pl.len()
                    );
                }
            }
        } else {
            std::collections::HashSet::new()
        };

    // Reassign regular episodes after specials are determined
    if !specials_set.is_empty() && !movie_mode {
        let non_special_pl: Vec<Playlist> = episodes_pl
            .iter()
            .filter(|pl| !specials_set.contains(&pl.num))
            .cloned()
            .collect();
        tmdb_ctx.episode_assignments = crate::util::assign_episodes(
            &non_special_pl,
            &tmdb_ctx.episodes,
            tmdb_ctx.start_episode,
        );
    }

    // --hide-specials: exclude detected specials from the rip set
    let selected = if args.hide_specials && !specials_set.is_empty() {
        let filtered: Vec<usize> = selected
            .into_iter()
            .filter(|&idx| !specials_set.contains(&episodes_pl[idx].num))
            .collect();
        if filtered.is_empty() {
            anyhow::bail!("No playlists remaining after hiding specials.");
        }
        filtered
    } else {
        selected
    };

    let outfiles = build_filenames(
        args,
        config,
        &label,
        &label_info,
        &episodes_pl,
        &selected,
        &tmdb_ctx,
        &specials_set,
        movie_mode,
        headless,
        &probe_cache,
    )?;

    let metadata_enabled = config.metadata_enabled() && !args.no_metadata;
    let custom_tags = config.metadata_tags();
    let metadata_per_playlist: Vec<Option<crate::types::MkvMetadata>> = selected
        .iter()
        .map(|&idx| {
            let pl = &episodes_pl[idx];
            let episodes = tmdb_ctx
                .episode_assignments
                .get(&pl.num)
                .cloned()
                .unwrap_or_default();
            crate::workflow::build_metadata(
                metadata_enabled,
                movie_mode,
                tmdb_ctx.show_name.as_deref(),
                tmdb_ctx.season_num,
                &episodes,
                tmdb_ctx.movie_title.as_ref().map(|(t, _)| t.as_str()),
                tmdb_ctx.date_released.as_deref(),
                &custom_tags,
            )
        })
        .collect();

    // Record history session (after TMDb lookup, before ripping)
    let session_id = if let Some(db) = history {
        let title = if movie_mode {
            tmdb_ctx
                .movie_title
                .as_ref()
                .map(|(t, _)| t.clone())
                .unwrap_or_else(|| label.clone())
        } else {
            tmdb_ctx.show_name.clone().unwrap_or_else(|| label.clone())
        };
        let info = crate::history::SessionInfo {
            volume_label: label.clone(),
            device: Some(device.to_string()),
            tmdb_id: tmdb_ctx.tmdb_id.map(|id| id as i64),
            tmdb_type: if movie_mode {
                Some("movie".into())
            } else {
                Some("tv".into())
            },
            title,
            season: tmdb_ctx.season_num.map(|s| s as i32),
            disc_number: label_info.as_ref().map(|l| l.disc as i32),
            batch_id: batch_id.map(|s| s.to_string()),
            config_snapshot: Some(
                serde_json::to_string(&crate::history::ConfigSnapshot::from_config(config))
                    .unwrap_or_default(),
            ),
        };
        match db.start_session(&info) {
            Ok(id) => {
                // Record disc playlists
                let min_dur = config.min_probe_duration(args.min_probe_duration);
                let playlist_infos: Vec<crate::history::DiscPlaylistInfo> = all_playlists
                    .iter()
                    .map(|pl| crate::history::DiscPlaylistInfo {
                        playlist: pl.num.clone(),
                        duration_ms: Some((pl.seconds as i64) * 1000),
                        video_streams: Some(pl.video_streams as i32),
                        audio_streams: Some(pl.audio_streams as i32),
                        subtitle_streams: Some(pl.subtitle_streams as i32),
                        chapters: chapter_counts.get(&pl.num).map(|&c| c as i32),
                        is_filtered: pl.seconds < min_dur,
                    })
                    .collect();
                if let Err(e) = db.record_disc_playlists(id, &playlist_infos) {
                    log::warn!("history: failed to record playlists: {}", e);
                }
                Some(id)
            }
            Err(e) => {
                log::warn!("history: failed to start session: {}", e);
                None
            }
        }
    } else {
        None
    };

    let (success_count, fail_count) = rip_selected(
        args,
        config,
        &device,
        &episodes_pl,
        &selected,
        &outfiles,
        &metadata_per_playlist,
        args.no_hooks,
        &label,
        &tmdb_ctx,
        stream_filter,
        tracks_spec,
        skip_eject,
        &probe_cache,
        &clip_sizes,
        movie_mode,
        label_info.as_ref(),
        &specials_set,
        history,
        session_id,
        label_info.as_ref().map(|l| l.disc),
    )?;

    // Finish history session
    if let (Some(db), Some(sid)) = (history, session_id) {
        let status = if success_count > 0 {
            crate::history::SessionStatus::Completed
        } else {
            crate::history::SessionStatus::Failed
        };
        if let Err(e) = db.finish_session(sid, status) {
            log::warn!("history: failed to finish session: {}", e);
        }
    }

    let regular_episodes =
        crate::util::count_assigned_episodes(&tmdb_ctx.episode_assignments, &specials_set);

    if fail_count > 0 && start_episode_override.is_none() {
        anyhow::bail!(
            "{} of {} playlist(s) failed to rip",
            fail_count,
            selected.len()
        );
    }

    Ok((success_count, regular_episodes))
}

#[allow(clippy::type_complexity)] // tuple is clear in context; a struct for one call site is over-engineering
fn scan_disc(
    args: &Args,
    config: &crate::config::Config,
) -> anyhow::Result<(
    String,
    Option<LabelInfo>,
    Vec<Playlist>,
    bool,
    crate::types::ProbeCache,
    u32,
    std::collections::HashSet<String>,
)> {
    let device = args.device().to_string_lossy();

    if !args.device().exists() {
        anyhow::bail!("No Blu-ray device found at {}", device);
    }

    if config.should_max_speed(args.no_max_speed) {
        disc::set_max_speed(&device);
    }

    let label = disc::get_volume_label(&device);
    let label_info = disc::parse_volume_label(&label);
    if !label.is_empty() {
        println!("Volume label: {}", label);
    }

    let min_probe_duration = config.min_probe_duration(args.min_probe_duration);

    // Resolve auto_detect setting
    let auto_detect = config.should_auto_detect(if args.auto_detect {
        Some(true)
    } else if args.no_auto_detect {
        Some(false)
    } else {
        None
    });

    eprint!("Scanning disc at {}...", device);
    let (playlists, probe_cache, skip_set) = crate::media::scan_playlists_with_progress(
        &device,
        min_probe_duration,
        auto_detect,
        Some(&|elapsed, timeout| {
            eprint!(
                "\rScanning disc at {} (AACS negotiation {}s/{}s)...",
                device, elapsed, timeout
            );
        }),
        Some(&|current, total, num| {
            eprint!("\rProbing playlist {} ({}/{})...", num, current, total);
        }),
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;
    println!();
    if playlists.is_empty() {
        anyhow::bail!("No playlists found. Check libaacs and KEYDB.cfg.");
    }

    let probed_count = playlists
        .iter()
        .filter(|pl| pl.seconds >= min_probe_duration)
        .count();
    let short_count = playlists.len() - probed_count;
    println!(
        "Found {} playlists ({} episode-length, {} short/extras).\n",
        playlists.len(),
        probed_count,
        short_count
    );

    if probed_count == 0 {
        anyhow::bail!("No episode-length playlists found. Try lowering --min-probe-duration.");
    }

    let movie_mode = args.movie || (probed_count == 1 && args.season.is_none());
    if movie_mode && !args.movie {
        println!("  (Single playlist detected — using movie mode. Use --season to force TV mode.)");
    }

    Ok((
        label,
        label_info,
        playlists,
        movie_mode,
        probe_cache,
        min_probe_duration,
        skip_set,
    ))
}

fn lookup_tmdb(
    args: &Args,
    config: &crate::config::Config,
    label_info: &Option<LabelInfo>,
    episodes_pl: &[Playlist],
    movie_mode: bool,
    headless: bool,
    start_episode_override: Option<u32>,
) -> anyhow::Result<TmdbContext> {
    let mut ctx = TmdbContext {
        tmdb_id: None,
        episode_assignments: HashMap::new(),
        episodes: Vec::new(),
        start_episode: 1,
        season_num: args.season.or(label_info.as_ref().map(|l| l.season)),
        movie_title: None,
        show_name: None,
        date_released: None,
    };

    // --title: skip TMDb entirely, use the provided title directly
    if let Some(ref title) = args.title {
        if movie_mode {
            ctx.movie_title = Some((title.clone(), args.year.clone().unwrap_or_default()));
            return Ok(ctx);
        } else {
            ctx.show_name = Some(title.clone());
            let season = match ctx.season_num {
                Some(s) => s,
                None if headless => {
                    anyhow::bail!(
                        "Cannot determine season number in headless mode. Use --season <NUM>."
                    );
                }
                None => prompt_number("  Season number: ", None)?,
            };
            ctx.season_num = Some(season);

            let disc_number = label_info.as_ref().map(|l| l.disc);
            let effective_start = start_episode_override.or(args.start_episode);
            let default_start = effective_start
                .unwrap_or_else(|| guess_start_episode(disc_number, episodes_pl.len()));
            let start_ep = if effective_start.is_none() && !headless {
                prompt_number(
                    &format!("  Starting episode number [{}]: ", default_start),
                    Some(default_start),
                )?
            } else {
                default_start
            };

            // Extra synthetic episodes for multi-episode detection in assign_episodes
            let synthetic_count = episodes_pl.len() * 2;
            let synthetic_episodes: Vec<Episode> = (start_ep..start_ep + synthetic_count as u32)
                .map(|n| Episode {
                    episode_number: n,
                    name: String::new(),
                    runtime: None,
                })
                .collect();

            ctx.episodes = synthetic_episodes.clone();
            ctx.start_episode = start_ep;
            ctx.episode_assignments = assign_episodes(episodes_pl, &synthetic_episodes, start_ep);
            return Ok(ctx);
        }
    }

    let mut api_key = tmdb::get_api_key(config);

    if api_key.is_none() && !headless {
        let input = prompt("TMDb API key not found. Enter key (or Enter to skip): ")?;
        if !input.is_empty() {
            tmdb::save_api_key(&input)?;
            println!("  Saved API key.");
            api_key = Some(input);
        }
    }

    if let Some(ref key) = api_key {
        let default_query = label_info.as_ref().map(|l| l.show.as_str()).unwrap_or("");

        if movie_mode {
            let movie_result = if headless {
                headless_tmdb_movie(key, default_query)?
            } else {
                prompt_tmdb_movie(key, default_query)?
            };
            if let Some((id, title, year)) = movie_result {
                ctx.tmdb_id = Some(id);
                if !year.is_empty() {
                    ctx.date_released = Some(year.clone());
                }
                ctx.movie_title = Some((title, year));
            }
        } else {
            if api_key.is_none() && (args.season.is_some() || args.start_episode.is_some()) {
                println!("Warning: --season/--start-episode require TMDb. Ignoring.");
            }

            let cli_season = args.season.or(label_info.as_ref().map(|l| l.season));

            let lookup = if headless {
                headless_tmdb_tv(key, default_query, cli_season)?
            } else {
                prompt_tmdb(key, default_query, cli_season)?
            };

            if let Some(lookup) = lookup {
                ctx.tmdb_id = Some(lookup.tmdb_id);
                ctx.season_num = Some(lookup.season);
                ctx.show_name = Some(lookup.show_name);
                ctx.date_released = lookup.first_air_date;

                let disc_number = label_info.as_ref().map(|l| l.disc);
                let effective_start = start_episode_override.or(args.start_episode);
                let default_start = effective_start
                    .unwrap_or_else(|| guess_start_episode(disc_number, episodes_pl.len()));

                let start_ep = if effective_start.is_none() && !headless {
                    prompt_number(
                        &format!("  Starting episode number [{}]: ", default_start),
                        Some(default_start),
                    )?
                } else {
                    default_start
                };

                ctx.episodes = lookup.episodes.clone();
                ctx.start_episode = start_ep;
                ctx.episode_assignments = assign_episodes(episodes_pl, &lookup.episodes, start_ep);

                // Show mappings and prompt for accept/manual (interactive only)
                if !headless {
                    loop {
                        println!("\n  Episode Mappings:");
                        for pl in episodes_pl.iter() {
                            let ep_str = if let Some(eps) = ctx.episode_assignments.get(&pl.num) {
                                eps.iter()
                                    .map(|e| {
                                        if e.name.is_empty() {
                                            format!("E{:02}", e.episode_number)
                                        } else {
                                            format!(
                                                "S{:02}E{:02} - {}",
                                                ctx.season_num.unwrap_or(0),
                                                e.episode_number,
                                                e.name
                                            )
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            } else {
                                "(none)".to_string()
                            };
                            println!("    {} ({})  ->  {}", pl.num, pl.duration, ep_str);
                        }

                        let response = prompt("\n  Accept mappings? [Y/n/manual]: ")?;
                        if response.is_empty()
                            || response.eq_ignore_ascii_case("y")
                            || response.eq_ignore_ascii_case("yes")
                        {
                            break;
                        } else if response.eq_ignore_ascii_case("n") {
                            let new_start = prompt_number(
                                &format!("  Starting episode number [{}]: ", start_ep),
                                Some(start_ep),
                            )?;
                            ctx.start_episode = new_start;
                            ctx.episode_assignments =
                                assign_episodes(episodes_pl, &lookup.episodes, new_start);
                            continue;
                        } else if response.eq_ignore_ascii_case("manual") {
                            let ep_by_num: std::collections::HashMap<u32, &crate::types::Episode> =
                                lookup
                                    .episodes
                                    .iter()
                                    .map(|e| (e.episode_number, e))
                                    .collect();
                            for pl in episodes_pl.iter() {
                                let current = ctx
                                    .episode_assignments
                                    .get(&pl.num)
                                    .map(|eps| {
                                        eps.iter()
                                            .map(|e| e.episode_number.to_string())
                                            .collect::<Vec<_>>()
                                            .join(",")
                                    })
                                    .unwrap_or_default();
                                loop {
                                    let input = prompt(&format!(
                                        "  Playlist {} ({}) [{}]: ",
                                        pl.num, pl.duration, current
                                    ))?;
                                    let input = if input.is_empty() {
                                        current.clone()
                                    } else {
                                        input
                                    };
                                    match util::parse_episode_input(&input) {
                                        Some(ep_nums) if ep_nums.is_empty() => {
                                            ctx.episode_assignments.remove(&pl.num);
                                            break;
                                        }
                                        Some(ep_nums) => {
                                            let eps: Vec<crate::types::Episode> = ep_nums
                                                .iter()
                                                .map(|&num| {
                                                    ep_by_num
                                                        .get(&num)
                                                        .map(|e| (*e).clone())
                                                        .unwrap_or(crate::types::Episode {
                                                            episode_number: num,
                                                            name: String::new(),
                                                            runtime: None,
                                                        })
                                                })
                                                .collect();
                                            ctx.episode_assignments.insert(pl.num.clone(), eps);
                                            break;
                                        }
                                        None => {
                                            println!("  Invalid input. Use: 3, 3-4, or 3,5");
                                        }
                                    }
                                }
                            }
                            continue; // Loop back to show updated mappings
                        } else {
                            println!("  Invalid choice. Enter Y, n, or manual.");
                        }
                    }
                }
            }
        }
    }

    // Headless fallback: no TMDb data and no --title provided — use label info
    if headless && !movie_mode && ctx.episode_assignments.is_empty() && args.title.is_none() {
        ctx.show_name = label_info.as_ref().map(|l| l.show.clone());

        let season = match ctx.season_num {
            Some(s) => s,
            None => {
                anyhow::bail!(
                    "Cannot determine season number in headless mode. Use --season <NUM>."
                );
            }
        };
        ctx.season_num = Some(season);

        let disc_number = label_info.as_ref().map(|l| l.disc);
        let start_ep = start_episode_override
            .or(args.start_episode)
            .unwrap_or_else(|| guess_start_episode(disc_number, episodes_pl.len()));

        let synthetic_count = episodes_pl.len() * 2;
        let synthetic_episodes: Vec<Episode> = (start_ep..start_ep + synthetic_count as u32)
            .map(|n| Episode {
                episode_number: n,
                name: String::new(),
                runtime: None,
            })
            .collect();

        ctx.episodes = synthetic_episodes.clone();
        ctx.start_episode = start_ep;
        ctx.episode_assignments = assign_episodes(episodes_pl, &synthetic_episodes, start_ep);
    }

    Ok(ctx)
}

fn display_and_select(
    episodes_pl: &[Playlist],
    episode_assignments: &EpisodeAssignments,
    season_num: Option<u32>,
    chapter_counts: &std::collections::HashMap<String, usize>,
    playlists_flag: Option<&str>,
    headless: bool,
) -> anyhow::Result<Vec<usize>> {
    let has_eps = !episode_assignments.is_empty();
    let has_ch = !chapter_counts.is_empty();
    let header_ch = if has_ch { "  Ch" } else { "" };
    let header_ep = if has_eps { "  Episode" } else { "" };
    println!(
        "\n  {:<4}  {:<10}  {:<10}{}{}",
        "#", "Playlist", "Duration", header_ch, header_ep
    );
    println!(
        "  {:<4}  {:<10}  {:<10}{}{}",
        "---",
        "--------",
        "--------",
        if has_ch { "  --" } else { "" },
        "-".repeat(header_ep.len())
    );

    for (i, pl) in episodes_pl.iter().enumerate() {
        let ch_str = if has_ch {
            format!(
                "  {:<2}",
                chapter_counts
                    .get(&pl.num)
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            )
        } else {
            String::new()
        };
        let ep_str = if let Some(eps) = episode_assignments.get(&pl.num) {
            if eps.len() == 1 {
                format!(
                    "  S{:02}E{:02} - {}",
                    season_num.unwrap_or(0),
                    eps[0].episode_number,
                    eps[0].name
                )
            } else if eps.len() > 1 {
                let first = &eps[0];
                let last = &eps[eps.len() - 1];
                format!(
                    "  S{:02}E{:02}-E{:02} - {}",
                    season_num.unwrap_or(0),
                    first.episode_number,
                    last.episode_number,
                    first.name
                )
            } else {
                String::new()
            }
        } else if has_eps {
            "  (no episode data)".into()
        } else {
            String::new()
        };
        println!(
            "  {:<4}  {:<10}  {:<10}{}{}",
            i + 1,
            pl.num,
            pl.duration,
            ch_str,
            ep_str
        );
    }
    println!();

    // --playlists flag: resolve selection non-interactively
    if let Some(selection_str) = playlists_flag {
        match parse_selection(selection_str, episodes_pl.len()) {
            Some(sel) => return Ok(sel),
            None => anyhow::bail!(
                "Invalid --playlists value '{}'. Use e.g. '1,2,3', '1-3', or 'all' (max {}).",
                selection_str,
                episodes_pl.len()
            ),
        }
    }

    // Headless without explicit selection: rip all playlists
    if headless {
        return Ok((0..episodes_pl.len()).collect());
    }

    let selected = loop {
        let input = prompt("Select playlists to rip (e.g. 1,2,3 or 1-3 or 'all') [all]: ")?;
        let input = if input.is_empty() {
            "all".to_string()
        } else {
            input
        };
        if let Some(sel) = parse_selection(&input, episodes_pl.len()) {
            break sel;
        }
        println!("Invalid selection. Try again.");
    };

    println!();
    Ok(selected)
}

#[allow(clippy::too_many_arguments)]
fn build_filenames(
    args: &Args,
    config: &crate::config::Config,
    label: &str,
    label_info: &Option<LabelInfo>,
    episodes_pl: &[Playlist],
    selected: &[usize],
    tmdb_ctx: &TmdbContext,
    specials: &std::collections::HashSet<String>,
    movie_mode: bool,
    headless: bool,
    probe_cache: &crate::types::ProbeCache,
) -> anyhow::Result<Vec<PathBuf>> {
    let output_dir = args
        .output
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new("."));

    let show_name_str = if movie_mode {
        tmdb_ctx
            .movie_title
            .as_ref()
            .map(|(t, _)| t.clone())
            .unwrap_or_else(|| "Unknown".to_string())
    } else {
        tmdb_ctx.show_name.clone().unwrap_or_else(|| {
            label_info
                .as_ref()
                .map(|l| l.show.clone())
                .unwrap_or_else(|| "Unknown".to_string())
        })
    };

    // Determine if we need to probe media info for any playlists
    let use_custom_format = args.format.is_some()
        || args.format_preset.is_some()
        || config.tv_format.is_some()
        || config.movie_format.is_some()
        || config.preset.is_some();

    let mut special_ep_cursor = 1u32;

    let default_names: Vec<String> = selected
        .iter()
        .enumerate()
        .map(|(sel_i, &idx)| {
            let pl = &episodes_pl[idx];
            let is_special = specials.contains(&pl.num);

            // Use cached probe data for media info (format templates, specials)
            let media_info = if use_custom_format || is_special {
                probe_cache.get(&pl.num).map(|(m, _)| m.clone())
            } else {
                None
            };

            let part = if tmdb_ctx.movie_title.is_some() && selected.len() > 1 {
                Some(sel_i as u32 + 1)
            } else {
                None
            };

            let episodes = if is_special {
                let ep = Episode {
                    episode_number: special_ep_cursor,
                    name: String::new(),
                    runtime: None,
                };
                special_ep_cursor += 1;
                vec![ep]
            } else {
                tmdb_ctx
                    .episode_assignments
                    .get(&pl.num)
                    .cloned()
                    .unwrap_or_default()
            };

            workflow::build_output_filename(
                pl,
                &episodes,
                tmdb_ctx.season_num.unwrap_or(0),
                movie_mode,
                is_special,
                tmdb_ctx
                    .movie_title
                    .as_ref()
                    .map(|(t, y)| (t.as_str(), y.as_str())),
                &show_name_str,
                label,
                label_info.as_ref(),
                config,
                args.format.as_deref(),
                args.format_preset.as_deref(),
                media_info.as_ref(),
                part,
            )
        })
        .collect();

    println!("  Output filenames:");
    for (i, &idx) in selected.iter().enumerate() {
        let pl = &episodes_pl[idx];
        println!("    {} ({}) -> {}", pl.num, pl.duration, default_names[i]);
    }

    let mut outfiles: Vec<PathBuf> = Vec::new();
    if headless {
        for name in &default_names {
            outfiles.push(output_dir.join(name));
        }
    } else {
        let customize = prompt("\n  Customize filenames? [y/N]: ")?;
        if customize.eq_ignore_ascii_case("y") || customize.eq_ignore_ascii_case("yes") {
            for (i, &idx) in selected.iter().enumerate() {
                let pl = &episodes_pl[idx];
                let input = prompt(&format!(
                    "  Name for playlist {} [{}]: ",
                    pl.num, default_names[i]
                ))?;
                let name = if input.is_empty() {
                    default_names[i].clone()
                } else {
                    format!("{}.mkv", sanitize_filename(&input))
                };
                outfiles.push(output_dir.join(&name));
            }
        } else {
            for name in &default_names {
                outfiles.push(output_dir.join(name));
            }
        }
    }

    Ok(outfiles)
}

#[allow(clippy::too_many_arguments)]
fn rip_selected(
    args: &Args,
    config: &crate::config::Config,
    device: &str,
    episodes_pl: &[Playlist],
    selected: &[usize],
    outfiles: &[PathBuf],
    metadata_per_playlist: &[Option<crate::types::MkvMetadata>],
    no_hooks: bool,
    label: &str,
    tmdb_ctx: &TmdbContext,
    stream_filter: &crate::streams::StreamFilter,
    tracks_spec: Option<&str>,
    skip_eject: bool,
    probe_cache: &crate::types::ProbeCache,
    clip_sizes: &std::collections::HashMap<String, u64>,
    movie_mode: bool,
    label_info: Option<&LabelInfo>,
    specials: &std::collections::HashSet<String>,
    history: Option<&crate::history::HistoryDb>,
    session_id: Option<i64>,
    disc_number: Option<u32>,
) -> anyhow::Result<(u32, u32)> {
    let output_dir = args
        .output
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new("."));

    if args.dry_run {
        println!("\n[DRY RUN] Would rip:");
        for (i, &idx) in selected.iter().enumerate() {
            let pl = &episodes_pl[idx];
            println!(
                "  {} ({}) -> {}",
                pl.num,
                pl.duration,
                outfiles[i]
                    .file_name()
                    .expect("output path has filename")
                    .to_string_lossy()
            );
        }
        return Ok((0, 0));
    }

    // Always mount for chapter extraction (MountGuard ensures unmount on exit)
    let (mount_point, mut _mount_guard) = match disc::ensure_mounted(device) {
        Ok((mount, did_mount)) => (Some(mount), Some(disc::MountGuard::new(device, did_mount))),
        Err(e) => {
            println!(
                "Warning: could not mount disc for chapter extraction: {}",
                e
            );
            (None, None)
        }
    };

    // Create output directory and any template subdirectories
    for outfile in outfiles {
        if let Some(parent) = outfile.parent() {
            std::fs::create_dir_all(parent)?;
        }
    }

    // Mark session as in_progress
    if let (Some(db), Some(sid)) = (history, session_id) {
        if let Err(e) = db.finish_session(sid, crate::history::SessionStatus::InProgress) {
            log::warn!("history: failed to mark session in_progress: {}", e);
        }
    }

    let mut success_count = 0u32;
    let mut fail_count = 0u32;
    let mut skip_count = 0u32;
    let mode_str = if movie_mode { "movie" } else { "tv" };

    for (i, &idx) in selected.iter().enumerate() {
        if crate::CANCELLED.load(std::sync::atomic::Ordering::Relaxed) {
            println!("\nCancelled.");
            break;
        }

        let pl = &episodes_pl[idx];
        let outfile = &outfiles[i];
        let filename = outfile
            .file_name()
            .expect("output path has filename")
            .to_string_lossy();

        let estimated_size = Some(crate::workflow::estimate_size(
            pl,
            clip_sizes.get(&pl.num).copied(),
            probe_cache.get(&pl.num).map(|(mi, _)| mi),
        ));

        match crate::workflow::check_overwrite(
            outfile,
            args.overwrite || config.overwrite(),
            estimated_size,
        )? {
            crate::workflow::OverwriteAction::Proceed => {}
            crate::workflow::OverwriteAction::Skip(size) => {
                println!(
                    "\nSkipping playlist {} -> {} (already exists, {})",
                    pl.num,
                    filename,
                    format_size(size)
                );
                // Record skip in history
                if let (Some(db), Some(sid)) = (history, session_id) {
                    let episodes = tmdb_ctx.episode_assignments.get(&pl.num).map(|eps| {
                        serde_json::to_string(
                            &eps.iter().map(|e| e.episode_number).collect::<Vec<_>>(),
                        )
                        .unwrap_or_default()
                    });
                    let file_info = crate::history::RippedFileInfo {
                        playlist: pl.num.clone(),
                        episodes,
                        output_path: outfile.display().to_string(),
                        file_size: Some(size as i64),
                        duration_ms: Some((pl.seconds as i64) * 1000),
                        streams: None,
                        chapters: None,
                    };
                    if let Ok(fid) = db.record_file(sid, &file_info) {
                        let _ =
                            db.update_file_status(fid, crate::history::FileStatus::Skipped, None);
                    }
                }
                skip_count += 1;
                continue;
            }
            crate::workflow::OverwriteAction::PartialReplace(size) => {
                println!(
                    "\nRe-ripping partial file {} -> {} (was {}, expected ~{})",
                    pl.num,
                    filename,
                    format_size(size),
                    estimated_size
                        .map(format_size)
                        .unwrap_or_else(|| "unknown".to_string()),
                );
            }
            crate::workflow::OverwriteAction::DeleteAndProceed(size) => {
                println!("\nOverwriting {} ({})", filename, format_size(size));
            }
        }

        println!(
            "\nRipping playlist {} ({}) -> {}",
            pl.num, pl.duration, filename
        );

        // Open input context (also extracts media/stream info, eliminating a
        // separate probe_playlist call that would re-open the device).
        let (ictx, guard, media_info, stream_info) =
            match crate::media::remux::open_remux_input(device, &pl.num) {
                Ok(result) => result,
                Err(e) => {
                    println!("Error opening playlist {}: {}", pl.num, e);
                    fail_count += 1;
                    continue;
                }
            };

        // Re-resolve filename with full media info from the open context.
        // The pre-computed outfile may have unresolved template vars like {resolution}.
        let is_special = specials.contains(&pl.num);
        let show_name_str = if movie_mode {
            tmdb_ctx
                .movie_title
                .as_ref()
                .map(|(t, _)| t.clone())
                .unwrap_or_else(|| "Unknown".to_string())
        } else {
            tmdb_ctx.show_name.clone().unwrap_or_else(|| {
                label_info
                    .map(|l| l.show.clone())
                    .unwrap_or_else(|| "Unknown".to_string())
            })
        };
        let episodes = tmdb_ctx
            .episode_assignments
            .get(&pl.num)
            .cloned()
            .unwrap_or_default();
        let part = if tmdb_ctx.movie_title.is_some() && selected.len() > 1 {
            selected
                .iter()
                .position(|&s| s == idx)
                .map(|p| p as u32 + 1)
        } else {
            None
        };
        let resolved_name = workflow::build_output_filename(
            pl,
            &episodes,
            tmdb_ctx.season_num.unwrap_or(0),
            movie_mode,
            is_special,
            tmdb_ctx
                .movie_title
                .as_ref()
                .map(|(t, y)| (t.as_str(), y.as_str())),
            &show_name_str,
            label,
            label_info,
            config,
            args.format.as_deref(),
            args.format_preset.as_deref(),
            Some(&media_info),
            part,
        );
        let original_name = outfiles[i]
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let filename_changed = resolved_name != original_name;
        let outfile = if filename_changed {
            eprintln!(
                "  Filename resolved: {} -> {}",
                original_name, resolved_name
            );
            &output_dir.join(&resolved_name)
        } else {
            outfile
        };
        let filename = outfile
            .file_name()
            .expect("output path has filename")
            .to_string_lossy();

        // Re-check overwrite with final filename if it changed
        if filename_changed {
            // Ensure output directory exists for re-resolved path
            if let Some(parent) = outfile.parent() {
                std::fs::create_dir_all(parent)?;
            }
            match crate::workflow::check_overwrite(
                outfile,
                args.overwrite || config.overwrite(),
                estimated_size,
            )? {
                crate::workflow::OverwriteAction::Proceed => {}
                crate::workflow::OverwriteAction::Skip(size) => {
                    println!(
                        "\nSkipping playlist {} -> {} (already exists, {})",
                        pl.num,
                        filename,
                        format_size(size)
                    );
                    if let (Some(db), Some(sid)) = (history, session_id) {
                        let episodes_json = tmdb_ctx.episode_assignments.get(&pl.num).map(|eps| {
                            serde_json::to_string(
                                &eps.iter().map(|e| e.episode_number).collect::<Vec<_>>(),
                            )
                            .unwrap_or_default()
                        });
                        let file_info = crate::history::RippedFileInfo {
                            playlist: pl.num.clone(),
                            episodes: episodes_json,
                            output_path: outfile.display().to_string(),
                            file_size: Some(size as i64),
                            duration_ms: Some((pl.seconds as i64) * 1000),
                            streams: None,
                            chapters: None,
                        };
                        if let Ok(fid) = db.record_file(sid, &file_info) {
                            let _ = db.update_file_status(
                                fid,
                                crate::history::FileStatus::Skipped,
                                None,
                            );
                        }
                    }
                    skip_count += 1;
                    continue;
                }
                crate::workflow::OverwriteAction::PartialReplace(size) => {
                    println!(
                        "\nRe-ripping partial file {} -> {} (was {}, expected ~{})",
                        pl.num,
                        filename,
                        format_size(size),
                        estimated_size
                            .map(format_size)
                            .unwrap_or_else(|| "unknown".to_string()),
                    );
                }
                crate::workflow::OverwriteAction::DeleteAndProceed(size) => {
                    println!("\nOverwriting {} ({})", filename, format_size(size));
                }
            }
        }

        // Resolve stream selection per-playlist
        let stream_selection = if let Some(tracks) = tracks_spec {
            match crate::streams::parse_track_spec(tracks, &stream_info) {
                Ok(indices) => {
                    let errors = crate::streams::validate_track_selection(&indices, &stream_info);
                    if !errors.is_empty() {
                        eprintln!("Warning: Playlist {}: {}", pl.num, errors.join(", "));
                    }
                    crate::media::StreamSelection::Manual(indices)
                }
                Err(e) => {
                    anyhow::bail!("Invalid --tracks spec: {}", e);
                }
            }
        } else if !stream_filter.is_empty() {
            let indices = stream_filter.apply(&stream_info);
            let errors = crate::streams::validate_track_selection(&indices, &stream_info);
            if !errors.is_empty() {
                eprintln!("Warning: Playlist {}: {}", pl.num, errors.join(", "));
            }
            crate::media::StreamSelection::Manual(indices)
        } else {
            crate::media::StreamSelection::All
        };

        // Compute expected stream counts for verification before stream_selection is moved.
        // When manual selection is active, the output will have fewer streams than the source.
        let (expected_video, expected_audio, expected_subtitle) = match &stream_selection {
            crate::media::StreamSelection::Manual(indices) => {
                crate::streams::count_selected_streams(indices, &stream_info)
            }
            crate::media::StreamSelection::All => {
                (pl.video_streams, pl.audio_streams, pl.subtitle_streams)
            }
        };

        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut options = crate::workflow::prepare_remux_options(
            pl,
            mount_point.as_deref(),
            cancel,
            config.reserve_index_space(),
            metadata_per_playlist[i].clone(),
        );
        options.stream_selection = stream_selection;

        // Record file start in history
        let episodes_json = tmdb_ctx.episode_assignments.get(&pl.num).map(|eps| {
            serde_json::to_string(&eps.iter().map(|e| e.episode_number).collect::<Vec<_>>())
                .unwrap_or_default()
        });
        let history_file_id = if let (Some(db), Some(sid)) = (history, session_id) {
            let file_info = crate::history::RippedFileInfo {
                playlist: pl.num.clone(),
                episodes: episodes_json,
                output_path: outfile.display().to_string(),
                file_size: None,
                duration_ms: Some((pl.seconds as i64) * 1000),
                streams: None,
                chapters: None,
            };
            match db.record_file(sid, &file_info) {
                Ok(fid) => Some(fid),
                Err(e) => {
                    log::warn!("history: failed to record file: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let pl_seconds = pl.seconds;
        let is_tty = crate::atty_stdout();
        let last_print = std::cell::Cell::new(std::time::Instant::now());
        let started = std::cell::Cell::new(false);
        let pl_num = pl.num.clone();

        let result = crate::media::remux::write_remux(ictx, guard, outfile, options, |progress| {
            // Use actual stream duration from FFmpeg when available,
            // falling back to libbluray's parsed playlist duration
            let duration = if progress.duration_secs > 0 {
                progress.duration_secs
            } else {
                pl_seconds
            };
            if is_tty {
                let size = format_size(progress.total_size);
                let time = format_time(progress.out_time_secs);
                let mut parts = vec![
                    format!("frame={}", progress.frame),
                    format!("fps={:.1}", progress.fps),
                    format!("size={}", size),
                    format!("time={}", time),
                    format!("bitrate={}", progress.bitrate),
                    format!("speed={:.1}x", progress.speed),
                ];
                if let Some(est) = rip::estimate_final_size(progress, duration) {
                    parts.push(format!("est=~{}", format_size(est)));
                }
                if let Some(eta_secs) = rip::estimate_eta(progress, duration) {
                    parts.push(format!("eta={}", rip::format_eta(eta_secs)));
                }
                print!("\r  {:<100}", parts.join(" "));
                io::stdout().flush().ok();
            } else {
                // Non-TTY: line-based progress at 10-second intervals
                if !started.get() {
                    started.set(true);
                    println!("  {}", format_progress_line(&pl_num, progress, duration));
                    last_print.set(std::time::Instant::now());
                } else if last_print.get().elapsed() >= std::time::Duration::from_secs(10) {
                    println!("  {}", format_progress_line(&pl_num, progress, duration));
                    last_print.set(std::time::Instant::now());
                }
            }
        });

        if is_tty {
            println!(); // newline after \r progress
        }

        let was_cancelled = matches!(&result, Err(crate::media::MediaError::Cancelled));

        let (
            hook_status,
            hook_error,
            hook_size,
            hook_chapters,
            verify_hook_status,
            verify_hook_detail,
        ) = match result {
            Ok(chapters_added) => {
                let final_size = std::fs::metadata(outfile)?.len();
                if !is_tty {
                    println!("  [{}] 100% {} — done", pl.num, format_size(final_size));
                }
                println!("Done: {} ({})", filename, format_size(final_size));
                if chapters_added > 0 {
                    println!("  Added {} chapter markers", chapters_added);
                }

                // Verification
                let (verify_status, verify_detail) = {
                    let do_verify = args.verify || (!args.no_verify && config.verify());
                    if do_verify {
                        let level = match args
                            .verify_level
                            .as_deref()
                            .unwrap_or(config.verify_level())
                        {
                            "full" => crate::verify::VerifyLevel::Full,
                            _ => crate::verify::VerifyLevel::Quick,
                        };
                        let expected = crate::verify::VerifyExpected {
                            duration_secs: pl.seconds,
                            video_streams: expected_video,
                            audio_streams: expected_audio,
                            subtitle_streams: expected_subtitle,
                            chapters: chapters_added,
                        };
                        let result = crate::verify::verify_output(outfile, &expected, level);
                        if result.passed {
                            println!("  Verified ({:?}): all checks passed", level);
                            ("passed", String::new())
                        } else {
                            let failed: Vec<&str> = result
                                .checks
                                .iter()
                                .filter(|c| !c.passed)
                                .map(|c| c.detail.as_str())
                                .collect();
                            log::warn!(
                                "Verification failed for {}: {}",
                                filename,
                                failed.join("; ")
                            );
                            println!("  WARNING: verification failed: {}", failed.join("; "));
                            let detail = result
                                .checks
                                .iter()
                                .filter(|c| !c.passed)
                                .map(|c| c.name)
                                .collect::<Vec<_>>()
                                .join(",");
                            ("failed", detail)
                        }
                    } else {
                        ("skipped", String::new())
                    }
                };

                success_count += 1;
                (
                    "success",
                    String::new(),
                    final_size,
                    chapters_added,
                    verify_status.to_string(),
                    verify_detail,
                )
            }
            Err(crate::media::MediaError::Cancelled) => {
                if outfile.exists() {
                    let _ = std::fs::remove_file(outfile);
                }
                println!("Cancelled — removed partial file {}", filename);
                fail_count += 1;
                (
                    "failed",
                    "Cancelled".to_string(),
                    0u64,
                    0usize,
                    "skipped".to_string(),
                    String::new(),
                )
            }
            Err(e) => {
                let err_msg = e.to_string();
                if outfile.exists() {
                    let _ = std::fs::remove_file(outfile);
                }
                println!("Error: {} — removed partial file {}", err_msg, filename);
                fail_count += 1;
                (
                    "failed",
                    err_msg,
                    0u64,
                    0usize,
                    "skipped".to_string(),
                    String::new(),
                )
            }
        };

        // Update history file status
        if let (Some(db), Some(fid), Some(sid)) = (history, history_file_id, session_id) {
            let (status, error) = if hook_status == "success" {
                (crate::history::FileStatus::Completed, None)
            } else {
                (
                    crate::history::FileStatus::Failed,
                    if hook_error.is_empty() {
                        None
                    } else {
                        Some(hook_error.as_str())
                    },
                )
            };
            // Update with final size for completed files
            if status == crate::history::FileStatus::Completed && hook_size > 0 {
                // Re-record with final size and chapter count, then set completed
                let episodes_json = tmdb_ctx.episode_assignments.get(&pl.num).map(|eps| {
                    serde_json::to_string(&eps.iter().map(|e| e.episode_number).collect::<Vec<_>>())
                        .unwrap_or_default()
                });
                let file_info = crate::history::RippedFileInfo {
                    playlist: pl.num.clone(),
                    episodes: episodes_json,
                    output_path: outfile.display().to_string(),
                    file_size: Some(hook_size as i64),
                    duration_ms: Some((pl.seconds as i64) * 1000),
                    streams: None,
                    chapters: Some(hook_chapters as i32),
                };
                let _ = db.record_file(sid, &file_info);
            }
            if let Err(e) = db.update_file_status(fid, status, error) {
                log::warn!("history: failed to update file status: {}", e);
            }
        }

        // Post-rip hook
        {
            let episodes = tmdb_ctx.episode_assignments.get(&pl.num);
            let title_str = if movie_mode {
                tmdb_ctx
                    .movie_title
                    .as_ref()
                    .map(|(t, _)| t.as_str())
                    .unwrap_or("")
            } else {
                tmdb_ctx.show_name.as_deref().unwrap_or("")
            };
            let mut vars = std::collections::HashMap::new();
            vars.insert("file", outfile.display().to_string());
            vars.insert("filename", filename.to_string());
            vars.insert("dir", output_dir.display().to_string());
            vars.insert("size", hook_size.to_string());
            vars.insert("chapters", hook_chapters.to_string());
            vars.insert("title", title_str.to_string());
            vars.insert(
                "season",
                tmdb_ctx
                    .season_num
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
            );
            vars.insert(
                "episode",
                episodes
                    .and_then(|e| e.first())
                    .map(|e| e.episode_number.to_string())
                    .unwrap_or_default(),
            );
            vars.insert(
                "episode_name",
                episodes
                    .and_then(|e| e.first())
                    .map(|e| e.name.clone())
                    .unwrap_or_default(),
            );
            vars.insert("playlist", pl.num.clone());
            vars.insert("label", label.to_string());
            vars.insert("mode", mode_str.to_string());
            vars.insert("device", device.to_string());
            vars.insert("status", hook_status.to_string());
            vars.insert("error", hook_error);
            vars.insert("verify", verify_hook_status);
            vars.insert("verify_detail", verify_hook_detail);
            vars.insert(
                "session_id",
                session_id.map(|id| id.to_string()).unwrap_or_default(),
            );
            vars.insert(
                "disc_number",
                disc_number.map(|n| n.to_string()).unwrap_or_default(),
            );
            vars.insert(
                "history_status",
                if history.is_some() && session_id.is_some() {
                    "saved"
                } else {
                    "disabled"
                }
                .to_string(),
            );
            crate::hooks::run_post_rip(config, &vars, no_hooks);
        }

        if was_cancelled {
            break;
        }
    }

    if let Some(ref mut guard) = _mount_guard {
        guard.cleanup();
    }

    // Post-session hook
    {
        let title_str = if movie_mode {
            tmdb_ctx
                .movie_title
                .as_ref()
                .map(|(t, _)| t.as_str())
                .unwrap_or("")
        } else {
            tmdb_ctx.show_name.as_deref().unwrap_or("")
        };
        let mut vars = std::collections::HashMap::new();
        vars.insert("title", title_str.to_string());
        vars.insert(
            "season",
            tmdb_ctx
                .season_num
                .map(|n| n.to_string())
                .unwrap_or_default(),
        );
        vars.insert("label", label.to_string());
        vars.insert("device", device.to_string());
        vars.insert("mode", mode_str.to_string());
        vars.insert("dir", output_dir.display().to_string());
        vars.insert("total", selected.len().to_string());
        vars.insert("succeeded", success_count.to_string());
        vars.insert("failed", fail_count.to_string());
        vars.insert("skipped", skip_count.to_string());
        vars.insert(
            "session_id",
            session_id.map(|id| id.to_string()).unwrap_or_default(),
        );
        vars.insert(
            "disc_number",
            disc_number.map(|n| n.to_string()).unwrap_or_default(),
        );
        vars.insert(
            "history_status",
            if history.is_some() && session_id.is_some() {
                "saved"
            } else {
                "disabled"
            }
            .to_string(),
        );
        crate::hooks::run_post_session(config, &vars, no_hooks);
    }

    println!(
        "\nAll done! Ripped {} playlist(s) to {}",
        selected.len(),
        output_dir.display()
    );

    if !skip_eject && fail_count == 0 && config.should_eject(args.cli_eject()) {
        println!("Ejecting disc...");
        if let Err(e) = disc::eject_disc(device) {
            println!("Warning: failed to eject disc: {}", e);
        }
    }

    Ok((success_count, fail_count))
}

fn format_time(seconds: u32) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    format!("{}:{:02}:{:02}", h, m, s)
}

fn format_progress_line(
    playlist_num: &str,
    progress: &crate::types::RipProgress,
    total_seconds: u32,
) -> String {
    let pct = if total_seconds > 0 {
        (progress.out_time_secs as f64 / total_seconds as f64 * 100.0).min(100.0) as u32
    } else {
        0
    };
    let size = format_size(progress.total_size);
    let speed_str = format!(
        "{:.0}MiB/s",
        progress.total_size as f64 / progress.out_time_secs.max(1) as f64 / 1048576.0
    );
    let eta_str = rip::estimate_eta(progress, total_seconds)
        .map(|e| format!("ETA {}", rip::format_eta(e)))
        .unwrap_or_default();
    let mut parts = vec![
        format!("[{}]", playlist_num),
        format!("{}%", pct),
        size,
        speed_str,
    ];
    if !eta_str.is_empty() {
        parts.push(eta_str);
    }
    parts.join(" ")
}

fn prompt(msg: &str) -> io::Result<String> {
    print!("{}", msg);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

fn prompt_number(msg: &str, default: Option<u32>) -> io::Result<u32> {
    loop {
        let input = prompt(msg)?;
        if input.is_empty() {
            if let Some(d) = default {
                return Ok(d);
            }
        }
        if let Ok(n) = input.parse::<u32>() {
            if n > 0 {
                return Ok(n);
            }
        }
        println!("  Invalid number.");
    }
}

fn prompt_tmdb(
    api_key: &str,
    default_query: &str,
    cli_season: Option<u32>,
) -> anyhow::Result<Option<TmdbLookupResult>> {
    let hint = if default_query.is_empty() {
        String::new()
    } else {
        format!(" [{}]", default_query)
    };
    let query = prompt(&format!("\nSearch TMDb for episode info{}: ", hint))?;
    let query = if query.is_empty() {
        default_query.to_string()
    } else {
        query
    };
    if query.is_empty() {
        return Ok(None);
    }

    let results = match tmdb::search_show(&query, api_key) {
        Ok(r) => r,
        Err(e) => {
            println!("  TMDb search failed: {}", e);
            return Ok(None);
        }
    };

    if results.is_empty() {
        println!("  No results found.");
        return Ok(None);
    }

    println!("\n  Results:");
    let display_count = results.len().min(5);
    for (i, show) in results.iter().take(5).enumerate() {
        let year = show
            .first_air_date
            .as_deref()
            .unwrap_or("")
            .get(..4)
            .unwrap_or("");
        println!("    {}. {} ({})", i + 1, show.name, year);
    }

    let show_idx = loop {
        let pick = prompt("  Select show (1-5, Enter for 1, 's' to skip): ")?;
        if pick.eq_ignore_ascii_case("s") {
            return Ok(None);
        }
        let pick = if pick.is_empty() {
            "1".to_string()
        } else {
            pick
        };
        if let Ok(n) = pick.parse::<usize>() {
            if n >= 1 && n <= display_count {
                break n - 1;
            }
        }
        println!("  Invalid selection.");
    };

    let show = &results[show_idx];
    let show_id = show.id;

    let season_num = if let Some(s) = cli_season {
        println!("  Using season {} (from --season flag)", s);
        s
    } else {
        prompt_number("  Season number: ", None)?
    };

    let episodes = match tmdb::get_season(show_id, season_num, api_key) {
        Ok(eps) => eps,
        Err(e) => {
            println!("  Failed to fetch season: {}", e);
            return Ok(None);
        }
    };

    if !episodes.is_empty() {
        println!("\n  Season {}: {} episodes", season_num, episodes.len());
        for ep in &episodes {
            let runtime = ep.runtime.unwrap_or(0);
            println!(
                "    E{:02} - {}  ({} min)",
                ep.episode_number, ep.name, runtime
            );
        }
    }

    Ok(Some(TmdbLookupResult {
        tmdb_id: show.id,
        episodes,
        season: season_num,
        show_name: show.name.clone(),
        first_air_date: show.first_air_date.clone(),
    }))
}

fn prompt_tmdb_movie(
    api_key: &str,
    default_query: &str,
) -> anyhow::Result<Option<(u64, String, String)>> {
    let hint = if default_query.is_empty() {
        String::new()
    } else {
        format!(" [{}]", default_query)
    };
    let query = prompt(&format!("\nSearch TMDb for movie{}: ", hint))?;
    let query = if query.is_empty() {
        default_query.to_string()
    } else {
        query
    };
    if query.is_empty() {
        return Ok(None);
    }

    let results = match tmdb::search_movie(&query, api_key) {
        Ok(r) => r,
        Err(e) => {
            println!("  TMDb search failed: {}", e);
            return Ok(None);
        }
    };

    if results.is_empty() {
        println!("  No results found.");
        return Ok(None);
    }

    println!("\n  Results:");
    let display_count = results.len().min(5);
    for (i, movie) in results.iter().take(5).enumerate() {
        let year = movie
            .release_date
            .as_deref()
            .unwrap_or("")
            .get(..4)
            .unwrap_or("");
        println!("    {}. {} ({})", i + 1, movie.title, year);
    }

    let movie_idx = loop {
        let pick = prompt("  Select movie (1-5, Enter for 1, 's' to skip): ")?;
        if pick.eq_ignore_ascii_case("s") {
            return Ok(None);
        }
        let pick = if pick.is_empty() {
            "1".to_string()
        } else {
            pick
        };
        if let Ok(n) = pick.parse::<usize>() {
            if n >= 1 && n <= display_count {
                break n - 1;
            }
        }
        println!("  Invalid selection.");
    };

    let movie = &results[movie_idx];
    let year = movie
        .release_date
        .as_deref()
        .unwrap_or("")
        .get(..4)
        .unwrap_or("")
        .to_string();

    println!("  Selected: {} ({})", movie.title, year);
    Ok(Some((movie.id, movie.title.clone(), year)))
}

fn headless_tmdb_movie(
    api_key: &str,
    default_query: &str,
) -> anyhow::Result<Option<(u64, String, String)>> {
    if default_query.is_empty() {
        return Ok(None);
    }

    let results = match tmdb::search_movie(default_query, api_key) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };

    if results.is_empty() {
        return Ok(None);
    }

    let movie = &results[0];
    let year = movie
        .release_date
        .as_deref()
        .unwrap_or("")
        .get(..4)
        .unwrap_or("")
        .to_string();

    log::info!("TMDb: auto-selected \"{}\" ({})", movie.title, year);
    println!("TMDb: auto-selected \"{}\" ({})", movie.title, year);
    Ok(Some((movie.id, movie.title.clone(), year)))
}

fn headless_tmdb_tv(
    api_key: &str,
    default_query: &str,
    cli_season: Option<u32>,
) -> anyhow::Result<Option<TmdbLookupResult>> {
    if default_query.is_empty() {
        return Ok(None);
    }

    let results = match tmdb::search_show(default_query, api_key) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };

    if results.is_empty() {
        return Ok(None);
    }

    let show = &results[0];
    log::info!("TMDb: auto-selected \"{}\"", show.name);
    println!("TMDb: auto-selected \"{}\"", show.name);

    let season_num = match cli_season {
        Some(s) => s,
        None => {
            anyhow::bail!("Cannot determine season number in headless mode. Use --season <NUM>.");
        }
    };

    let episodes = match tmdb::get_season(show.id, season_num, api_key) {
        Ok(eps) => eps,
        Err(_) => return Ok(None),
    };

    Ok(Some(TmdbLookupResult {
        tmdb_id: show.id,
        episodes,
        season: season_num,
        show_name: show.name.clone(),
        first_air_date: show.first_air_date.clone(),
    }))
}

pub fn run_batch(
    args: &Args,
    config: &crate::config::Config,
    headless: bool,
    stream_filter: &crate::streams::StreamFilter,
    tracks_spec: Option<&str>,
    history: Option<&crate::history::HistoryDb>,
    ignore_history: bool,
) -> anyhow::Result<()> {
    let mut disc_count: u32 = 0;
    let mut total_files: u32 = 0;
    let mut total_failures: u32 = 0;
    let mut next_start_episode = args.start_episode.unwrap_or(1);
    let device = args.device().to_string_lossy().to_string();
    let batch_id = uuid::Uuid::new_v4().to_string();

    loop {
        if crate::CANCELLED.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        // Wait for disc if not the first iteration
        if disc_count > 0 {
            println!("\nWaiting for next disc...");
            loop {
                if crate::CANCELLED.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let label = disc::get_volume_label(&device);
                if !label.is_empty() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            if crate::CANCELLED.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
        }

        disc_count += 1;
        log::info!("=== Batch disc {} ===", disc_count);

        match run(
            args,
            config,
            headless,
            stream_filter,
            tracks_spec,
            Some(next_start_episode),
            true, // skip_eject: batch loop handles eject
            history,
            ignore_history,
            Some(&batch_id),
        ) {
            Ok((files_ripped, regular_episodes_ripped)) => {
                total_files += files_ripped;
                next_start_episode += regular_episodes_ripped;
            }
            Err(e) => {
                eprintln!("Disc {} failed: {}", disc_count, e);
                total_failures += 1;
            }
        }

        // Always eject in batch mode to prepare for next disc
        println!("Ejecting disc...");
        if let Err(e) = disc::eject_disc(&device) {
            eprintln!("Warning: failed to eject disc: {}", e);
        }
    }

    println!(
        "\nBatch complete: {} disc(s), {} file(s), {} failure(s)",
        disc_count, total_files, total_failures
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_progress_line() {
        let progress = crate::types::RipProgress {
            frame: 1000,
            fps: 50.0,
            total_size: 256 * 1024 * 1024, // 256 MiB
            out_time_secs: 300,
            bitrate: "25000".into(),
            speed: 2.0,
            ..Default::default()
        };
        let line = format_progress_line("00001", &progress, 2500);
        assert!(line.starts_with("[00001] 12%"));
        assert!(line.contains("MiB"));
        assert!(line.contains("ETA"));
    }

    #[test]
    fn test_format_progress_line_complete() {
        let progress = crate::types::RipProgress {
            frame: 0,
            fps: 0.0,
            total_size: 2 * 1024 * 1024 * 1024, // 2 GiB
            out_time_secs: 2500,
            bitrate: String::new(),
            speed: 1.0,
            ..Default::default()
        };
        let line = format_progress_line("00001", &progress, 2500);
        assert!(line.starts_with("[00001] 100%"));
    }

    #[test]
    fn test_format_video_column() {
        let info = crate::types::MediaInfo {
            codec: "h264".into(),
            resolution: "1080p".into(),
            framerate: "23.976".into(),
            ..Default::default()
        };
        assert_eq!(format_video_column(&info), "H.264 1080p 23.976");
    }

    #[test]
    fn test_format_video_column_hevc() {
        let info = crate::types::MediaInfo {
            codec: "hevc".into(),
            resolution: "2160p".into(),
            framerate: "23.976".into(),
            ..Default::default()
        };
        assert_eq!(format_video_column(&info), "HEVC 2160p 23.976");
    }

    #[test]
    fn test_format_audio_column() {
        let streams = vec![
            crate::types::AudioStream {
                index: 0,
                codec: "truehd".into(),
                channels: 8,
                channel_layout: "7.1".into(),
                language: Some("eng".into()),
                profile: Some("TrueHD".into()),
            },
            crate::types::AudioStream {
                index: 1,
                codec: "ac3".into(),
                channels: 2,
                channel_layout: "stereo".into(),
                language: Some("eng".into()),
                profile: None,
            },
        ];
        assert_eq!(
            format_audio_column(&streams),
            "a0:TrueHD 7.1, a1:ac3 stereo"
        );
    }

    #[test]
    fn test_format_audio_column_empty() {
        assert_eq!(format_audio_column(&[]), "");
    }

    #[test]
    fn test_format_subtitle_column() {
        let streams = vec![
            crate::types::SubtitleStream {
                index: 6,
                codec: "hdmv_pgs_subtitle".into(),
                language: Some("eng".into()),
                forced: false,
            },
            crate::types::SubtitleStream {
                index: 7,
                codec: "hdmv_pgs_subtitle".into(),
                language: Some("eng".into()),
                forced: true,
            },
        ];
        assert_eq!(format_subtitle_column(&streams), "s0:eng, s1:eng FORCED");
    }

    #[test]
    fn test_format_subtitle_column_empty() {
        assert_eq!(format_subtitle_column(&[]), "");
    }

    // --- CLI flag conflict tests for stream selection ---

    #[test]
    fn test_tracks_conflicts_with_audio_lang() {
        use clap::Parser;
        let result =
            crate::Args::try_parse_from(["bluback", "--tracks", "a:0", "--audio-lang", "eng"]);
        assert!(
            result.is_err(),
            "--tracks should conflict with --audio-lang"
        );
    }

    #[test]
    fn test_tracks_conflicts_with_all_streams() {
        use clap::Parser;
        let result = crate::Args::try_parse_from(["bluback", "--tracks", "a:0", "--all-streams"]);
        assert!(
            result.is_err(),
            "--tracks should conflict with --all-streams"
        );
    }

    #[test]
    fn test_all_streams_conflicts_with_audio_lang() {
        use clap::Parser;
        let result =
            crate::Args::try_parse_from(["bluback", "--all-streams", "--audio-lang", "eng"]);
        assert!(
            result.is_err(),
            "--all-streams should conflict with --audio-lang"
        );
    }

    #[test]
    fn test_audio_lang_and_subtitle_lang_compose() {
        use clap::Parser;
        let result = crate::Args::try_parse_from([
            "bluback",
            "--audio-lang",
            "eng,jpn",
            "--subtitle-lang",
            "eng",
            "--no-tui",
        ]);
        assert!(
            result.is_ok(),
            "--audio-lang and --subtitle-lang should compose: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_prefer_surround_composes_with_lang_flags() {
        use clap::Parser;
        let result = crate::Args::try_parse_from([
            "bluback",
            "--audio-lang",
            "eng",
            "--prefer-surround",
            "--no-tui",
        ]);
        assert!(
            result.is_ok(),
            "--prefer-surround should compose with --audio-lang: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_batch_summary_format() {
        let summary = format!(
            "Batch complete: {} disc(s), {} file(s), {} failure(s)",
            3, 12, 1
        );
        assert_eq!(
            summary,
            "Batch complete: 3 disc(s), 12 file(s), 1 failure(s)"
        );
    }

    #[test]
    fn test_batch_summary_zero_failures() {
        let summary = format!(
            "Batch complete: {} disc(s), {} file(s), {} failure(s)",
            2, 8, 0
        );
        assert!(summary.contains("0 failure(s)"));
    }
}
