use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use crate::disc;
use crate::rip;
use crate::tmdb;
use crate::types::*;
use crate::util::{self, *};
use crate::Args;

struct TmdbContext {
    episode_assignments: EpisodeAssignments,
    season_num: Option<u32>,
    movie_title: Option<(String, String)>,
    show_name: Option<String>,
}

pub fn run(args: &Args, config: &crate::config::Config) -> anyhow::Result<()> {
    let device = args.device.to_string_lossy();

    let (label, label_info, episodes_pl, movie_mode) = scan_disc(args, config)?;

    let tmdb_ctx = lookup_tmdb(args, config, &label_info, &episodes_pl, movie_mode)?;

    let selected = display_and_select(
        &episodes_pl,
        &tmdb_ctx.episode_assignments,
        tmdb_ctx.season_num,
    )?;

    let outfiles = build_filenames(
        args,
        config,
        &device,
        &label,
        &label_info,
        &episodes_pl,
        &selected,
        &tmdb_ctx,
        movie_mode,
    )?;

    rip_selected(args, config, &device, &episodes_pl, &selected, &outfiles)
}

fn scan_disc(
    args: &Args,
    config: &crate::config::Config,
) -> anyhow::Result<(String, Option<LabelInfo>, Vec<Playlist>, bool)> {
    let device = args.device.to_string_lossy();

    if !args.device.exists() {
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

    println!("Scanning disc at {}...", device);
    let playlists = disc::scan_playlists(&device)?;
    if playlists.is_empty() {
        anyhow::bail!("No playlists found. Check libaacs and KEYDB.cfg.");
    }

    let episodes_pl: Vec<Playlist> = disc::filter_episodes(&playlists, args.min_duration)
        .into_iter()
        .cloned()
        .collect();
    let short_count = playlists.len() - episodes_pl.len();
    println!(
        "Found {} playlists ({} episode-length, {} short/extras).\n",
        playlists.len(),
        episodes_pl.len(),
        short_count
    );

    if episodes_pl.is_empty() {
        anyhow::bail!("No episode-length playlists found. Try lowering --min-duration.");
    }

    let movie_mode = args.movie || (episodes_pl.len() == 1 && args.season.is_none());
    if movie_mode && !args.movie {
        println!("  (Single playlist detected — using movie mode. Use --season to force TV mode.)");
    }

    Ok((label, label_info, episodes_pl, movie_mode))
}

fn lookup_tmdb(
    args: &Args,
    config: &crate::config::Config,
    label_info: &Option<LabelInfo>,
    episodes_pl: &[Playlist],
    movie_mode: bool,
) -> anyhow::Result<TmdbContext> {
    let mut ctx = TmdbContext {
        episode_assignments: HashMap::new(),
        season_num: args.season.or(label_info.as_ref().map(|l| l.season)),
        movie_title: None,
        show_name: None,
    };

    let mut api_key = tmdb::get_api_key(config);

    if api_key.is_none() {
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
            ctx.movie_title = prompt_tmdb_movie(key, default_query)?;
        } else {
            if api_key.is_none() && (args.season.is_some() || args.start_episode.is_some()) {
                println!("Warning: --season/--start-episode require TMDb. Ignoring.");
            }

            let cli_season = args.season.or(label_info.as_ref().map(|l| l.season));

            if let Some(lookup) = prompt_tmdb(key, default_query, cli_season)? {
                ctx.season_num = Some(lookup.season);
                ctx.show_name = Some(lookup.show_name);

                let disc_number = label_info.as_ref().map(|l| l.disc);
                let default_start = args
                    .start_episode
                    .unwrap_or_else(|| guess_start_episode(disc_number, episodes_pl.len()));

                let start_ep = if args.start_episode.is_none() {
                    prompt_number(
                        &format!("  Starting episode number [{}]: ", default_start),
                        Some(default_start),
                    )?
                } else {
                    default_start
                };

                ctx.episode_assignments = assign_episodes(episodes_pl, &lookup.episodes, start_ep);
            }
        }
    }

    Ok(ctx)
}

fn display_and_select(
    episodes_pl: &[Playlist],
    episode_assignments: &EpisodeAssignments,
    season_num: Option<u32>,
) -> anyhow::Result<Vec<usize>> {
    let has_eps = !episode_assignments.is_empty();
    let header_ep = if has_eps { "  Episode" } else { "" };
    println!(
        "\n  {:<4}  {:<10}  {:<10}{}",
        "#", "Playlist", "Duration", header_ep
    );
    println!(
        "  {:<4}  {:<10}  {:<10}{}",
        "---",
        "--------",
        "--------",
        "-".repeat(header_ep.len())
    );

    for (i, pl) in episodes_pl.iter().enumerate() {
        let ep_str = if let Some(ep) = episode_assignments.get(&pl.num) {
            format!(
                "  S{:02}E{:02} - {}",
                season_num.unwrap_or(0),
                ep.episode_number,
                ep.name
            )
        } else if has_eps {
            "  (no episode data)".into()
        } else {
            String::new()
        };
        println!(
            "  {:<4}  {:<10}  {:<10}{}",
            i + 1,
            pl.num,
            pl.duration,
            ep_str
        );
    }
    println!();

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
    device: &str,
    label: &str,
    label_info: &Option<LabelInfo>,
    episodes_pl: &[Playlist],
    selected: &[usize],
    tmdb_ctx: &TmdbContext,
    movie_mode: bool,
) -> anyhow::Result<Vec<PathBuf>> {
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

    let default_names: Vec<String> = selected
        .iter()
        .enumerate()
        .map(|(sel_i, &idx)| {
            let pl = &episodes_pl[idx];

            let media_info = if use_custom_format {
                disc::probe_media_info(device, &pl.num)
            } else {
                None
            };

            let fmt = if use_custom_format {
                Some(format_template.as_str())
            } else {
                None
            };

            let mut extra_vars: HashMap<&str, String> = HashMap::new();
            extra_vars.insert("show", show_name_str.clone());
            extra_vars.insert(
                "disc",
                label_info
                    .as_ref()
                    .map(|l| l.disc.to_string())
                    .unwrap_or_default(),
            );
            extra_vars.insert("label", label.to_string());
            extra_vars.insert("playlist", pl.num.clone());

            if let Some((ref title, ref year)) = tmdb_ctx.movie_title {
                let part = if selected.len() > 1 {
                    Some(sel_i as u32 + 1)
                } else {
                    None
                };
                util::make_movie_filename(
                    title,
                    year,
                    part,
                    fmt,
                    media_info.as_ref(),
                    Some(&extra_vars),
                )
            } else {
                util::make_filename(
                    &pl.num,
                    tmdb_ctx.episode_assignments.get(&pl.num),
                    tmdb_ctx.season_num.unwrap_or(0),
                    fmt,
                    media_info.as_ref(),
                    Some(&extra_vars),
                )
            }
        })
        .collect();

    println!("  Output filenames:");
    for (i, &idx) in selected.iter().enumerate() {
        let pl = &episodes_pl[idx];
        println!("    {} ({}) -> {}", pl.num, pl.duration, default_names[i]);
    }

    let customize = prompt("\n  Customize filenames? [y/N]: ")?;
    let mut outfiles: Vec<PathBuf> = Vec::new();
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
            outfiles.push(args.output.join(&name));
        }
    } else {
        for name in &default_names {
            outfiles.push(args.output.join(name));
        }
    }

    Ok(outfiles)
}

fn rip_selected(
    args: &Args,
    config: &crate::config::Config,
    device: &str,
    episodes_pl: &[Playlist],
    selected: &[usize],
    outfiles: &[PathBuf],
) -> anyhow::Result<()> {
    if args.dry_run {
        println!("\n[DRY RUN] Would rip:");
        for (i, &idx) in selected.iter().enumerate() {
            let pl = &episodes_pl[idx];
            println!(
                "  {} ({}) -> {}",
                pl.num,
                pl.duration,
                outfiles[i].file_name().unwrap().to_string_lossy()
            );
        }
        return Ok(());
    }

    // Create output directory and any template subdirectories
    for outfile in outfiles {
        if let Some(parent) = outfile.parent() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let mut had_failure = false;

    for (i, &idx) in selected.iter().enumerate() {
        let pl = &episodes_pl[idx];
        let outfile = &outfiles[i];
        let filename = outfile.file_name().unwrap().to_string_lossy();

        // Skip if output file already exists
        if outfile.exists() {
            let existing_size = std::fs::metadata(outfile)?.len();
            println!(
                "\nSkipping playlist {} -> {} (already exists, {})",
                pl.num,
                filename,
                format_size(existing_size)
            );
            continue;
        }

        println!(
            "\nRipping playlist {} ({}) -> {}",
            pl.num, pl.duration, filename
        );

        let streams = match disc::probe_streams(device, &pl.num) {
            Some(s) => s,
            None => {
                println!(
                    "Warning: Failed to probe streams for playlist {}, skipping.",
                    pl.num
                );
                had_failure = true;
                continue;
            }
        };

        let map_args = rip::build_map_args(&streams);
        let mut child = rip::start_rip(device, &pl.num, &map_args, outfile)?;

        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take();
        let reader = io::BufReader::new(stdout);
        let mut state = HashMap::new();

        for line in reader.lines() {
            let line = line?;
            if let Some(progress) = rip::parse_progress_line(&line, &mut state) {
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

                if let Some(est) = rip::estimate_final_size(&progress, pl.seconds) {
                    parts.push(format!("est=~{}", format_size(est)));
                }
                if let Some(eta_secs) = rip::estimate_eta(&progress, pl.seconds) {
                    parts.push(format!("eta={}", rip::format_eta(eta_secs)));
                }

                print!("\r  {:<100}", parts.join(" "));
                io::stdout().flush()?;
            }
        }

        let status = child.wait()?;
        println!();

        if !status.success() {
            let stderr_msg = stderr
                .and_then(|mut s| {
                    let mut buf = String::new();
                    io::Read::read_to_string(&mut s, &mut buf).ok()?;
                    Some(buf)
                })
                .unwrap_or_default();
            if stderr_msg.is_empty() {
                println!(
                    "Error: ffmpeg exited with code {}",
                    status.code().unwrap_or(-1)
                );
            } else {
                let last_line = stderr_msg.lines().last().unwrap_or("");
                println!("Error: ffmpeg: {}", last_line);
            }
            had_failure = true;
            continue;
        }

        let final_size = std::fs::metadata(outfile)?.len();
        println!("Done: {} ({})", filename, format_size(final_size));
    }

    println!(
        "\nAll done! Ripped {} playlist(s) to {}",
        selected.len(),
        args.output.display()
    );

    if !had_failure && config.should_eject(args.cli_eject()) {
        println!("Ejecting disc...");
        if let Err(e) = disc::eject_disc(device) {
            println!("Warning: failed to eject disc: {}", e);
        }
    }

    Ok(())
}

fn format_time(seconds: u32) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    format!("{}:{:02}:{:02}", h, m, s)
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
        episodes,
        season: season_num,
        show_name: show.name.clone(),
    }))
}

fn prompt_tmdb_movie(
    api_key: &str,
    default_query: &str,
) -> anyhow::Result<Option<(String, String)>> {
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
    Ok(Some((movie.title.clone(), year)))
}
