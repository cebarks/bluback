# Features Guide

## Chapter Preservation

bluback automatically extracts chapter markers from the Blu-ray's MPLS playlist files and embeds them directly into the output MKV files during remux via the FFmpeg AVChapter API. No external tools are needed.

The disc is temporarily mounted to read the playlist data from `BDMV/PLAYLIST/`, then unmounted after extraction. Chapter counts are displayed alongside each playlist in TUI mode.

## Stream Selection

By default, bluback includes all video, audio, and subtitle streams from each playlist. You can filter streams in three ways:

**Config defaults** — Set language preferences in `config.toml` that apply to every rip:

```toml
[streams]
audio_languages = ["eng", "jpn"]
subtitle_languages = ["eng"]
prefer_surround = true
```

**CLI flags** — Override config for a single run:

```bash
# Keep only English and Japanese audio, English subtitles
bluback --audio-lang eng,jpn --subtitle-lang eng

# Select specific streams by index (use --list-playlists -v to see indices)
bluback --tracks "a:1;s:0-1"

# Override config filters, include everything
bluback --all-streams
```

**TUI track picker** — Press `t` on any playlist in the Playlist Manager to expand its stream list. Toggle individual streams with `Space`. Custom selections are shown in the Ch column as `1v 2a 3s*`.

Language filters are preferences, not hard requirements: if no streams match a configured language, all streams of that type are included with a warning. Streams without language tags (`und`) are always included.

## Rip Verification

bluback can verify output files after ripping to catch corruption:

```bash
bluback --verify                      # Quick: probe headers (duration, streams, chapters)
bluback --verify --verify-level full  # Full: headers + sample frame decode at 5 seek points
```

Enable by default in config: `verify = true`, `verify_level = "quick"`.

Quick verification checks:
- Duration matches expected (2% tolerance)
- Stream counts match source
- Chapter count matches source

Full verification adds sample frame decode at 5 evenly-spaced seek points.

In TUI mode, failed verification prompts to delete & retry, keep, or skip. In CLI mode, a warning is logged and the file is kept.

Hook variables `{verify}` (passed/failed/skipped) and `{verify_detail}` (failed check names) are available for post-rip hooks.

## Batch Mode

Continuous multi-disc ripping: rip → eject → wait for next disc → auto-start.

```bash
bluback --batch -o ~/rips
```

Or enable in config: `batch = true`.

In batch mode:
- Episode numbers auto-advance across discs (specials excluded)
- TUI auto-restarts the wizard when a new disc is detected
- CLI uses an outer loop with disc polling (2-second interval)
- A disc counter is shown in the TUI title

`--batch` conflicts with `--dry-run`, `--list-playlists`, `--check`, `--settings`, and `--no-eject`.

## Post-Rip Hooks

User-configurable shell commands that run after individual rips and/or session completion.

```toml
# Runs after each file is ripped
[post_rip]
command = "notify-send 'Ripped {filename}'"
on_failure = false    # Also run on failed rips
blocking = true       # Wait for hook to finish
log_output = false    # Log hook stdout/stderr

# Runs after all files are done
[post_session]
command = "echo '{succeeded}/{total} files ripped from {label}'"
```

Disable for a run with `--no-hooks`.

### Per-File Template Variables

`{file}`, `{filename}`, `{dir}`, `{size}`, `{chapters}`, `{title}`, `{season}`, `{episode}`, `{episode_name}`, `{playlist}`, `{label}`, `{mode}`, `{device}`, `{status}`, `{error}`, `{verify}`, `{verify_detail}`

### Per-Session Template Variables

`{total}`, `{succeeded}`, `{failed}`, `{skipped}`, `{label}`, `{mode}`, `{device}`

Hook failures are logged but never fail the rip.

## MKV Metadata

bluback auto-generates and embeds metadata tags during remux:

| Tag | Value |
|-----|-------|
| `TITLE` | Episode or movie title |
| `SHOW` | Show name (TV mode) |
| `SEASON_NUMBER` | Season number (TV mode) |
| `EPISODE_SORT` | Episode number (TV mode) |
| `DATE_RELEASED` | Release date (from TMDb) |
| `REMUXED_WITH` | `bluback vX.Y.Z` |

Custom tags can be added via config:

```toml
[metadata]
enabled = true
[metadata.tags]
CUSTOM_TAG = "custom value"
```

Custom tags override auto-generated ones on name conflict. Empty values are never written.

Disable per-run with `--no-metadata`.

## Rip History

SQLite database tracking every disc scan and rip session. Default location: `~/.local/share/bluback/history.db`.

### Features

- **Episode continuation** — remembers the last episode ripped for a show/season, auto-advances the starting episode for the next disc
- **Duplicate detection** — warns when re-ripping a disc that's already been ripped (by volume label or TMDb ID)
- **Retention auto-prune** — automatically cleans old sessions on startup based on configured retention period

### CLI Management

```bash
bluback history                          # List recent sessions
bluback history list --status completed  # Filter by status
bluback history show 42                  # Session details + files
bluback history stats                    # Aggregate summary
bluback history delete 42 43             # Delete sessions
bluback history clear --older-than 90d   # Prune old sessions
bluback history export                   # JSON dump
```

See [CLI Reference](cli-reference.md) for full flag details.

### TUI Integration

Press `Ctrl+H` during any screen to open the history overlay. Browse sessions, view details, and delete entries without leaving the wizard.

Contextual hints appear on wizard screens showing continuation info and duplicate warnings.

### Configuration

```toml
[history]
enabled = true
path = "~/.local/share/bluback/history.db"
retention = "90d"
retention_statuses = ["completed", "scanned"]
```

Disable per-run with `--no-history`. Skip duplicate detection/continuation (but still record) with `--ignore-history`.

## Auto-Detection

Optional heuristic system that pre-marks likely specials and multi-episode playlists.

```bash
bluback --auto-detect
```

Or enable in config: `auto_detect = true`.

### How It Works

**Layer 1 — Duration heuristics:**
- Playlists <50% of median duration → high-confidence special
- Playlists 50-75% of median → medium-confidence special
- Playlists >200% of median → likely multi-episode

Additional signals: stream count anomalies, chapter count anomalies.

**Layer 2 — TMDb runtime matching:**
- Compares playlist durations against TMDb episode runtimes (±10% or ±3min tolerance)
- Fetches season 0 for special matching

Three confidence levels (High/Medium/Low). In TUI mode, high-confidence suggestions are pre-marked. Press `A` in the Playlist Manager to accept all medium+ suggestions. In headless CLI mode, high-confidence suggestions are auto-applied.

`--specials` takes precedence over auto-detection. Conflicts with `--movie`.

## Multi-Drive Support

bluback supports parallel ripping from multiple Blu-ray drives simultaneously.

### How It Works

A background drive monitor polls for optical drives every 2 seconds. Each detected drive gets its own independent session with a dedicated tab in the TUI.

- **Tab switching:** `Tab` / `Shift+Tab` to switch between drive sessions
- **Inter-session linking:** `Ctrl+L` copies TMDb context (show name, season, next episode) from one session to another — avoids redundant TMDb lookups across discs of the same show
- **Episode overlap detection:** warns if two sessions assign the same episode for the same show/season

### Configuration

```toml
# "auto" (default): TUI auto-detects all drives
# "manual": single device mode
multi_drive = "auto"
```

Multi-drive is TUI-only. CLI mode uses a single drive specified by `-d`.
