# Headless CLI Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable fully unattended CLI runs via `--yes`/`-y` flag (and auto-detect non-TTY stdin), plus `--title`, `--year`, `--playlists`, and `--list-playlists` flags.

**Architecture:** Add new clap fields to `Args`, compute a `headless` bool in `main()`, pass it to `cli::run()`. Each interactive prompt site in `cli.rs` gets an `if headless { use default }` check. A new `list_playlists()` function handles early dispatch. No changes to TUI mode.

**Tech Stack:** Rust, clap (derive), existing disc/tmdb/util modules.

**Spec:** `docs/superpowers/specs/2026-03-24-headless-cli-design.md`

---

### File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/main.rs` | Modify | Add new Args fields, `atty_stdin()`, headless computation, `--list-playlists` dispatch |
| `src/cli.rs` | Modify | Accept `headless` param, headless checks at all prompt sites, `list_playlists()` function |

---

### Task 1: Add new CLI flags to Args struct

**Files:**
- Modify: `src/main.rs:15-99`

- [ ] **Step 1: Add new fields to Args**

Add these fields to the `Args` struct:

```rust
/// Accept all defaults without prompting (auto if stdin is not a TTY)
#[arg(short = 'y', long)]
yes: bool,

/// Set show (TV) or movie title directly, skipping TMDb lookup
#[arg(long)]
title: Option<String>,

/// Movie release year for filename templates (used with --title in --movie mode)
#[arg(long)]
year: Option<String>,

/// Select specific playlists (e.g. "1,2,3", "1-3", "all")
#[arg(long)]
playlists: Option<String>,

/// Scan disc and print playlist info, then exit
#[arg(long)]
list_playlists: bool,
```

- [ ] **Step 2: Add `atty_stdin()` helper**

Add below the existing `atty_stdout()`:

```rust
fn atty_stdin() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}
```

- [ ] **Step 3: Add headless computation and pass to cli::run**

In `main()`, after `let use_tui = ...`, add:

```rust
let headless = args.yes || (!atty_stdin() && !use_tui);
```

Update the CLI dispatch from `cli::run(&args, &config)` to `cli::run(&args, &config, headless)`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | head -5`
Expected: Compilation error about `cli::run` signature mismatch (expected — we'll fix in Task 3).

- [ ] **Step 5: Commit**

```
feat(cli): add --yes, --title, --year, --playlists, --list-playlists flags
```

---

### Task 2: Implement `--list-playlists` early dispatch

**Files:**
- Modify: `src/main.rs:101-147`
- Modify: `src/cli.rs` (add `list_playlists` function)

- [ ] **Step 1: Add `list_playlists()` function to cli.rs**

Add at the end of `cli.rs` (before `prompt()` helper):

```rust
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

    println!("Scanning disc at {}...\n", device);
    let playlists = disc::scan_playlists(&device)?;
    if playlists.is_empty() {
        anyhow::bail!("No playlists found. Check libaacs and KEYDB.cfg.");
    }

    // Extract chapter counts
    let chapter_counts = {
        let device_str = device.to_string();
        match disc::ensure_mounted(&device_str) {
            Ok((mount, did_mount)) => {
                let nums: Vec<&str> = playlists.iter().map(|pl| pl.num.as_str()).collect();
                let counts = crate::chapters::count_chapters_for_playlists(
                    std::path::Path::new(&mount),
                    &nums,
                );
                if did_mount {
                    let _ = disc::unmount_disc(&device_str);
                }
                counts
            }
            Err(_) => std::collections::HashMap::new(),
        }
    };

    let has_ch = !chapter_counts.is_empty();
    let ch_header = if has_ch { "  Ch" } else { "" };

    // Build filtered index mapping (1-indexed, matching --playlists numbering)
    let mut filtered_idx = 0u32;

    println!(
        "  {:<4}  {:<10}  {:<10}{}  Sel",
        "#", "Playlist", "Duration", ch_header
    );
    println!(
        "  {:<4}  {:<10}  {:<10}{}  ---",
        "---", "--------", "--------",
        if has_ch { "  --" } else { "" }
    );

    for (i, pl) in playlists.iter().enumerate() {
        let short = pl.seconds < args.min_duration;
        let sel_str = if short {
            "  *".to_string()
        } else {
            filtered_idx += 1;
            format!("  {}", filtered_idx)
        };
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
        println!(
            "  {:<4}  {:<10}  {:<10}{}{}",
            i + 1,
            pl.num,
            pl.duration,
            ch_str,
            sel_str
        );
    }

    let episode_count = filtered_idx;
    let short_count = playlists.len() as u32 - episode_count;
    println!(
        "\n  {} playlists ({} episode-length, {} short/extras)",
        playlists.len(),
        episode_count,
        short_count
    );
    println!("  * = below --min-duration ({} secs), not selectable via --playlists", args.min_duration);
    println!("  Sel column = index for --playlists flag");

    Ok(())
}
```

- [ ] **Step 2: Add early dispatch in main()**

In `main()`, after device resolution (after `args.device = Some(drives[0].clone());`) but before the `let use_tui = ...` line, add:

```rust
if args.list_playlists {
    return cli::list_playlists(&args, &config);
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | head -5`
Expected: Still compile error from Task 1 (cli::run signature) — that's OK.

- [ ] **Step 4: Commit**

```
feat(cli): implement --list-playlists command
```

---

### Task 3: Thread headless through cli::run and subfunctions

**Files:**
- Modify: `src/cli.rs:19-65`

- [ ] **Step 1: Update `run()` signature**

Change:
```rust
pub fn run(args: &Args, config: &crate::config::Config) -> anyhow::Result<()> {
```
to:
```rust
pub fn run(args: &Args, config: &crate::config::Config, headless: bool) -> anyhow::Result<()> {
```

- [ ] **Step 2: Thread headless through internal calls**

Update the calls inside `run()`:

```rust
let tmdb_ctx = lookup_tmdb(args, config, &label_info, &episodes_pl, movie_mode, headless)?;

let selected = display_and_select(
    &episodes_pl,
    &tmdb_ctx.episode_assignments,
    tmdb_ctx.season_num,
    &chapter_counts,
    args.playlists.as_deref(),
    headless,
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
    headless,
)?;
```

- [ ] **Step 3: Update function signatures**

Update `lookup_tmdb`:
```rust
fn lookup_tmdb(
    args: &Args,
    config: &crate::config::Config,
    label_info: &Option<LabelInfo>,
    episodes_pl: &[Playlist],
    movie_mode: bool,
    headless: bool,
) -> anyhow::Result<TmdbContext> {
```

Update `display_and_select`:
```rust
fn display_and_select(
    episodes_pl: &[Playlist],
    episode_assignments: &EpisodeAssignments,
    season_num: Option<u32>,
    chapter_counts: &std::collections::HashMap<String, usize>,
    playlists_flag: Option<&str>,
    headless: bool,
) -> anyhow::Result<Vec<usize>> {
```

Update `build_filenames`:
```rust
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
    headless: bool,
) -> anyhow::Result<Vec<PathBuf>> {
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully.

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: All existing tests pass.

- [ ] **Step 6: Commit**

```
refactor(cli): thread headless parameter through CLI functions
```

---

### Task 4: Headless `display_and_select()` + `--playlists` flag

**Files:**
- Modify: `src/cli.rs` (`display_and_select` function, around lines 278-365)

- [ ] **Step 1: Add headless/playlists early return**

At the end of `display_and_select`, replace the interactive selection loop:

```rust
    println!();

    // --playlists flag or headless mode: skip interactive prompt
    if let Some(selection_str) = playlists_flag {
        return match parse_selection(selection_str, episodes_pl.len()) {
            Some(sel) => Ok(sel),
            None => anyhow::bail!(
                "Invalid --playlists selection '{}'. Use e.g. 1,2,3 or 1-3 or 'all' (max {})",
                selection_str,
                episodes_pl.len()
            ),
        };
    }
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
```

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```
feat(cli): headless playlist selection with --playlists flag
```

---

### Task 5: Headless `build_filenames()`

**Files:**
- Modify: `src/cli.rs` (`build_filenames` function, around lines 367-496)

- [ ] **Step 1: Skip customize prompt when headless**

Replace the filename customization section (starting from `let customize = prompt(...)`) with:

```rust
    let mut outfiles: Vec<PathBuf> = Vec::new();
    if !headless {
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
                outfiles.push(args.output.join(&name));
            }
            return Ok(outfiles);
        }
    }
    for name in &default_names {
        outfiles.push(args.output.join(name));
    }

    Ok(outfiles)
```

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```
feat(cli): skip filename customization prompt in headless mode
```

---

### Task 6: Headless `lookup_tmdb()` — `--title` path

**Files:**
- Modify: `src/cli.rs` (`lookup_tmdb` function, around lines 117-276)

This is the most complex task. When `--title` is provided, skip TMDb entirely and build a `TmdbContext` using the provided title, `--year`, and sequential episode assignment.

- [ ] **Step 1: Add --title early return at the top of lookup_tmdb**

After initializing `ctx`, add:

```rust
    // --title flag: skip TMDb entirely, use provided title
    if let Some(ref title) = args.title {
        if movie_mode {
            let year = args.year.clone().unwrap_or_default();
            ctx.movie_title = Some((title.clone(), year));
        } else {
            ctx.show_name = Some(title.clone());

            // Determine season — required in headless TV mode
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

            // Sequential episode assignment (no TMDb data)
            let disc_number = label_info.as_ref().map(|l| l.disc);
            let start_ep = args
                .start_episode
                .unwrap_or_else(|| guess_start_episode(disc_number, episodes_pl.len()));

            let synthetic_episodes: Vec<crate::types::Episode> =
                (start_ep..start_ep + episodes_pl.len() as u32 * 2)
                    .map(|n| crate::types::Episode {
                        episode_number: n,
                        name: String::new(),
                        runtime: None,
                    })
                    .collect();

            ctx.episode_assignments =
                assign_episodes(episodes_pl, &synthetic_episodes, start_ep);
        }
        return Ok(ctx);
    }
```

Note: We generate `episodes_pl.len() * 2` synthetic episodes to account for multi-episode detection — `assign_episodes` may consume more than one episode per playlist for double-length playlists.

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```
feat(cli): --title flag skips TMDb and uses sequential episodes
```

---

### Task 7: Headless `lookup_tmdb()` — TMDb auto-resolve path

**Files:**
- Modify: `src/cli.rs` (`lookup_tmdb` function)

When headless without `--title`: skip API key prompt, auto-search using volume label, auto-select first result, auto-accept mappings.

- [ ] **Step 1: Make API key prompt headless-aware**

Replace the API key prompt block:

```rust
    if api_key.is_none() {
        if headless {
            // No API key in headless mode — skip TMDb
        } else {
            let input = prompt("TMDb API key not found. Enter key (or Enter to skip): ")?;
            if !input.is_empty() {
                tmdb::save_api_key(&input)?;
                println!("  Saved API key.");
                api_key = Some(input);
            }
        }
    }
```

- [ ] **Step 2: Make TMDb movie lookup headless-aware**

Replace the movie mode TMDb call. Change `prompt_tmdb_movie(key, default_query)?` to:

```rust
            if headless {
                headless_tmdb_movie(key, default_query)?
            } else {
                prompt_tmdb_movie(key, default_query)?
            }
```

Add the `headless_tmdb_movie` function:

```rust
fn headless_tmdb_movie(
    api_key: &str,
    default_query: &str,
) -> anyhow::Result<Option<(String, String)>> {
    if default_query.is_empty() {
        return Ok(None);
    }

    let results = match tmdb::search_movie(default_query, api_key) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("TMDb search failed: {}", e);
            return Ok(None);
        }
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

    eprintln!("TMDb: auto-selected \"{}\" ({})", movie.title, year);
    Ok(Some((movie.title.clone(), year)))
}
```

- [ ] **Step 3: Make TMDb TV lookup headless-aware**

Replace `prompt_tmdb(key, default_query, cli_season)?` with:

```rust
                if headless {
                    headless_tmdb_tv(key, default_query, cli_season)?
                } else {
                    prompt_tmdb(key, default_query, cli_season)?
                }
```

Add the `headless_tmdb_tv` function:

```rust
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
        Err(e) => {
            eprintln!("TMDb search failed: {}", e);
            return Ok(None);
        }
    };

    if results.is_empty() {
        return Ok(None);
    }

    let show = &results[0];
    let show_id = show.id;
    eprintln!("TMDb: auto-selected \"{}\"", show.name);

    let season_num = match cli_season {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "Cannot determine season number in headless mode. Use --season <NUM>."
            );
        }
    };

    let episodes = match tmdb::get_season(show_id, season_num, api_key) {
        Ok(eps) => eps,
        Err(e) => {
            eprintln!("Failed to fetch season: {}", e);
            return Ok(None);
        }
    };

    Ok(Some(TmdbLookupResult {
        episodes,
        season: season_num,
        show_name: show.name.clone(),
    }))
}
```

- [ ] **Step 4: Make starting episode prompt headless-aware**

In `lookup_tmdb`, replace the starting episode prompt:

```rust
                let start_ep = if args.start_episode.is_none() && !headless {
                    prompt_number(
                        &format!("  Starting episode number [{}]: ", default_start),
                        Some(default_start),
                    )?
                } else {
                    default_start
                };
```

- [ ] **Step 5: Make accept-mappings loop headless-aware**

Replace the mappings acceptance loop. After `ctx.episode_assignments = assign_episodes(...)`, change the loop:

```rust
                if !headless {
                    // Show mappings and prompt for accept/manual
                    loop {
                        println!("\n  Episode Mappings:");
                        // ... (keep existing display + prompt loop exactly as-is)
                    }
                }
```

The entire existing `loop { ... }` block (lines ~175-270) gets wrapped in `if !headless { ... }`.

- [ ] **Step 6: Handle headless fallback when TMDb produces no result**

After the entire `if let Some(ref key) = api_key { ... }` block, before `Ok(ctx)`, add fallback logic for headless mode when TMDb was skipped or failed:

```rust
    // Headless fallback: if no TMDb data and TV mode, build sequential assignments
    if headless && !movie_mode && ctx.episode_assignments.is_empty() && ctx.show_name.is_none() {
        // Use volume label for show name
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
        let start_ep = args
            .start_episode
            .unwrap_or_else(|| guess_start_episode(disc_number, episodes_pl.len()));

        let synthetic_episodes: Vec<crate::types::Episode> =
            (start_ep..start_ep + episodes_pl.len() as u32 * 2)
                .map(|n| crate::types::Episode {
                    episode_number: n,
                    name: String::new(),
                    runtime: None,
                })
                .collect();

        ctx.episode_assignments =
            assign_episodes(episodes_pl, &synthetic_episodes, start_ep);
    }
```

- [ ] **Step 7: Verify it compiles and tests pass**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 8: Commit**

```
feat(cli): headless TMDb auto-resolve and fallback logic
```

---

### Task 8: Wire `--year` into movie filename generation

**Files:**
- Modify: `src/cli.rs` (`build_filenames` function)

- [ ] **Step 1: Use `--year` in movie mode filename**

In `build_filenames`, in the movie title handling (around `if let Some((ref title, ref year)) = tmdb_ctx.movie_title`), the year already flows from `TmdbContext.movie_title`. The `--title` path in Task 6 already sets this from `args.year`. Verify this is working by checking the code path.

If `--year` is provided without `--title` in movie mode (TMDb auto-resolve), the TMDb year should take priority. The `--year` flag is only used in the `--title` path (Task 6 Step 1 already handles this: `let year = args.year.clone().unwrap_or_default();`).

No code changes needed if Task 6 is correct. Verify and move on.

- [ ] **Step 2: Commit (skip if no changes)**

---

### Task 9: Update CLAUDE.md and TODO.md

**Files:**
- Modify: `CLAUDE.md`
- Modify: `TODO.md`

- [ ] **Step 1: Update CLI Flags section in CLAUDE.md**

Add the new flags to the CLI Flags section:

```
      --yes / -y             Accept all defaults without prompting
      --title <STRING>       Set show/movie title directly (skips TMDb)
      --year <STRING>        Movie release year (with --title in --movie mode)
      --playlists <SEL>      Select specific playlists (e.g. 1,2,3 or 1-3)
      --list-playlists       Print playlist info and exit
```

- [ ] **Step 2: Mark TODO item as done**

In `TODO.md`, change:
```
- full headless run via CLI (no user input needed)
```
to:
```
- ~~full headless run via CLI (no user input needed)~~ Done: --yes/-y flag with --title, --year, --playlists, --list-playlists
```

- [ ] **Step 3: Commit**

```
docs: document headless CLI flags and mark TODO as done
```
