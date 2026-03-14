#!/usr/bin/env python3
"""Blu-ray to MKV ripper using ffmpeg + libaacs with TMDb episode matching."""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

TMDB_API_KEY_FILE = Path.home() / ".config" / "ripblu" / "tmdb_api_key"
DEFAULT_DEVICE = "/dev/sr0"


def get_tmdb_api_key() -> str | None:
    if TMDB_API_KEY_FILE.exists():
        return TMDB_API_KEY_FILE.read_text().strip()
    return os.environ.get("TMDB_API_KEY")


def save_tmdb_api_key(key: str):
    TMDB_API_KEY_FILE.parent.mkdir(parents=True, exist_ok=True)
    TMDB_API_KEY_FILE.write_text(key + "\n")
    TMDB_API_KEY_FILE.chmod(0o600)


def tmdb_request(path: str, api_key: str, params: dict | None = None) -> dict:
    params = params or {}
    params["api_key"] = api_key
    qs = urllib.parse.urlencode(params)
    url = f"https://api.themoviedb.org/3{path}?{qs}"
    req = urllib.request.Request(url)
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def tmdb_search_show(query: str, api_key: str) -> list[dict]:
    data = tmdb_request("/search/tv", api_key, {"query": query})
    return data.get("results", [])


def tmdb_get_season(show_id: int, season: int, api_key: str) -> list[dict]:
    data = tmdb_request(f"/tv/{show_id}/season/{season}", api_key)
    return data.get("episodes", [])


def duration_to_seconds(dur_str: str) -> int:
    parts = dur_str.split(":")
    if len(parts) == 3:
        return int(parts[0]) * 3600 + int(parts[1]) * 60 + int(parts[2])
    elif len(parts) == 2:
        return int(parts[0]) * 60 + int(parts[1])
    return 0


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


def filter_episodes(playlists: list[dict], min_duration: int = 900) -> list[dict]:
    """Filter playlists to only those likely to be episodes (above min_duration seconds)."""
    return [pl for pl in playlists if pl["seconds"] >= min_duration]


def guess_start_episode(disc_number: int | None, episodes_on_disc: int) -> int:
    """Guess starting episode number based on disc number and episodes per disc."""
    if not disc_number or disc_number < 1 or episodes_on_disc < 1:
        return 1
    return 1 + episodes_on_disc * (disc_number - 1)


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


def build_map_args(streams: dict) -> list[str]:
    args = ["-map", "0:v:0"]

    audio = streams["audio_streams"]
    surround_idx = None
    stereo_idx = None
    for i, line in enumerate(audio):
        if re.search(r"5\.1|7\.1|surround", line):
            surround_idx = surround_idx if surround_idx is not None else i
        if "stereo" in line:
            stereo_idx = stereo_idx if stereo_idx is not None else i

    if surround_idx is not None:
        args += ["-map", f"0:a:{surround_idx}"]
        if stereo_idx is not None:
            args += ["-map", f"0:a:{stereo_idx}"]
    elif audio:
        args += ["-map", "0:a:0"]

    if streams["sub_count"] > 0:
        args += ["-map", "0:s?"]

    return args


def sanitize_filename(name: str) -> str:
    name = re.sub(r'[/<>:"|?*]', '', name)
    name = name.replace(' ', '_')
    return name


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
    season_num = args.season if args.season is not None else label_info.get("season")
    api_key = get_tmdb_api_key()

    if api_key is None:
        setup = input("TMDb API key not found. Enter key (or Enter to skip): ").strip()
        if setup:
            save_tmdb_api_key(setup)
            api_key = setup
            print(f"  Saved to {TMDB_API_KEY_FILE}")

    if api_key is None and (args.season is not None or args.start_episode is not None):
        print("Warning: --season/--start-episode require TMDb. Ignoring.")

    if api_key:
        default_query = label_info.get("show", "")
        cli_season = args.season if args.season is not None else label_info.get("season")
        episodes, show_id, season_num = prompt_tmdb(api_key, default_query, cli_season)

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
        selection = input("Select playlists to rip (e.g. 1,2,3 or 1-3 or 'all') [all]: ").strip()
        selection = selection or "all"
        selected = parse_selection(selection, len(episodes_pl))
        if selected is not None:
            break
        print("Invalid selection. Try again.")

    # Name each playlist
    print()
    outfiles = []
    default_names = []
    for idx in selected:
        pl = episodes_pl[idx]
        ep = episode_assignments.get(pl["num"])

        if ep and season_num is not None:
            default_names.append(f"S{season_num:02d}E{ep['episode_number']:02d}_{sanitize_filename(ep['name'])}")
        else:
            default_names.append(f"playlist{pl['num']}")

    print("  Output filenames:")
    for i, idx in enumerate(selected):
        pl = episodes_pl[idx]
        print(f"    {pl['num']} ({pl['duration']}) -> {default_names[i]}.mkv")

    customize = input("\n  Customize filenames? [y/N]: ").strip().lower()
    if customize in ("y", "yes"):
        for i, idx in enumerate(selected):
            pl = episodes_pl[idx]
            name = input(f"  Name for playlist {pl['num']} [{default_names[i]}]: ").strip()
            name = sanitize_filename(name) if name else default_names[i]
            outfiles.append(Path(outdir) / f"{name}.mkv")
    else:
        for name in default_names:
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


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nAborted.")
        sys.exit(130)
