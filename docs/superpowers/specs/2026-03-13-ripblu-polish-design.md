# ripblu Polish — Design Spec

## Summary

Focused cleanup of the ripblu Blu-ray ripper script. Fixes broken episode matching, adds proper argument parsing, improves error handling, and adds volume label parsing for a smoother interactive experience. Stays as a single-file pure-stdlib Python script.

## Changes

### 1. Sequential Episode Assignment

**Problem**: Duration-based episode matching (`match_episodes()`) fails when episodes have similar runtimes, which is common for TV series.

**Solution**: Replace with sequential assignment.

- Remove `match_episodes()` function entirely
- After TMDb season lookup, show the full episode list and ask for a starting episode number
- Filter playlists to episode-length only (configurable threshold, default 15 min / 900s) to exclude menus, trailers, and extras
- Assign episodes in playlist order: first playlist → start_ep, second → start_ep+1, etc.
- TMDb provides episode names for display and filenames

**Episode start guessing**: If the volume label contains a disc number, use the count of episode-length playlists on the current disc as the assumed per-disc count, then: `start_ep = 1 + episode_count * (disc_number - 1)`. Example: 5 episode-length playlists on disc 2 → guess start at E06. Show as an editable default, not a hard assumption.

**Overflow handling**: If `start_ep + playlist_count` exceeds the TMDb episode count, playlists beyond the last known episode fall back to `playlistNNNNN` naming.

### 2. Volume Label Parsing

Read the volume label via `lsblk -no LABEL <device>`. Parse common patterns:

| Label Pattern | Extracted Info |
|---|---|
| `SGU_BR_S1D2` | show=SGU, season=1, disc=2 |
| `SHOW_S1_D2` | show=SHOW, season=1, disc=2 |
| `SHOW_SEASON1_DISC2` | show=SHOW, season=1, disc=2 |

Use regex with named groups to handle variants. Extracted values become defaults for interactive prompts (TMDb search query, season number, disc number for episode guessing). If parsing fails, user enters values manually — same as today.

### 3. Argparse

Replace `sys.argv` slicing with `argparse`. Arguments:

| Flag | Short | Default | Description |
|---|---|---|---|
| `--device` | `-d` | `/dev/sr0` | Blu-ray device path |
| `--output` | `-o` | cwd | Output directory |
| `--season` | `-s` | (interactive) | Season number, skips prompt |
| `--start-episode` | `-e` | (interactive) | Starting episode number, skips prompt |
| `--min-duration` | | `900` | Minimum seconds to consider a playlist an episode |
| `--dry-run` | | `false` | Show what would be ripped without ripping |

All interactive prompts still function when flags are not provided.

### 4. Error Handling & Validation

- **Dependency check**: Verify `ffprobe` and `ffmpeg` are on PATH at startup, exit with clear message if missing
- **Input validation**: Validate user input against actual playlist count. On invalid input, print error and re-prompt (loop until valid). Same pattern for all interactive inputs (show selection, season, episode start, playlist selection).
- **ffprobe failure**: Check return code from `scan_playlists()` and `probe_streams()`. If `scan_playlists` fails with no playlists, exit with error (already handled). If `probe_streams` fails mid-rip, skip that playlist with a warning and continue to the next.
- **KeyboardInterrupt**: Catch at top level for clean exit (`sys.exit(130)`)
- **TMDb errors**: Already partially handled; ensure all API calls are wrapped
- **`--season` / `--start-episode` without TMDb**: If these flags are passed but no TMDb API key is configured, print a warning that episode naming requires TMDb and ignore the flags

### 5. Unchanged

- Single-file script (no package restructuring)
- Pure Python stdlib (no pip dependencies)
- Lossless remux via ffmpeg `-c copy`
- Audio selection logic (surround preferred, stereo as secondary)
- All subtitle streams included
- TMDb API key storage at `~/.config/ripblu/tmdb_api_key`

## Interactive Flow (Updated)

```
$ ripblu

Scanning disc at /dev/sr0...
Found 24 playlists (5 episode-length, 19 short/extras).

Volume label: SGU_BR_S1D2
Search TMDb for episode info [SGU]:
  1. Stargate Universe (2009)
  2. ...
Select show (1-5): 1
Season number [1]:
  Season 1: 20 episodes
  E01 - Air (Part 1)           (44 min)
  E02 - Air (Part 2)           (44 min)
  ...
  E20 - Incursion (Part 2)     (44 min)
Starting episode number [6]:

  #   Playlist  Duration    Episode
  ---  --------  --------    -------
  1   00003     0:43:22     S01E06 - Water
  2   00004     0:43:18     S01E07 - Earth (Part 1)
  3   00005     0:43:44     S01E08 - Earth (Part 2)
  4   00006     0:43:10     S01E09 - Life
  5   00007     0:43:30     S01E10 - Justice

Select playlists to rip (1,2,3 or 1-3 or 'all'): all

  Name for playlist 00003 [S01E06_Water]:
  Name for playlist 00004 [S01E07_Earth_(Part_1)]:
  ...

Ripping playlist 00003 (0:43:22) -> S01E06_Water.mkv
Done: S01E06_Water.mkv (4.2 GB)
...
All done! Ripped 5 playlist(s) to /home/anten/rips
```

## Dry Run Behavior

`--dry-run` runs the full interactive flow (scanning, TMDb lookup, playlist selection, naming) but skips the actual ffmpeg invocations. After the naming step, instead of ripping, it prints a summary:

```
[DRY RUN] Would rip:
  00003 (0:43:22) -> S01E06_Water.mkv
  00004 (0:43:18) -> S01E07_Earth_(Part_1).mkv
  ...
```
