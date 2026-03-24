# Headless CLI Mode Design

## Problem

bluback's `--no-tui` CLI mode requires interactive input at 11 prompt points (TMDb search, show selection, season, episode mapping, playlist selection, filename customization). This makes it unusable in scripted, automated, or non-interactive contexts (cron jobs, CI pipelines, remote scripts).

## Goal

Enable fully unattended CLI runs by providing a `--yes` flag (and auto-detecting non-interactive stdin) that resolves all prompts to sensible defaults, plus new flags for the decisions that matter most in scripted workflows.

## New CLI Flags

| Flag | Short | Type | Description |
|------|-------|------|-------------|
| `--yes` | `-y` | bool | Accept all defaults without prompting. Auto-enabled when stdin is not a TTY. |
| `--title <STRING>` | | `Option<String>` | Set show (TV) or movie title directly. Skips TMDb lookup entirely. |
| `--year <STRING>` | | `Option<String>` | Movie release year for filename templates. Used with `--title` in `--movie` mode. Ignored outside movie mode. |
| `--playlists <SELECTION>` | | `Option<String>` | Select specific playlists (e.g. `1,2,3`, `1-3`, `all`). Defaults to `all` in headless mode. |
| `--list-playlists` | | bool | Scan disc and print playlist info, then exit. Silently ignores unrelated flags. |

## Headless Mode Activation

```rust
let headless = args.yes || (!atty_stdin() && !use_tui);
```

- `--yes` / `-y`: explicit opt-in from any context
- Non-TTY stdin + non-TUI mode: automatic (covers piped input, cron jobs, `ssh remote bluback`)
- TUI mode: `--yes` is ignored (TUI always interactive)

## Prompt Resolution Table

| Prompt | Headless behavior | Override flag |
|--------|-------------------|---------------|
| TMDb API key entry | Skip (no TMDb) | — |
| TMDb search query | Use volume label as query, auto-search | `--title` (skips TMDb entirely) |
| Show/movie selection from results | Auto-select first result | `--title` (skips TMDb entirely) |
| Season number | `--season` if provided, else volume label parse, else **error** | `--season` |
| Starting episode number | `--start-episode` if provided, else `guess_start_episode()` | `--start-episode` |
| Accept episode mappings | Auto-accept | — |
| Manual episode assignment | Never entered (auto-accept skips it) | — |
| Playlist selection | Use `--playlists` if provided, else select all | `--playlists` |
| Customize filenames? | No | — |
| Per-file name customization | Never entered | — |
| TMDb movie search + selection | Auto-search + auto-select first result | `--title` + `--year` |

### Error case

In headless TV mode, if season cannot be determined (no `--season` flag, volume label doesn't parse to a season number), bail with a clear error:

```
Error: Cannot determine season number in headless mode. Use --season <NUM>.
```

This is the one case where silently guessing would produce wrong filenames.

## `--title` Behavior

When `--title` is provided:
- The entire TMDb flow is skipped (no network calls)
- The title string is used directly as the show/movie name in filename templates
- Works in both TV and movie mode
- In movie mode, `--year` provides the year for filename templates; without it the year placeholder collapses via bracket groups
- **Episode assignment without TMDb**: In TV mode, episodes are assigned sequentially starting from `--start-episode` (or `guess_start_episode()` if not provided), using `Episode` objects with empty names. This produces filenames like `S01E01.mkv` rather than `S01E01_Episode_Title.mkv`. The `assign_episodes()` call must happen outside the TMDb lookup path when `--title` is set.

When `--title` is NOT provided in headless mode:
- TMDb auto-resolves using the volume label as the search query
- First result is auto-selected
- If TMDb key isn't configured or search fails, falls back to volume-label-based filenames with sequential episode numbering (same as above)

## `--list-playlists` Behavior

- Scans the disc (using `--device` or auto-detect)
- Prints **all** playlists (not filtered by `--min-duration`), showing: MPLS number, duration, chapter count
- Short playlists are marked with `*`; episode-length playlists show their filtered index (the number used with `--playlists`)
- Stream summary (video/audio codec info) deferred to a future enhancement — requires per-playlist FFmpeg probe which is slow
- Exits with code 0
- No ripping, no TMDb, no prompts
- Output is plain text with consistent column widths (grep/awk-friendly)
- Silently ignores all unrelated flags (`--yes`, `--dry-run`, `--season`, etc.)
- Dispatched early in `main.rs` after device resolution, before TUI/CLI mode split

## `--playlists` Behavior

- Accepts the same format as the interactive prompt: `1,2,3`, `1-3`, `all`
- **Indexing**: Numbers are 1-indexed positions in the *filtered* (episode-length) playlist list, not raw MPLS numbers. Position 1 = first playlist that passed `--min-duration` filtering. `--list-playlists` shows these indices in its "Sel" column so users can cross-reference.
- Invalid selections (out of range, unparseable) produce an error and exit
- In interactive CLI mode (not headless), `--playlists` skips the selection prompt
- In TUI mode, ignored
- Compatible with `--dry-run` (shows what would be ripped from the selected playlists)

## Compatibility Notes

- `--dry-run` works with headless mode: resolves all decisions using defaults/flags, prints what would be ripped, exits.
- `--yes` + `--settings` is a no-op conflict: `--settings` opens an interactive TUI panel. No clap conflict needed; `--yes` is simply irrelevant.
- `--title` without `--movie`: if auto-movie-detection triggers (single playlist, no `--season`), `--title` and `--year` apply to movie mode. This is the existing auto-detection behavior, not a new interaction.

## Files Changed

### `main.rs`
- Add `yes`, `title`, `year`, `playlists`, `list_playlists` fields to `Args` struct
- Add `atty_stdin()` helper
- Add `--list-playlists` early dispatch (after device resolution, before TUI/CLI split)
- Compute `headless` bool, pass to `cli::run`

### `cli.rs`
- `run()` accepts `headless: bool` parameter
- `lookup_tmdb()`: headless + `--title` skips TMDb; headless without `--title` auto-resolves; headless TV mode without determinable season errors out
- `display_and_select()`: `--playlists` or headless returns indices without prompting
- `build_filenames()`: headless skips customize prompt
- Each prompt site gets an `if headless { use default }` check at the call site (no changes to `prompt()` / `prompt_number()` helpers themselves)

### No changes to TUI mode
- `--yes`, `--title`, `--year`, `--playlists` are CLI-mode concerns
- TUI mode ignores these flags

## Example Workflows

```bash
# Fully headless TV rip (TMDb auto-resolve)
bluback -y --season 1 --start-episode 1 -o /export/media/tv/

# Headless with explicit title (no TMDb)
bluback -y --title "Breaking Bad" --season 1 -o /export/media/tv/

# Headless movie rip
bluback -y --movie --title "The Matrix" --year "1999" -o /export/media/movies/

# Inspect disc, then rip specific playlists
bluback --list-playlists
bluback -y --title "Breaking Bad" --season 1 --playlists 1-3 -o /export/media/tv/

# Cron job (headless auto-enabled via non-TTY stdin)
0 2 * * * bluback --season 1 --title "Show" -o /backup/tv/ 2>&1 | logger
```
