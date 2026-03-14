# ripblu Polish Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Polish the ripblu script — fix broken episode matching, add argparse, volume label parsing, error handling, and dry-run support.

**Architecture:** Single-file Python script (`ripblu.py`) with pure stdlib dependencies. All changes stay within this one file. Tests go in `tests/test_ripblu.py` testing the pure functions.

**Tech Stack:** Python 3.12+, stdlib only (argparse, subprocess, json, urllib, re, shutil, pathlib)

**Spec:** `docs/superpowers/specs/2026-03-13-ripblu-polish-design.md`

---

## Chunk 1: Tests and Pure Function Changes

### Task 1: Set up test file and test existing pure functions

**Files:**
- Create: `tests/test_ripblu.py`

- [ ] **Step 1: Create test file with tests for existing pure functions**

```python
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from ripblu import duration_to_seconds, sanitize_filename


class TestDurationToSeconds:
    def test_hms(self):
        assert duration_to_seconds("1:23:45") == 5025

    def test_ms(self):
        assert duration_to_seconds("23:45") == 1425

    def test_zeros(self):
        assert duration_to_seconds("0:00:00") == 0

    def test_invalid(self):
        assert duration_to_seconds("") == 0


class TestSanitizeFilename:
    def test_spaces_to_underscores(self):
        assert sanitize_filename("Hello World") == "Hello_World"

    def test_removes_special_chars(self):
        assert sanitize_filename('foo/bar:baz"qux') == "foobarbazqux"

    def test_preserves_parens(self):
        assert sanitize_filename("Earth (Part 1)") == "Earth_(Part_1)"
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `python -m pytest tests/test_ripblu.py -v`
Expected: All PASS (these test existing, working functions)

- [ ] **Step 3: Commit**

`test: add tests for existing pure functions`

### Task 2: Add volume label parsing

**Files:**
- Modify: `ripblu.py` (add `parse_volume_label` and `get_volume_label` functions)
- Modify: `tests/test_ripblu.py` (add tests)

- [ ] **Step 1: Write failing tests for `parse_volume_label`**

Add to `tests/test_ripblu.py`:

```python
from ripblu import parse_volume_label


class TestParseVolumeLabel:
    def test_sXdY_format(self):
        result = parse_volume_label("SGU_BR_S1D2")
        assert result == {"show": "SGU", "season": 1, "disc": 2}

    def test_sX_dY_underscore_separated(self):
        result = parse_volume_label("SHOW_S1_D2")
        assert result == {"show": "SHOW", "season": 1, "disc": 2}

    def test_season_disc_long_form(self):
        result = parse_volume_label("SHOW_SEASON1_DISC2")
        assert result == {"show": "SHOW", "season": 1, "disc": 2}

    def test_no_match(self):
        result = parse_volume_label("RANDOM_DISC")
        assert result == {}

    def test_empty_string(self):
        result = parse_volume_label("")
        assert result == {}

    def test_show_with_underscores_before_season(self):
        result = parse_volume_label("THE_WIRE_S3D1")
        assert result == {"show": "THE WIRE", "season": 3, "disc": 1}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python -m pytest tests/test_ripblu.py::TestParseVolumeLabel -v`
Expected: FAIL with ImportError

- [ ] **Step 3: Implement `parse_volume_label` and `get_volume_label`**

Add to `ripblu.py` after the `sanitize_filename` function:

```python
def parse_volume_label(label: str) -> dict:
    """Parse a Blu-ray volume label for show name, season, and disc number."""
    if not label:
        return {}
    patterns = [
        r"^(?P<show>.+?)_?SEASON(?P<season>\d+)_?DISC(?P<disc>\d+)",
        r"^(?P<show>.+?)_S(?P<season>\d+)_?D(?P<disc>\d+)",
    ]
    for pattern in patterns:
        m = re.match(pattern, label, re.IGNORECASE)
        if m:
            show = m.group("show").strip("_").replace("_", " ")
            return {
                "show": show,
                "season": int(m.group("season")),
                "disc": int(m.group("disc")),
            }
    return {}


def get_volume_label(device: str) -> str:
    """Get volume label from a block device via lsblk."""
    try:
        result = subprocess.run(
            ["lsblk", "-no", "LABEL", device],
            capture_output=True, text=True, timeout=5,
        )
        return result.stdout.strip()
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return ""
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python -m pytest tests/test_ripblu.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

`feat: add volume label parsing for auto-detecting show/season/disc`

### Task 3: Add playlist filtering function

**Files:**
- Modify: `ripblu.py` (add `filter_episodes` function)
- Modify: `tests/test_ripblu.py` (add tests)

- [ ] **Step 1: Write failing tests for `filter_episodes`**

Add to `tests/test_ripblu.py`:

```python
from ripblu import filter_episodes


class TestFilterEpisodes:
    def test_filters_short_playlists(self):
        playlists = [
            {"num": "00001", "duration": "0:00:30", "seconds": 30},
            {"num": "00002", "duration": "0:43:00", "seconds": 2580},
            {"num": "00003", "duration": "0:44:00", "seconds": 2640},
            {"num": "00004", "duration": "0:02:00", "seconds": 120},
        ]
        result = filter_episodes(playlists, min_duration=900)
        assert len(result) == 2
        assert result[0]["num"] == "00002"
        assert result[1]["num"] == "00003"

    def test_all_long(self):
        playlists = [
            {"num": "00001", "duration": "0:43:00", "seconds": 2580},
            {"num": "00002", "duration": "0:44:00", "seconds": 2640},
        ]
        result = filter_episodes(playlists, min_duration=900)
        assert len(result) == 2

    def test_all_short(self):
        playlists = [
            {"num": "00001", "duration": "0:00:30", "seconds": 30},
        ]
        result = filter_episodes(playlists, min_duration=900)
        assert len(result) == 0
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python -m pytest tests/test_ripblu.py::TestFilterEpisodes -v`
Expected: FAIL with ImportError

- [ ] **Step 3: Implement `filter_episodes`**

Add to `ripblu.py` after `scan_playlists`:

```python
def filter_episodes(playlists: list[dict], min_duration: int = 900) -> list[dict]:
    """Filter playlists to only those likely to be episodes (above min_duration seconds)."""
    return [pl for pl in playlists if pl["seconds"] >= min_duration]
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python -m pytest tests/test_ripblu.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

`feat: add playlist filtering to separate episodes from menus/extras`

### Task 4: Add `guess_start_episode` function

**Files:**
- Modify: `ripblu.py` (add function)
- Modify: `tests/test_ripblu.py` (add tests)

- [ ] **Step 1: Write failing tests**

Add to `tests/test_ripblu.py`:

```python
from ripblu import guess_start_episode


class TestGuessStartEpisode:
    def test_disc_1(self):
        assert guess_start_episode(disc_number=1, episodes_on_disc=5) == 1

    def test_disc_2(self):
        assert guess_start_episode(disc_number=2, episodes_on_disc=5) == 6

    def test_disc_3(self):
        assert guess_start_episode(disc_number=3, episodes_on_disc=4) == 9

    def test_no_disc_number(self):
        assert guess_start_episode(disc_number=None, episodes_on_disc=5) == 1

    def test_zero_episodes(self):
        assert guess_start_episode(disc_number=2, episodes_on_disc=0) == 1
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python -m pytest tests/test_ripblu.py::TestGuessStartEpisode -v`
Expected: FAIL with ImportError

- [ ] **Step 3: Implement `guess_start_episode`**

Add to `ripblu.py` after `filter_episodes`:

```python
def guess_start_episode(disc_number: int | None, episodes_on_disc: int) -> int:
    """Guess starting episode number based on disc number and episodes per disc."""
    if not disc_number or disc_number < 1 or episodes_on_disc < 1:
        return 1
    return 1 + episodes_on_disc * (disc_number - 1)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python -m pytest tests/test_ripblu.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

`feat: add episode start guessing from disc number`

### Task 5: Add `assign_episodes` function (replaces `match_episodes`)

**Files:**
- Modify: `ripblu.py` (add `assign_episodes`, remove `match_episodes`)
- Modify: `tests/test_ripblu.py` (add tests)

- [ ] **Step 1: Write failing tests**

Add to `tests/test_ripblu.py`:

```python
from ripblu import assign_episodes


class TestAssignEpisodes:
    def test_basic_assignment(self):
        playlists = [
            {"num": "00001", "duration": "0:43:00", "seconds": 2580},
            {"num": "00002", "duration": "0:44:00", "seconds": 2640},
        ]
        episodes = [
            {"episode_number": 1, "name": "Pilot", "runtime": 44},
            {"episode_number": 2, "name": "Second", "runtime": 44},
            {"episode_number": 3, "name": "Third", "runtime": 44},
        ]
        result = assign_episodes(playlists, episodes, start_episode=1)
        assert result["00001"]["name"] == "Pilot"
        assert result["00002"]["name"] == "Second"

    def test_start_offset(self):
        playlists = [
            {"num": "00003", "duration": "0:43:00", "seconds": 2580},
        ]
        episodes = [
            {"episode_number": 1, "name": "Pilot", "runtime": 44},
            {"episode_number": 2, "name": "Second", "runtime": 44},
            {"episode_number": 3, "name": "Third", "runtime": 44},
        ]
        result = assign_episodes(playlists, episodes, start_episode=3)
        assert result["00003"]["name"] == "Third"

    def test_overflow_past_episode_list(self):
        playlists = [
            {"num": "00001", "duration": "0:43:00", "seconds": 2580},
            {"num": "00002", "duration": "0:44:00", "seconds": 2640},
        ]
        episodes = [
            {"episode_number": 1, "name": "Pilot", "runtime": 44},
        ]
        result = assign_episodes(playlists, episodes, start_episode=1)
        assert result["00001"]["name"] == "Pilot"
        assert "00002" not in result

    def test_empty_episodes(self):
        playlists = [
            {"num": "00001", "duration": "0:43:00", "seconds": 2580},
        ]
        result = assign_episodes(playlists, [], start_episode=1)
        assert result == {}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python -m pytest tests/test_ripblu.py::TestAssignEpisodes -v`
Expected: FAIL with ImportError

- [ ] **Step 3: Implement `assign_episodes` and remove `match_episodes`**

Remove the `match_episodes` function (lines 127-145 of current `ripblu.py`).

Add in its place:

```python
def assign_episodes(playlists: list[dict], episodes: list[dict], start_episode: int) -> dict[str, dict]:
    """Assign episodes to playlists sequentially starting from start_episode.
    Returns {playlist_num: episode} for playlists that have a matching episode."""
    assignments = {}
    ep_by_num = {ep["episode_number"]: ep for ep in episodes}
    for i, pl in enumerate(playlists):
        ep_num = start_episode + i
        if ep_num in ep_by_num:
            assignments[pl["num"]] = ep_by_num[ep_num]
    return assignments
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python -m pytest tests/test_ripblu.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

`feat: replace duration-based episode matching with sequential assignment`

---

## Chunk 2: Argparse, Main Rewrite, Error Handling

### Task 6: Add dependency checking and input validation helpers

**Files:**
- Modify: `ripblu.py` (add `check_dependencies` and `prompt_validated` functions)
- Modify: `tests/test_ripblu.py` (add tests)

- [ ] **Step 1: Write failing tests**

Add to `tests/test_ripblu.py`:

```python
from ripblu import parse_selection


class TestParseSelection:
    def test_single_number(self):
        assert parse_selection("2", max_val=5) == [1]

    def test_comma_separated(self):
        assert parse_selection("1,3,5", max_val=5) == [0, 2, 4]

    def test_range(self):
        assert parse_selection("2-4", max_val=5) == [1, 2, 3]

    def test_mixed(self):
        assert parse_selection("1,3-5", max_val=5) == [0, 2, 3, 4]

    def test_all(self):
        assert parse_selection("all", max_val=3) == [0, 1, 2]

    def test_out_of_bounds(self):
        assert parse_selection("6", max_val=5) is None

    def test_zero(self):
        assert parse_selection("0", max_val=5) is None

    def test_invalid(self):
        assert parse_selection("abc", max_val=5) is None

    def test_empty(self):
        assert parse_selection("", max_val=5) is None

    def test_reversed_range(self):
        assert parse_selection("4-2", max_val=5) is None
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python -m pytest tests/test_ripblu.py::TestParseSelection -v`
Expected: FAIL with ImportError

- [ ] **Step 3: Implement `check_dependencies` and `parse_selection`**

Add `import shutil` to the imports block (after `import sys`), then add these functions to `ripblu.py`:

```python
def check_dependencies():
    """Verify ffmpeg and ffprobe are available on PATH."""
    missing = []
    for cmd in ("ffmpeg", "ffprobe"):
        if shutil.which(cmd) is None:
            missing.append(cmd)
    if missing:
        print(f"Error: required commands not found: {', '.join(missing)}")
        print("Install ffmpeg with libbluray support.")
        sys.exit(1)


def parse_selection(text: str, max_val: int) -> list[int] | None:
    """Parse a playlist selection string into 0-based indices.
    Returns None if the input is invalid."""
    text = text.strip()
    if not text:
        return None
    if text == "all":
        return list(range(max_val))

    indices = []
    try:
        for part in text.split(","):
            part = part.strip()
            if "-" in part:
                start_s, end_s = part.split("-", 1)
                start, end = int(start_s), int(end_s)
                if start > end or start < 1 or end > max_val:
                    return None
                indices.extend(range(start - 1, end))
            else:
                val = int(part)
                if val < 1 or val > max_val:
                    return None
                indices.append(val - 1)
    except ValueError:
        return None
    return indices if indices else None
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python -m pytest tests/test_ripblu.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

`feat: add dependency check and validated playlist selection parser`

### Task 7: Replace sys.argv with argparse

**Files:**
- Modify: `ripblu.py` (add `build_parser` function, update `main`)

- [ ] **Step 1: Implement argparse**

Add to `ripblu.py` before `main()`:

```python
import argparse


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="ripblu",
        description="Rip Blu-ray discs to MKV files using ffmpeg + libaacs",
    )
    parser.add_argument("-d", "--device", default=DEFAULT_DEVICE,
                        help=f"Blu-ray device path (default: {DEFAULT_DEVICE})")
    parser.add_argument("-o", "--output", default=".",
                        help="Output directory (default: current directory)")
    parser.add_argument("-s", "--season", type=int, default=None,
                        help="Season number (skips interactive prompt)")
    parser.add_argument("-e", "--start-episode", type=int, default=None,
                        help="Starting episode number (skips interactive prompt)")
    parser.add_argument("--min-duration", type=int, default=900,
                        help="Minimum seconds to consider a playlist an episode (default: 900)")
    parser.add_argument("--dry-run", action="store_true",
                        help="Show what would be ripped without ripping")
    return parser
```

- [ ] **Step 2: Update `main()` to use argparse instead of sys.argv**

Replace the argument parsing block at the top of `main()` (lines 191-203) with:

```python
def main():
    parser = build_parser()
    args = parser.parse_args()

    device = args.device
    outdir = args.output
```

Remove the old `sys.argv` handling and `import sys` is kept (still used for `sys.exit`).

- [ ] **Step 3: Verify the script still shows help**

Run: `python ripblu.py --help`
Expected: Shows argparse-generated help with all flags

- [ ] **Step 4: Commit**

`feat: replace hand-rolled argv parsing with argparse`

### Task 8: Rewrite `main()` with new interactive flow

This is the core integration task. Rewrites `main()` to use all the new functions: volume label parsing, playlist filtering, sequential episode assignment, validated input, and dry-run support.

**Files:**
- Modify: `ripblu.py` (rewrite `main()` and `prompt_tmdb()`)

- [ ] **Step 1: Rewrite `prompt_tmdb` to accept defaults and season from CLI**

Replace the existing `prompt_tmdb` function with:

```python
def prompt_tmdb(api_key: str, default_query: str = "",
                cli_season: int | None = None) -> tuple[list[dict] | None, int | None, int | None]:
    """Interactive TMDb search. Returns (episodes, show_id, season_num) or (None, None, None)."""
    default_hint = f" [{default_query}]" if default_query else ""
    query = input(f"\nSearch TMDb for episode info{default_hint}: ").strip()
    query = query or default_query
    if not query:
        return None, None, None

    try:
        results = tmdb_search_show(query, api_key)
    except urllib.error.URLError as e:
        print(f"  TMDb search failed: {e}")
        return None, None, None

    if not results:
        print("  No results found.")
        return None, None, None

    print("\n  Results:")
    for i, show in enumerate(results[:5]):
        year = show.get("first_air_date", "")[:4]
        print(f"    {i+1}. {show['name']} ({year})")

    while True:
        pick = input("  Select show (1-5, or Enter to skip): ").strip()
        if not pick:
            return None, None, None
        if pick.isdigit() and 1 <= int(pick) <= len(results[:5]):
            break
        print("  Invalid selection.")

    show = results[int(pick) - 1]
    show_id = show["id"]

    if cli_season is not None:
        season_num = cli_season
        print(f"  Using season {season_num} (from --season flag)")
    else:
        while True:
            season_str = input("  Season number: ").strip()
            if season_str.isdigit() and int(season_str) > 0:
                season_num = int(season_str)
                break
            print("  Invalid season number.")

    try:
        episodes = tmdb_get_season(show_id, season_num, api_key)
    except urllib.error.URLError as e:
        print(f"  Failed to fetch season: {e}")
        return None, None, None

    if episodes:
        print(f"\n  Season {season_num}: {len(episodes)} episodes")
        for ep in episodes:
            runtime = ep.get("runtime") or 0
            print(f"    E{ep['episode_number']:02d} - {ep['name']}  ({runtime} min)")

    return episodes, show_id, season_num
```

- [ ] **Step 2: Rewrite `main()` with the new flow**

```python
def main():
    parser = build_parser()
    args = parser.parse_args()

    device = args.device
    outdir = args.output

    check_dependencies()

    if not Path(device).is_block_device():
        print(f"Error: No Blu-ray device found at {device}")
        sys.exit(1)

    # Read volume label
    label = get_volume_label(device)
    label_info = parse_volume_label(label)
    if label:
        print(f"Volume label: {label}")

    print(f"Scanning disc at {device}...")
    playlists = scan_playlists(device)

    if not playlists:
        print("Error: No playlists found. Check libaacs and KEYDB.cfg.")
        sys.exit(1)

    # Filter to episode-length playlists
    episodes_pl = filter_episodes(playlists, args.min_duration)
    short_count = len(playlists) - len(episodes_pl)
    print(f"Found {len(playlists)} playlists ({len(episodes_pl)} episode-length, {short_count} short/extras).\n")

    if not episodes_pl:
        print("No episode-length playlists found. Try lowering --min-duration.")
        sys.exit(1)

    # TMDb lookup
    episode_assignments = {}
    season_num = args.season or label_info.get("season")
    api_key = get_tmdb_api_key()

    if api_key is None:
        setup = input("TMDb API key not found. Enter key (or Enter to skip): ").strip()
        if setup:
            save_tmdb_api_key(setup)
            api_key = setup
            print(f"  Saved to {TMDB_API_KEY_FILE}")

    if api_key is None and (args.season or args.start_episode):
        print("Warning: --season/--start-episode require TMDb. Ignoring.")

    if api_key:
        default_query = label_info.get("show", "")
        episodes, show_id, season_num = prompt_tmdb(api_key, default_query, args.season or label_info.get("season"))

        if episodes:
            # Determine starting episode
            disc_number = label_info.get("disc")
            default_start = args.start_episode
            if default_start is None:
                default_start = guess_start_episode(disc_number, len(episodes_pl))

            if args.start_episode is None:
                while True:
                    start_input = input(f"  Starting episode number [{default_start}]: ").strip()
                    if not start_input:
                        start_ep = default_start
                        break
                    if start_input.isdigit() and int(start_input) > 0:
                        start_ep = int(start_input)
                        break
                    print("  Invalid episode number.")
            else:
                start_ep = args.start_episode

            episode_assignments = assign_episodes(episodes_pl, episodes, start_ep)

    # Display playlists
    has_eps = bool(episode_assignments)
    header_match = "  Episode" if has_eps else ""
    print(f"\n  {'#':<4}  {'Playlist':<10}  {'Duration':<10}{header_match}")
    print(f"  {'---':<4}  {'--------':<10}  {'--------':<10}{'-' * len(header_match)}")

    for i, pl in enumerate(episodes_pl):
        ep = episode_assignments.get(pl["num"])
        ep_str = ""
        if ep:
            ep_str = f"  S{season_num:02d}E{ep['episode_number']:02d} - {ep['name']}"
        elif has_eps:
            ep_str = "  (no episode data)"
        print(f"  {i+1:<4}  {pl['num']:<10}  {pl['duration']:<10}{ep_str}")

    print()

    # Select playlists
    while True:
        selection = input("Select playlists to rip (e.g. 1,2,3 or 1-3 or 'all'): ").strip()
        selected = parse_selection(selection, len(episodes_pl))
        if selected is not None:
            break
        print("Invalid selection. Try again.")

    # Name each playlist
    print()
    outfiles = []
    for idx in selected:
        pl = episodes_pl[idx]
        ep = episode_assignments.get(pl["num"])

        if ep and season_num is not None:
            default_name = f"S{season_num:02d}E{ep['episode_number']:02d}_{sanitize_filename(ep['name'])}"
        else:
            default_name = f"playlist{pl['num']}"

        name = input(f"  Name for playlist {pl['num']} ({pl['duration']}) [{default_name}]: ").strip()
        name = name or default_name
        name = sanitize_filename(name)
        outfiles.append(Path(outdir) / f"{name}.mkv")

    # Dry run
    if args.dry_run:
        print("\n[DRY RUN] Would rip:")
        for i, idx in enumerate(selected):
            pl = episodes_pl[idx]
            print(f"  {pl['num']} ({pl['duration']}) -> {outfiles[i].name}")
        return

    Path(outdir).mkdir(parents=True, exist_ok=True)

    # Rip
    for i, idx in enumerate(selected):
        pl = episodes_pl[idx]
        outfile = outfiles[i]

        print(f"\nRipping playlist {pl['num']} ({pl['duration']}) -> {outfile.name}")

        streams = probe_streams(device, pl["num"])
        if streams is None:
            print(f"Warning: Failed to probe streams for playlist {pl['num']}, skipping.")
            continue

        map_args = build_map_args(streams)

        cmd = [
            "ffmpeg", "-y",
            "-playlist", pl["num"],
            "-i", f"bluray:{device}",
            *map_args,
            "-c", "copy",
            str(outfile),
        ]

        result = subprocess.run(cmd)
        if result.returncode != 0:
            print(f"Error: ffmpeg exited with code {result.returncode}")
            continue

        size = outfile.stat().st_size / (1024 ** 3)
        print(f"Done: {outfile.name} ({size:.1f} GB)")

    print(f"\nAll done! Ripped {len(selected)} playlist(s) to {outdir}")
```

- [ ] **Step 3: Update `scan_playlists` and `probe_streams` with error handling**

Add `TimeoutExpired` handling to `scan_playlists`:

```python
def scan_playlists(device: str) -> list[dict]:
    try:
        result = subprocess.run(
            ["ffprobe", "-i", f"bluray:{device}"],
            capture_output=True, text=True, timeout=60,
        )
    except subprocess.TimeoutExpired:
        print("Error: ffprobe timed out while scanning disc.")
        return []

    output = result.stdout + result.stderr

    playlists = []
    for line in output.splitlines():
        m = re.search(r"playlist (\d+)\.mpls \((\d+:\d+:\d+)\)", line)
        if m:
            playlists.append({
                "num": m.group(1),
                "duration": m.group(2),
                "seconds": duration_to_seconds(m.group(2)),
            })
    return playlists
```

Replace the existing `probe_streams` function:

```python
def probe_streams(device: str, playlist_num: str) -> dict | None:
    try:
        result = subprocess.run(
            ["ffprobe", "-playlist", playlist_num, "-i", f"bluray:{device}"],
            capture_output=True, text=True, timeout=60,
        )
    except subprocess.TimeoutExpired:
        return None
    if result.returncode != 0:
        return None

    output = result.stdout + result.stderr

    audio_streams = []
    sub_count = 0
    for line in output.splitlines():
        if "Stream" in line and "Audio" in line:
            audio_streams.append(line)
        if "Stream" in line and "Subtitle" in line:
            sub_count += 1

    return {"audio_streams": audio_streams, "sub_count": sub_count}
```

- [ ] **Step 4: Add KeyboardInterrupt handler to `__main__` block**

Replace the `if __name__` block:

```python
if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nAborted.")
        sys.exit(130)
```

- [ ] **Step 5: Add `import argparse` to the imports block**

Add after `import sys` (note: `import shutil` was already added in Task 6):

```python
import argparse
```

- [ ] **Step 6: Verify script shows help and dry-run works syntactically**

Run: `python ripblu.py --help`
Expected: Shows help with all new flags

- [ ] **Step 7: Commit**

`feat: rewrite main flow with sequential episodes, filtering, validation, and dry-run`

### Task 9: Final cleanup and manual test

**Files:**
- Modify: `ripblu.py` (remove dead imports)

- [ ] **Step 1: Run full test suite**

Run: `python -m pytest tests/test_ripblu.py -v`
Expected: All PASS

- [ ] **Step 2: Remove unused imports**

Check if `import sys` is still needed (yes, for `sys.exit`). Remove any other dead imports. The `os` import can likely be removed — check if `os.environ.get` (used in `get_tmdb_api_key`) and `os.getcwd` are still referenced. `os.environ.get` is still used. `os.getcwd` should no longer be used (argparse defaults to `"."`). Remove `os.getcwd` usage if present.

- [ ] **Step 3: Verify no references to `match_episodes` remain**

Run: `grep -n match_episodes ripblu.py`
Expected: No output

- [ ] **Step 4: Run tests one final time**

Run: `python -m pytest tests/test_ripblu.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

`chore: remove dead code and unused imports`
