#!/usr/bin/env python3
"""Blu-ray to MKV ripper using ffmpeg + libaacs with TMDb episode matching."""

import json
import os
import re
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
    result = subprocess.run(
        ["ffprobe", "-i", f"bluray:{device}"],
        capture_output=True, text=True, timeout=60,
    )
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


def probe_streams(device: str, playlist_num: str) -> dict:
    result = subprocess.run(
        ["ffprobe", "-playlist", playlist_num, "-i", f"bluray:{device}"],
        capture_output=True, text=True, timeout=60,
    )
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


def match_episodes(playlists: list[dict], episodes: list[dict], threshold: int = 120) -> dict[str, dict]:
    """Match playlists to episodes by duration. Returns {playlist_num: episode}."""
    matches = {}
    unmatched_episodes = list(episodes)

    for pl in playlists:
        best_match = None
        best_diff = threshold + 1
        for ep in unmatched_episodes:
            ep_seconds = (ep.get("runtime") or 0) * 60
            diff = abs(pl["seconds"] - ep_seconds)
            if diff < best_diff:
                best_diff = diff
                best_match = ep
        if best_match and best_diff <= threshold:
            matches[pl["num"]] = best_match
            unmatched_episodes.remove(best_match)

    return matches


def prompt_tmdb(api_key: str) -> tuple[list[dict] | None, int | None, int | None]:
    """Interactive TMDb search. Returns (episodes, show_id, season_num) or (None, None, None)."""
    query = input("\nSearch TMDb for episode info (show name, or Enter to skip): ").strip()
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

    pick = input("  Select show (1-5, or Enter to skip): ").strip()
    if not pick.isdigit() or not (1 <= int(pick) <= len(results[:5])):
        return None, None, None

    show = results[int(pick) - 1]
    show_id = show["id"]

    season_str = input(f"  Season number: ").strip()
    if not season_str.isdigit():
        return None, None, None

    season_num = int(season_str)
    try:
        episodes = tmdb_get_season(show_id, season_num, api_key)
    except urllib.error.URLError as e:
        print(f"  Failed to fetch season: {e}")
        return None, None, None

    return episodes, show_id, season_num


def main():
    device = DEFAULT_DEVICE
    outdir = os.getcwd()

    args = sys.argv[1:]
    if args and args[0] in ("-h", "--help"):
        print("Usage: ripblu [device] [output_dir]")
        print(f"  device defaults to {DEFAULT_DEVICE}")
        print("  output_dir defaults to current directory")
        sys.exit(0)
    if args:
        device = args[0]
    if len(args) > 1:
        outdir = args[1]

    if not Path(device).is_block_device():
        print(f"Error: No Blu-ray device found at {device}")
        sys.exit(1)

    print(f"Scanning disc at {device}...")
    playlists = scan_playlists(device)

    if not playlists:
        print("Error: No playlists found. Check libaacs and KEYDB.cfg.")
        sys.exit(1)

    # Try TMDb matching
    episode_matches = {}
    api_key = get_tmdb_api_key()
    season_num = None

    if api_key is None:
        setup = input("\nTMDb API key not found. Enter key (or Enter to skip): ").strip()
        if setup:
            save_tmdb_api_key(setup)
            api_key = setup
            print(f"  Saved to {TMDB_API_KEY_FILE}")

    if api_key:
        episodes, show_id, season_num = prompt_tmdb(api_key)
        if episodes:
            episode_matches = match_episodes(playlists, episodes)

    # Display playlists
    print(f"\nFound {len(playlists)} playlists:\n")
    header_match = "  Episode" if episode_matches else ""
    print(f"  {'#':<4}  {'Playlist':<10}  {'Duration':<10}{header_match}")
    print(f"  {'---':<4}  {'--------':<10}  {'--------':<10}{'-' * len(header_match)}")

    for i, pl in enumerate(playlists):
        ep = episode_matches.get(pl["num"])
        ep_str = ""
        if ep:
            ep_str = f"  S{season_num:02d}E{ep['episode_number']:02d} - {ep['name']}"
        elif episode_matches:
            ep_str = "  (unmatched)"
        print(f"  {i+1:<4}  {pl['num']:<10}  {pl['duration']:<10}{ep_str}")

    print()

    # Select playlists
    selection = input("Select playlists to rip (e.g. 1,2,3 or 1-3 or 'all'): ").strip()
    if not selection:
        print("No playlists selected.")
        sys.exit(0)

    selected = []
    if selection == "all":
        selected = list(range(len(playlists)))
    else:
        for part in selection.split(","):
            part = part.strip()
            if "-" in part:
                start, end = part.split("-", 1)
                selected.extend(range(int(start) - 1, int(end)))
            else:
                selected.append(int(part) - 1)

    if not selected:
        print("No playlists selected.")
        sys.exit(0)

    # Name each playlist
    print()
    outfiles = []
    for idx in selected:
        pl = playlists[idx]
        ep = episode_matches.get(pl["num"])

        if ep and season_num is not None:
            default_name = f"S{season_num:02d}E{ep['episode_number']:02d}_{sanitize_filename(ep['name'])}"
        else:
            default_name = f"playlist{pl['num']}"

        name = input(f"  Name for playlist {pl['num']} ({pl['duration']}) [{default_name}]: ").strip()
        name = name or default_name
        name = sanitize_filename(name)
        outfiles.append(Path(outdir) / f"{name}.mkv")

    Path(outdir).mkdir(parents=True, exist_ok=True)

    # Rip
    for i, idx in enumerate(selected):
        pl = playlists[idx]
        outfile = outfiles[i]

        print(f"\nRipping playlist {pl['num']} ({pl['duration']}) -> {outfile.name}")

        streams = probe_streams(device, pl["num"])
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
    main()
