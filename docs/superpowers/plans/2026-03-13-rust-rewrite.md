# bluback Rust Rewrite Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite ripblu as "bluback" in Rust — a Blu-ray backup tool with a ratatui TUI as the default interface and a plain-text CLI fallback.

**Architecture:** Single binary Rust crate. Core logic in `disc.rs`, `tmdb.rs`, `rip.rs`, `util.rs` with shared types in `types.rs`. Two UI frontends: `tui/` (ratatui wizard + dashboard) and `cli.rs` (plain text). Mode selected by TTY detection + `--no-tui` flag.

**Tech Stack:** Rust, clap (derive), ratatui + crossterm, ureq, serde + serde_json, regex, anyhow, which

**Spec:** `docs/superpowers/specs/2026-03-13-rust-rewrite-design.md`

---

## Chunk 1: Foundation — Scaffold, Types, Pure Functions

### Task 1: Initialize Rust project

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

- [ ] **Step 1: Create Cargo project**

Run: `cargo init --name bluback`

- [ ] **Step 2: Edit `Cargo.toml` with dependencies**

```toml
[package]
name = "bluback"
version = "0.1.0"
edition = "2021"
description = "Blu-ray backup tool with TUI"

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
crossterm = "0.28"
ratatui = "0.29"
regex = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ureq = { version = "3", features = ["json"] }
which = "7"
```

- [ ] **Step 3: Set up `src/main.rs` with module declarations and clap**

```rust
mod cli;
mod disc;
mod rip;
mod tmdb;
mod tui;
mod types;
mod util;

use clap::Parser;
use std::path::PathBuf;

const DEFAULT_DEVICE: &str = "/dev/sr0";

#[derive(Parser, Debug)]
#[command(name = "bluback", version, about = "Back up Blu-ray discs to MKV files using ffmpeg + libaacs")]
pub struct Args {
    /// Blu-ray device path
    #[arg(short, long, default_value = DEFAULT_DEVICE)]
    device: PathBuf,

    /// Output directory
    #[arg(short, long, default_value = ".")]
    output: PathBuf,

    /// Season number
    #[arg(short, long)]
    season: Option<u32>,

    /// Starting episode number
    #[arg(short = 'e', long)]
    start_episode: Option<u32>,

    /// Minimum seconds to consider a playlist an episode
    #[arg(long, default_value = "900")]
    min_duration: u32,

    /// Show what would be ripped without ripping
    #[arg(long)]
    dry_run: bool,

    /// Plain text mode (auto if not a TTY)
    #[arg(long)]
    no_tui: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Check dependencies
    disc::check_dependencies()?;

    // Determine UI mode
    let use_tui = !args.no_tui && atty_stdout();

    if use_tui {
        todo!("TUI mode")
    } else {
        cli::run(&args)
    }
}

fn atty_stdout() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}
```

- [ ] **Step 4: Create stub modules so it compiles**

Create `src/cli.rs`:
```rust
use crate::Args;

pub fn run(_args: &Args) -> anyhow::Result<()> {
    todo!()
}
```

Create `src/disc.rs`:
```rust
pub fn check_dependencies() -> anyhow::Result<()> {
    todo!()
}
```

Create `src/rip.rs`:
```rust
// Rip module - ffmpeg invocation and progress parsing
```

Create `src/tmdb.rs`:
```rust
// TMDb API client
```

Create `src/tui/mod.rs`:
```rust
// TUI module
```

Create `src/types.rs`:
```rust
// Shared types
```

Create `src/util.rs`:
```rust
// Utility functions
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: Compiles with warnings about unused/dead code (expected at this stage)

- [ ] **Step 6: Commit**

`feat: initialize Rust project with clap and module structure`

---

### Task 2: Define shared types

**Files:**
- Modify: `src/types.rs`

- [ ] **Step 1: Define all types**

```rust
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Playlist {
    pub num: String,
    pub duration: String,
    pub seconds: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Episode {
    pub episode_number: u32,
    pub name: String,
    pub runtime: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TmdbShow {
    pub id: u64,
    pub name: String,
    pub first_air_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LabelInfo {
    pub show: String,
    pub season: u32,
    pub disc: u32,
}

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub audio_streams: Vec<String>,
    pub sub_count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct RipProgress {
    pub frame: u64,
    pub fps: f64,
    pub total_size: u64,
    pub out_time_secs: u32,
    pub bitrate: String,
    pub speed: f64,
}

#[derive(Debug, Clone)]
pub enum PlaylistStatus {
    Pending,
    Ripping(RipProgress),
    Done(u64),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct RipJob {
    pub playlist: Playlist,
    pub episode: Option<Episode>,
    pub filename: String,
    pub status: PlaylistStatus,
}

/// Result of assigning episodes to playlists sequentially.
pub type EpisodeAssignments = HashMap<String, Episode>;
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`

- [ ] **Step 3: Commit**

`feat: define shared types for playlists, episodes, and rip jobs`

---

### Task 3: Implement utility functions with tests

**Files:**
- Modify: `src/util.rs`

- [ ] **Step 1: Write tests first**

```rust
use crate::types::{Episode, Playlist};
use std::collections::HashMap;

pub fn duration_to_seconds(dur: &str) -> u32 {
    todo!()
}

pub fn sanitize_filename(name: &str) -> String {
    todo!()
}

pub fn parse_selection(text: &str, max_val: usize) -> Option<Vec<usize>> {
    todo!()
}

pub fn guess_start_episode(disc_number: Option<u32>, episodes_on_disc: usize) -> u32 {
    todo!()
}

pub fn assign_episodes(
    playlists: &[Playlist],
    episodes: &[Episode],
    start_episode: u32,
) -> HashMap<String, Episode> {
    todo!()
}

pub fn format_size(bytes: u64) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    // duration_to_seconds
    #[test]
    fn test_duration_hms() {
        assert_eq!(duration_to_seconds("1:23:45"), 5025);
    }

    #[test]
    fn test_duration_ms() {
        assert_eq!(duration_to_seconds("23:45"), 1425);
    }

    #[test]
    fn test_duration_zeros() {
        assert_eq!(duration_to_seconds("0:00:00"), 0);
    }

    #[test]
    fn test_duration_invalid() {
        assert_eq!(duration_to_seconds(""), 0);
    }

    // sanitize_filename
    #[test]
    fn test_sanitize_spaces() {
        assert_eq!(sanitize_filename("Hello World"), "Hello_World");
    }

    #[test]
    fn test_sanitize_special_chars() {
        assert_eq!(sanitize_filename(r#"foo/bar:baz"qux"#), "foobarbazqux");
    }

    #[test]
    fn test_sanitize_preserves_parens() {
        assert_eq!(sanitize_filename("Earth (Part 1)"), "Earth_(Part_1)");
    }

    // parse_selection
    #[test]
    fn test_selection_single() {
        assert_eq!(parse_selection("2", 5), Some(vec![1]));
    }

    #[test]
    fn test_selection_comma() {
        assert_eq!(parse_selection("1,3,5", 5), Some(vec![0, 2, 4]));
    }

    #[test]
    fn test_selection_range() {
        assert_eq!(parse_selection("2-4", 5), Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_selection_mixed() {
        assert_eq!(parse_selection("1,3-5", 5), Some(vec![0, 2, 3, 4]));
    }

    #[test]
    fn test_selection_all() {
        assert_eq!(parse_selection("all", 3), Some(vec![0, 1, 2]));
    }

    #[test]
    fn test_selection_out_of_bounds() {
        assert_eq!(parse_selection("6", 5), None);
    }

    #[test]
    fn test_selection_zero() {
        assert_eq!(parse_selection("0", 5), None);
    }

    #[test]
    fn test_selection_invalid() {
        assert_eq!(parse_selection("abc", 5), None);
    }

    #[test]
    fn test_selection_empty() {
        assert_eq!(parse_selection("", 5), None);
    }

    #[test]
    fn test_selection_reversed_range() {
        assert_eq!(parse_selection("4-2", 5), None);
    }

    #[test]
    fn test_selection_open_ended() {
        assert_eq!(parse_selection("3-", 5), Some(vec![2, 3, 4]));
    }

    // guess_start_episode
    #[test]
    fn test_guess_disc_1() {
        assert_eq!(guess_start_episode(Some(1), 5), 1);
    }

    #[test]
    fn test_guess_disc_2() {
        assert_eq!(guess_start_episode(Some(2), 5), 6);
    }

    #[test]
    fn test_guess_no_disc() {
        assert_eq!(guess_start_episode(None, 5), 1);
    }

    #[test]
    fn test_guess_zero_episodes() {
        assert_eq!(guess_start_episode(Some(2), 0), 1);
    }

    // assign_episodes
    #[test]
    fn test_assign_basic() {
        let playlists = vec![
            Playlist { num: "00001".into(), duration: "0:43:00".into(), seconds: 2580 },
            Playlist { num: "00002".into(), duration: "0:44:00".into(), seconds: 2640 },
        ];
        let episodes = vec![
            Episode { episode_number: 1, name: "Pilot".into(), runtime: Some(44) },
            Episode { episode_number: 2, name: "Second".into(), runtime: Some(44) },
        ];
        let result = assign_episodes(&playlists, &episodes, 1);
        assert_eq!(result["00001"].name, "Pilot");
        assert_eq!(result["00002"].name, "Second");
    }

    #[test]
    fn test_assign_offset() {
        let playlists = vec![
            Playlist { num: "00003".into(), duration: "0:43:00".into(), seconds: 2580 },
        ];
        let episodes = vec![
            Episode { episode_number: 1, name: "Pilot".into(), runtime: Some(44) },
            Episode { episode_number: 2, name: "Second".into(), runtime: Some(44) },
            Episode { episode_number: 3, name: "Third".into(), runtime: Some(44) },
        ];
        let result = assign_episodes(&playlists, &episodes, 3);
        assert_eq!(result["00003"].name, "Third");
    }

    #[test]
    fn test_assign_overflow() {
        let playlists = vec![
            Playlist { num: "00001".into(), duration: "0:43:00".into(), seconds: 2580 },
            Playlist { num: "00002".into(), duration: "0:44:00".into(), seconds: 2640 },
        ];
        let episodes = vec![
            Episode { episode_number: 1, name: "Pilot".into(), runtime: Some(44) },
        ];
        let result = assign_episodes(&playlists, &episodes, 1);
        assert_eq!(result["00001"].name, "Pilot");
        assert!(!result.contains_key("00002"));
    }

    #[test]
    fn test_assign_empty() {
        let playlists = vec![
            Playlist { num: "00001".into(), duration: "0:43:00".into(), seconds: 2580 },
        ];
        let result = assign_episodes(&playlists, &[], 1);
        assert!(result.is_empty());
    }

    // format_size
    #[test]
    fn test_format_bytes() {
        assert_eq!(format_size(500), "500.0 B");
    }

    #[test]
    fn test_format_kib() {
        assert_eq!(format_size(2048), "2.0 KiB");
    }

    #[test]
    fn test_format_mib() {
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn test_format_gib() {
        assert_eq!(format_size(3 * 1024u64.pow(3)), "3.0 GiB");
    }

    #[test]
    fn test_format_tib() {
        assert_eq!(format_size(2 * 1024u64.pow(4)), "2.0 TiB");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- util`
Expected: All fail with "not yet implemented"

- [ ] **Step 3: Implement all utility functions**

```rust
use crate::types::{Episode, Playlist};
use std::collections::HashMap;

pub fn duration_to_seconds(dur: &str) -> u32 {
    let parts: Vec<&str> = dur.split(':').collect();
    match parts.len() {
        3 => {
            let h: u32 = parts[0].parse().unwrap_or(0);
            let m: u32 = parts[1].parse().unwrap_or(0);
            let s: u32 = parts[2].parse().unwrap_or(0);
            h * 3600 + m * 60 + s
        }
        2 => {
            let m: u32 = parts[0].parse().unwrap_or(0);
            let s: u32 = parts[1].parse().unwrap_or(0);
            m * 60 + s
        }
        _ => 0,
    }
}

pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !r#"/<>:"|?*"#.contains(*c))
        .collect();
    cleaned.replace(' ', "_")
}

pub fn parse_selection(text: &str, max_val: usize) -> Option<Vec<usize>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text == "all" {
        return Some((0..max_val).collect());
    }

    let mut indices = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let mut split = part.splitn(2, '-');
            let start_s = split.next()?;
            let end_s = split.next()?;
            let start: usize = start_s.parse().ok()?;
            let end: usize = if end_s.is_empty() {
                max_val
            } else {
                end_s.parse().ok()?
            };
            if start > end || start < 1 || end > max_val {
                return None;
            }
            indices.extend(start - 1..end);
        } else {
            let val: usize = part.parse().ok()?;
            if val < 1 || val > max_val {
                return None;
            }
            indices.push(val - 1);
        }
    }

    if indices.is_empty() { None } else { Some(indices) }
}

pub fn guess_start_episode(disc_number: Option<u32>, episodes_on_disc: usize) -> u32 {
    match disc_number {
        Some(d) if d >= 1 && episodes_on_disc >= 1 => {
            1 + (episodes_on_disc as u32) * (d - 1)
        }
        _ => 1,
    }
}

pub fn assign_episodes(
    playlists: &[Playlist],
    episodes: &[Episode],
    start_episode: u32,
) -> HashMap<String, Episode> {
    let ep_by_num: HashMap<u32, &Episode> = episodes
        .iter()
        .map(|ep| (ep.episode_number, ep))
        .collect();

    let mut assignments = HashMap::new();
    for (i, pl) in playlists.iter().enumerate() {
        let ep_num = start_episode + i as u32;
        if let Some(ep) = ep_by_num.get(&ep_num) {
            assignments.insert(pl.num.clone(), (*ep).clone());
        }
    }
    assignments
}

pub fn format_size(bytes: u64) -> String {
    let mut size = bytes as f64;
    for unit in &["B", "KiB", "MiB", "GiB"] {
        if size.abs() < 1024.0 {
            return format!("{:.1} {}", size, unit);
        }
        size /= 1024.0;
    }
    format!("{:.1} TiB", size)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -- util`
Expected: All PASS

- [ ] **Step 5: Commit**

`feat: implement utility functions with tests`

---

### Task 4: Implement disc module with parse_volume_label tests

**Files:**
- Modify: `src/disc.rs`

- [ ] **Step 1: Write tests and stubs for `parse_volume_label` and `filter_episodes`**

```rust
use anyhow::{bail, Result};
use regex::Regex;
use std::process::Command;
use std::time::Duration;

use crate::types::{LabelInfo, Playlist, StreamInfo};
use crate::util::duration_to_seconds;

pub fn check_dependencies() -> Result<()> {
    let mut missing = Vec::new();
    for cmd in &["ffmpeg", "ffprobe"] {
        if which::which(cmd).is_err() {
            missing.push(*cmd);
        }
    }
    if !missing.is_empty() {
        bail!(
            "Required commands not found: {}. Install ffmpeg with libbluray support.",
            missing.join(", ")
        );
    }
    Ok(())
}

pub fn get_volume_label(device: &str) -> String {
    Command::new("lsblk")
        .args(["-no", "LABEL", device])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

pub fn parse_volume_label(label: &str) -> Option<LabelInfo> {
    if label.is_empty() {
        return None;
    }
    let patterns = [
        r"(?i)^(?P<show>.+?)_?SEASON(?P<season>\d+)_?DISC(?P<disc>\d+)",
        r"(?i)^(?P<show>.+?)_S(?P<season>\d+)_?D(?P<disc>\d+)",
    ];
    for pat in &patterns {
        let re = Regex::new(pat).unwrap();
        if let Some(caps) = re.captures(label) {
            let show = caps["show"].trim_matches('_').replace('_', " ");
            let season: u32 = caps["season"].parse().unwrap();
            let disc: u32 = caps["disc"].parse().unwrap();
            return Some(LabelInfo { show, season, disc });
        }
    }
    None
}

pub fn scan_playlists(device: &str) -> Result<Vec<Playlist>> {
    let output = Command::new("ffprobe")
        .args(["-i", &format!("bluray:{}", device)])
        .output()?;

    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);

    let re = Regex::new(r"playlist (\d+)\.mpls \((\d+:\d+:\d+)\)").unwrap();
    let mut playlists = Vec::new();
    for caps in re.captures_iter(&text) {
        let num = caps[1].to_string();
        let duration = caps[2].to_string();
        let seconds = duration_to_seconds(&duration);
        playlists.push(Playlist { num, duration, seconds });
    }
    Ok(playlists)
}

pub fn filter_episodes(playlists: &[Playlist], min_duration: u32) -> Vec<&Playlist> {
    playlists.iter().filter(|pl| pl.seconds >= min_duration).collect()
}

pub fn probe_streams(device: &str, playlist_num: &str) -> Option<StreamInfo> {
    let output = Command::new("ffprobe")
        .args(["-playlist", playlist_num, "-i", &format!("bluray:{}", device)])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);

    let mut audio_streams = Vec::new();
    let mut sub_count = 0u32;
    for line in text.lines() {
        if line.contains("Stream") && line.contains("Audio") {
            audio_streams.push(line.to_string());
        }
        if line.contains("Stream") && line.contains("Subtitle") {
            sub_count += 1;
        }
    }
    Some(StreamInfo { audio_streams, sub_count })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_label_sxdy() {
        let info = parse_volume_label("SGU_BR_S1D2").unwrap();
        assert_eq!(info.show, "SGU BR");
        assert_eq!(info.season, 1);
        assert_eq!(info.disc, 2);
    }

    #[test]
    fn test_parse_label_underscore_separated() {
        let info = parse_volume_label("SHOW_S1_D2").unwrap();
        assert_eq!(info.show, "SHOW");
        assert_eq!(info.season, 1);
        assert_eq!(info.disc, 2);
    }

    #[test]
    fn test_parse_label_long_form() {
        let info = parse_volume_label("SHOW_SEASON1_DISC2").unwrap();
        assert_eq!(info.show, "SHOW");
        assert_eq!(info.season, 1);
        assert_eq!(info.disc, 2);
    }

    #[test]
    fn test_parse_label_no_match() {
        assert!(parse_volume_label("RANDOM_DISC").is_none());
    }

    #[test]
    fn test_parse_label_empty() {
        assert!(parse_volume_label("").is_none());
    }

    #[test]
    fn test_parse_label_show_with_underscores() {
        let info = parse_volume_label("THE_WIRE_S3D1").unwrap();
        assert_eq!(info.show, "THE WIRE");
        assert_eq!(info.season, 3);
        assert_eq!(info.disc, 1);
    }

    #[test]
    fn test_filter_episodes() {
        let playlists = vec![
            Playlist { num: "00001".into(), duration: "0:00:30".into(), seconds: 30 },
            Playlist { num: "00002".into(), duration: "0:43:00".into(), seconds: 2580 },
            Playlist { num: "00003".into(), duration: "0:44:00".into(), seconds: 2640 },
            Playlist { num: "00004".into(), duration: "0:02:00".into(), seconds: 120 },
        ];
        let result = filter_episodes(&playlists, 900);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].num, "00002");
        assert_eq!(result[1].num, "00003");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -- disc`
Expected: All PASS

- [ ] **Step 3: Commit**

`feat: implement disc scanning, volume label parsing, and stream probing`

---

### Task 5: Implement rip module with format_size and build_map_args tests

**Files:**
- Modify: `src/rip.rs`

- [ ] **Step 1: Implement rip module**

```rust
use regex::Regex;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::types::{RipProgress, StreamInfo};
use crate::util::{duration_to_seconds, format_size};

pub fn build_map_args(streams: &StreamInfo) -> Vec<String> {
    let mut args = vec!["-map".into(), "0:v:0".into()];

    let mut surround_idx: Option<usize> = None;
    let mut stereo_idx: Option<usize> = None;
    let surround_re = Regex::new(r"5\.1|7\.1|surround").unwrap();

    for (i, line) in streams.audio_streams.iter().enumerate() {
        if surround_re.is_match(line) && surround_idx.is_none() {
            surround_idx = Some(i);
        }
        if line.contains("stereo") && stereo_idx.is_none() {
            stereo_idx = Some(i);
        }
    }

    if let Some(si) = surround_idx {
        args.extend(["-map".into(), format!("0:a:{}", si)]);
        if let Some(sti) = stereo_idx {
            args.extend(["-map".into(), format!("0:a:{}", sti)]);
        }
    } else if !streams.audio_streams.is_empty() {
        args.extend(["-map".into(), "0:a:0".into()]);
    }

    if streams.sub_count > 0 {
        args.extend(["-map".into(), "0:s?".into()]);
    }

    args
}

/// Spawn ffmpeg for ripping. Returns the child process.
/// Read progress from stdout (via `-progress pipe:1`).
pub fn start_rip(
    device: &str,
    playlist_num: &str,
    map_args: &[String],
    outfile: &Path,
) -> anyhow::Result<Child> {
    let child = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-nostats", "-progress", "pipe:1"])
        .args(["-playlist", playlist_num, "-i", &format!("bluray:{}", device)])
        .args(map_args)
        .args(["-c", "copy"])
        .arg(outfile)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    Ok(child)
}

/// Parse a single key=value line from ffmpeg `-progress` output.
/// Accumulates state in `progress_state`. Returns `Some(RipProgress)` when
/// a `progress=` line is encountered (indicates end of a progress block).
pub fn parse_progress_line(
    line: &str,
    state: &mut HashMap<String, String>,
) -> Option<RipProgress> {
    let line = line.trim();
    if let Some((key, val)) = line.split_once('=') {
        state.insert(key.to_string(), val.to_string());
    }

    if !line.starts_with("progress=") {
        return None;
    }

    let frame = state.get("frame")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let fps = state.get("fps")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);

    let total_size = state.get("total_size")
        .and_then(|v| v.parse::<i64>().ok())
        .map(|v| v.max(0) as u64)
        .unwrap_or(0);

    let out_time_secs = state.get("out_time")
        .map(|v| {
            let truncated = v.split('.').next().unwrap_or("00:00:00");
            duration_to_seconds(truncated)
        })
        .unwrap_or(0);

    let bitrate = state.get("bitrate")
        .cloned()
        .unwrap_or_else(|| "0".into());

    let speed = state.get("speed")
        .and_then(|v| v.trim_end_matches('x').parse().ok())
        .unwrap_or(0.0);

    Some(RipProgress {
        frame,
        fps,
        total_size,
        out_time_secs,
        bitrate,
        speed,
    })
}

/// Estimate the final file size based on current progress.
pub fn estimate_final_size(progress: &RipProgress, total_seconds: u32) -> Option<u64> {
    if progress.out_time_secs > 0 && total_seconds > 0 {
        Some(progress.total_size / progress.out_time_secs as u64 * total_seconds as u64)
    } else {
        None
    }
}

/// Estimate remaining wall-clock time.
pub fn estimate_eta(progress: &RipProgress, total_seconds: u32) -> Option<u32> {
    if progress.speed > 0.0 && total_seconds > 0 && progress.out_time_secs < total_seconds {
        let remaining_content = total_seconds - progress.out_time_secs;
        Some((remaining_content as f64 / progress.speed) as u32)
    } else {
        None
    }
}

/// Format seconds as H:MM:SS or M:SS.
pub fn format_eta(seconds: u32) -> String {
    let hrs = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hrs > 0 {
        format!("{}:{:02}:{:02}", hrs, mins, secs)
    } else {
        format!("{}:{:02}", mins, secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_map_args_surround_and_stereo() {
        let streams = StreamInfo {
            audio_streams: vec![
                "Stream #0:1: Audio: pcm, 48000 Hz, 5.1, s16".into(),
                "Stream #0:2: Audio: pcm, 48000 Hz, stereo, s16".into(),
            ],
            sub_count: 2,
        };
        let args = build_map_args(&streams);
        assert_eq!(args, vec![
            "-map", "0:v:0",
            "-map", "0:a:0",
            "-map", "0:a:1",
            "-map", "0:s?",
        ]);
    }

    #[test]
    fn test_build_map_args_no_surround() {
        let streams = StreamInfo {
            audio_streams: vec![
                "Stream #0:1: Audio: pcm, 48000 Hz, stereo, s16".into(),
            ],
            sub_count: 0,
        };
        let args = build_map_args(&streams);
        assert_eq!(args, vec!["-map", "0:v:0", "-map", "0:a:0"]);
    }

    #[test]
    fn test_build_map_args_no_audio() {
        let streams = StreamInfo {
            audio_streams: vec![],
            sub_count: 1,
        };
        let args = build_map_args(&streams);
        assert_eq!(args, vec!["-map", "0:v:0", "-map", "0:s?"]);
    }

    #[test]
    fn test_parse_progress_line_accumulates() {
        let mut state = HashMap::new();
        assert!(parse_progress_line("frame=100", &mut state).is_none());
        assert!(parse_progress_line("fps=24.0", &mut state).is_none());
        assert!(parse_progress_line("total_size=1048576", &mut state).is_none());
        assert!(parse_progress_line("out_time=00:01:30.000000", &mut state).is_none());
        assert!(parse_progress_line("bitrate=1234.5kbits/s", &mut state).is_none());
        assert!(parse_progress_line("speed=2.5x", &mut state).is_none());

        let progress = parse_progress_line("progress=continue", &mut state).unwrap();
        assert_eq!(progress.frame, 100);
        assert!((progress.fps - 24.0).abs() < 0.01);
        assert_eq!(progress.total_size, 1048576);
        assert_eq!(progress.out_time_secs, 90);
        assert_eq!(progress.bitrate, "1234.5kbits/s");
        assert!((progress.speed - 2.5).abs() < 0.01);
    }

    #[test]
    fn test_parse_progress_negative_size() {
        let mut state = HashMap::new();
        parse_progress_line("total_size=-1", &mut state);
        let progress = parse_progress_line("progress=continue", &mut state).unwrap();
        assert_eq!(progress.total_size, 0);
    }

    #[test]
    fn test_estimate_final_size() {
        let progress = RipProgress {
            total_size: 1_000_000,
            out_time_secs: 100,
            ..Default::default()
        };
        assert_eq!(estimate_final_size(&progress, 2600), Some(26_000_000));
    }

    #[test]
    fn test_estimate_eta() {
        let progress = RipProgress {
            out_time_secs: 1000,
            speed: 2.0,
            ..Default::default()
        };
        // 2600 total - 1000 done = 1600 remaining content / 2.0x = 800 seconds
        assert_eq!(estimate_eta(&progress, 2600), Some(800));
    }

    #[test]
    fn test_format_eta_with_hours() {
        assert_eq!(format_eta(3661), "1:01:01");
    }

    #[test]
    fn test_format_eta_minutes_only() {
        assert_eq!(format_eta(125), "2:05");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -- rip`
Expected: All PASS

- [ ] **Step 3: Commit**

`feat: implement rip module with ffmpeg progress parsing and stream mapping`

---

### Task 6: Implement TMDb client

**Files:**
- Modify: `src/tmdb.rs`

- [ ] **Step 1: Implement TMDb module**

```rust
use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::types::{Episode, TmdbShow};

fn config_path() -> PathBuf {
    dirs_or_home().join(".config").join("bluback").join("tmdb_api_key")
}

fn dirs_or_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

pub fn get_api_key() -> Option<String> {
    let path = config_path();
    if path.exists() {
        fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
    } else {
        std::env::var("TMDB_API_KEY").ok()
    }
}

pub fn save_api_key(key: &str) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, format!("{}\n", key))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn tmdb_get(path: &str, api_key: &str, extra_params: &[(&str, &str)]) -> Result<serde_json::Value> {
    let mut url = format!("https://api.themoviedb.org/3{}?api_key={}", path, api_key);
    for (k, v) in extra_params {
        url.push('&');
        url.push_str(k);
        url.push('=');
        url.push_str(&urlencoding(v));
    }

    let response: serde_json::Value = ureq::get(&url)
        .header("Accept", "application/json")
        .call()
        .context("TMDb request failed")?
        .into_json()
        .context("Failed to parse TMDb response")?;

    Ok(response)
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                String::from(b as char)
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

pub fn search_show(query: &str, api_key: &str) -> Result<Vec<TmdbShow>> {
    let data = tmdb_get("/search/tv", api_key, &[("query", query)])?;
    let results: Vec<TmdbShow> = serde_json::from_value(
        data.get("results").cloned().unwrap_or_default()
    )?;
    Ok(results)
}

pub fn get_season(show_id: u64, season: u32, api_key: &str) -> Result<Vec<Episode>> {
    let path = format!("/tv/{}/season/{}", show_id, season);
    let data = tmdb_get(&path, api_key, &[])?;
    let episodes: Vec<Episode> = serde_json::from_value(
        data.get("episodes").cloned().unwrap_or_default()
    )?;
    Ok(episodes)
}
```

Note: No unit tests for TMDb — it requires network access. Tested manually.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`

- [ ] **Step 3: Run all tests to make sure nothing broke**

Run: `cargo test`
Expected: All existing tests PASS

- [ ] **Step 4: Commit**

`feat: implement TMDb API client with search and season lookup`

---

## Chunk 2: CLI Mode — Plain Text Interactive

### Task 7: Implement CLI plain-text mode

**Files:**
- Modify: `src/cli.rs`

This is the full plain-text interactive mode, equivalent to the Python script. It uses all the core modules.

- [ ] **Step 1: Implement `cli::run`**

```rust
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use crate::disc;
use crate::rip;
use crate::tmdb;
use crate::types::*;
use crate::util::*;
use crate::Args;

pub fn run(args: &Args) -> anyhow::Result<()> {
    let device = args.device.to_string_lossy();

    // Check device
    if !args.device.exists() {
        anyhow::bail!("No Blu-ray device found at {}", device);
    }

    // Volume label
    let label = disc::get_volume_label(&device);
    let label_info = disc::parse_volume_label(&label);
    if !label.is_empty() {
        println!("Volume label: {}", label);
    }

    // Scan
    println!("Scanning disc at {}...", device);
    let playlists = disc::scan_playlists(&device)?;
    if playlists.is_empty() {
        anyhow::bail!("No playlists found. Check libaacs and KEYDB.cfg.");
    }

    let episodes_pl: Vec<&Playlist> = disc::filter_episodes(&playlists, args.min_duration);
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

    // TMDb lookup
    let mut episode_assignments: EpisodeAssignments = HashMap::new();
    let mut season_num: Option<u32> = args.season.or(label_info.as_ref().map(|l| l.season));
    let mut api_key = tmdb::get_api_key();

    if api_key.is_none() {
        let input = prompt("TMDb API key not found. Enter key (or Enter to skip): ")?;
        if !input.is_empty() {
            tmdb::save_api_key(&input)?;
            println!("  Saved API key.");
            api_key = Some(input);
        }
    }

    if api_key.is_none() && (args.season.is_some() || args.start_episode.is_some()) {
        println!("Warning: --season/--start-episode require TMDb. Ignoring.");
    }

    if let Some(ref key) = api_key {
        let default_query = label_info.as_ref().map(|l| l.show.as_str()).unwrap_or("");
        let cli_season = args.season.or(label_info.as_ref().map(|l| l.season));

        if let Some((episodes, _show_id, sn)) = prompt_tmdb(key, default_query, cli_season)? {
            season_num = Some(sn);

            // Determine starting episode
            let disc_number = label_info.as_ref().map(|l| l.disc);
            let default_start = args.start_episode.unwrap_or_else(|| {
                guess_start_episode(disc_number, episodes_pl.len())
            });

            let start_ep = if args.start_episode.is_none() {
                prompt_number(&format!("  Starting episode number [{}]: ", default_start), Some(default_start))?
            } else {
                default_start
            };

            // Build owned playlists for assign_episodes
            let pl_owned: Vec<Playlist> = episodes_pl.iter().map(|p| (*p).clone()).collect();
            episode_assignments = assign_episodes(&pl_owned, &episodes, start_ep);
        }
    }

    // Display playlists
    let has_eps = !episode_assignments.is_empty();
    let header_ep = if has_eps { "  Episode" } else { "" };
    println!("\n  {:<4}  {:<10}  {:<10}{}", "#", "Playlist", "Duration", header_ep);
    println!("  {:<4}  {:<10}  {:<10}{}", "---", "--------", "--------", "-".repeat(header_ep.len()));

    for (i, pl) in episodes_pl.iter().enumerate() {
        let ep_str = if let Some(ep) = episode_assignments.get(&pl.num) {
            format!("  S{:02}E{:02} - {}", season_num.unwrap_or(0), ep.episode_number, ep.name)
        } else if has_eps {
            "  (no episode data)".into()
        } else {
            String::new()
        };
        println!("  {:<4}  {:<10}  {:<10}{}", i + 1, pl.num, pl.duration, ep_str);
    }
    println!();

    // Select playlists
    let selected = loop {
        let input = prompt("Select playlists to rip (e.g. 1,2,3 or 1-3 or 'all') [all]: ")?;
        let input = if input.is_empty() { "all".to_string() } else { input };
        if let Some(sel) = parse_selection(&input, episodes_pl.len()) {
            break sel;
        }
        println!("Invalid selection. Try again.");
    };

    // Generate default names
    println!();
    let mut default_names: Vec<String> = Vec::new();
    for &idx in &selected {
        let pl = episodes_pl[idx];
        let name = if let Some(ep) = episode_assignments.get(&pl.num) {
            format!("S{:02}E{:02}_{}", season_num.unwrap_or(0), ep.episode_number, sanitize_filename(&ep.name))
        } else {
            format!("playlist{}", pl.num)
        };
        default_names.push(name);
    }

    // Show filenames and ask to customize
    println!("  Output filenames:");
    for (i, &idx) in selected.iter().enumerate() {
        let pl = episodes_pl[idx];
        println!("    {} ({}) -> {}.mkv", pl.num, pl.duration, default_names[i]);
    }

    let customize = prompt("\n  Customize filenames? [y/N]: ")?;
    let mut outfiles: Vec<PathBuf> = Vec::new();
    if customize.eq_ignore_ascii_case("y") || customize.eq_ignore_ascii_case("yes") {
        for (i, &idx) in selected.iter().enumerate() {
            let pl = episodes_pl[idx];
            let input = prompt(&format!("  Name for playlist {} [{}]: ", pl.num, default_names[i]))?;
            let name = if input.is_empty() { default_names[i].clone() } else { sanitize_filename(&input) };
            outfiles.push(args.output.join(format!("{}.mkv", name)));
        }
    } else {
        for name in &default_names {
            outfiles.push(args.output.join(format!("{}.mkv", name)));
        }
    }

    // Dry run
    if args.dry_run {
        println!("\n[DRY RUN] Would rip:");
        for (i, &idx) in selected.iter().enumerate() {
            let pl = episodes_pl[idx];
            println!("  {} ({}) -> {}", pl.num, pl.duration, outfiles[i].file_name().unwrap().to_string_lossy());
        }
        return Ok(());
    }

    std::fs::create_dir_all(&args.output)?;

    // Rip
    for (i, &idx) in selected.iter().enumerate() {
        let pl = episodes_pl[idx];
        let outfile = &outfiles[i];
        let filename = outfile.file_name().unwrap().to_string_lossy();

        println!("\nRipping playlist {} ({}) -> {}", pl.num, pl.duration, filename);

        let streams = match disc::probe_streams(&device, &pl.num) {
            Some(s) => s,
            None => {
                println!("Warning: Failed to probe streams for playlist {}, skipping.", pl.num);
                continue;
            }
        };

        let map_args = rip::build_map_args(&streams);
        let mut child = rip::start_rip(&device, &pl.num, &map_args, outfile)?;

        let stdout = child.stdout.take().expect("stdout piped");
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
            println!("Error: ffmpeg exited with code {}", status.code().unwrap_or(-1));
            continue;
        }

        let final_size = std::fs::metadata(outfile)?.len();
        println!("Done: {} ({})", filename, format_size(final_size));
    }

    println!("\nAll done! Ripped {} playlist(s) to {}", selected.len(), args.output.display());
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
) -> anyhow::Result<Option<(Vec<Episode>, u64, u32)>> {
    let hint = if default_query.is_empty() {
        String::new()
    } else {
        format!(" [{}]", default_query)
    };
    let query = prompt(&format!("\nSearch TMDb for episode info{}: ", hint))?;
    let query = if query.is_empty() { default_query.to_string() } else { query };
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
        let year = show.first_air_date.as_deref().unwrap_or("").get(..4).unwrap_or("");
        println!("    {}. {} ({})", i + 1, show.name, year);
    }

    let show_idx = loop {
        let pick = prompt("  Select show (1-5, Enter for 1, 's' to skip): ")?;
        if pick.eq_ignore_ascii_case("s") {
            return Ok(None);
        }
        let pick = if pick.is_empty() { "1".to_string() } else { pick };
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
            println!("    E{:02} - {}  ({} min)", ep.episode_number, ep.name, runtime);
        }
    }

    Ok(Some((episodes, show_id, season_num)))
}
```

- [ ] **Step 2: Verify it compiles and existing tests pass**

Run: `cargo build && cargo test`

- [ ] **Step 3: Manual test with a disc**

Run: `cargo run -- --no-tui`
Expected: Same interactive flow as the Python script

- [ ] **Step 4: Commit**

`feat: implement CLI plain-text mode (feature parity with Python)`

---

## Chunk 3: TUI Mode — Wizard and Dashboard

### Task 8: TUI framework and app state

**Files:**
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Implement TUI app state and event loop**

```rust
pub mod dashboard;
pub mod wizard;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use std::collections::HashMap;
use std::io;
use std::time::Duration;

use crate::types::*;
use crate::Args;

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Scanning,
    TmdbSearch,
    ShowSelect,
    SeasonEpisode,
    PlaylistSelect,
    Confirm,
    Ripping,
    Done,
}

pub struct App {
    pub screen: Screen,
    pub args: Args,
    pub quit: bool,

    // Disc data
    pub label: String,
    pub label_info: Option<LabelInfo>,
    pub playlists: Vec<Playlist>,
    pub episodes_pl: Vec<Playlist>,

    // TMDb data
    pub api_key: Option<String>,
    pub search_query: String,
    pub search_results: Vec<TmdbShow>,
    pub selected_show: Option<usize>,
    pub season_num: Option<u32>,
    pub episodes: Vec<Episode>,
    pub start_episode: Option<u32>,
    pub episode_assignments: EpisodeAssignments,

    // Selection state
    pub playlist_selected: Vec<bool>,
    pub filenames: Vec<String>,
    pub list_cursor: usize,

    // Text input
    pub input_buffer: String,
    pub input_active: bool,

    // Rip state
    pub rip_jobs: Vec<RipJob>,
    pub current_rip: usize,

    // Status/error messages
    pub status_message: String,
}

impl App {
    pub fn new(args: Args) -> Self {
        Self {
            screen: Screen::Scanning,
            args,
            quit: false,
            label: String::new(),
            label_info: None,
            playlists: Vec::new(),
            episodes_pl: Vec::new(),
            api_key: None,
            search_query: String::new(),
            search_results: Vec::new(),
            selected_show: None,
            season_num: None,
            episodes: Vec::new(),
            start_episode: None,
            episode_assignments: HashMap::new(),
            playlist_selected: Vec::new(),
            filenames: Vec::new(),
            list_cursor: 0,
            input_buffer: String::new(),
            input_active: false,
            rip_jobs: Vec::new(),
            current_rip: 0,
            status_message: String::new(),
        }
    }
}

pub fn run(args: &Args) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, args);

    // Restore terminal
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, args: &Args) -> Result<()> {
    let mut app = App::new(args.clone());

    // Initial scan (blocking — runs before event loop)
    app.status_message = format!("Scanning disc at {}...", args.device.display());
    terminal.draw(|f| wizard::render_scanning(f, &app))?;

    // Perform scan
    let device = args.device.to_string_lossy().to_string();
    app.label = crate::disc::get_volume_label(&device);
    app.label_info = crate::disc::parse_volume_label(&app.label);
    app.playlists = crate::disc::scan_playlists(&device)?;
    app.episodes_pl = crate::disc::filter_episodes(&app.playlists, args.min_duration)
        .into_iter().cloned().collect();
    app.api_key = crate::tmdb::get_api_key();

    // Pre-fill from label/args
    if let Some(ref info) = app.label_info {
        app.search_query = info.show.clone();
        app.season_num = Some(info.season);
    }
    if let Some(s) = args.season {
        app.season_num = Some(s);
    }
    app.start_episode = args.start_episode;

    // Initialize playlist selection (all selected)
    app.playlist_selected = vec![true; app.episodes_pl.len()];

    if app.episodes_pl.is_empty() {
        app.status_message = "No episode-length playlists found.".into();
        app.screen = Screen::Done;
    } else if app.api_key.is_some() {
        app.screen = Screen::TmdbSearch;
        app.input_active = true;
        app.input_buffer = app.search_query.clone();
    } else {
        app.screen = Screen::PlaylistSelect;
    }

    // Event loop
    loop {
        terminal.draw(|f| {
            match app.screen {
                Screen::Scanning => wizard::render_scanning(f, &app),
                Screen::TmdbSearch => wizard::render_tmdb_search(f, &app),
                Screen::ShowSelect => wizard::render_show_select(f, &app),
                Screen::SeasonEpisode => wizard::render_season_episode(f, &app),
                Screen::PlaylistSelect => wizard::render_playlist_select(f, &app),
                Screen::Confirm => wizard::render_confirm(f, &app),
                Screen::Ripping => dashboard::render(f, &app),
                Screen::Done => dashboard::render_done(f, &app),
            }
        })?;

        if app.quit {
            break;
        }

        // Poll for events
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Global quit
                if key.code == KeyCode::Char('q') && !app.input_active {
                    app.quit = true;
                    continue;
                }
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.quit = true;
                    continue;
                }

                match app.screen {
                    Screen::TmdbSearch => wizard::handle_tmdb_search_input(&mut app, key),
                    Screen::ShowSelect => wizard::handle_show_select_input(&mut app, key),
                    Screen::SeasonEpisode => wizard::handle_season_episode_input(&mut app, key),
                    Screen::PlaylistSelect => wizard::handle_playlist_select_input(&mut app, key),
                    Screen::Confirm => wizard::handle_confirm_input(&mut app, key),
                    Screen::Ripping => dashboard::handle_input(&mut app, key),
                    Screen::Done => {
                        app.quit = true;
                    }
                    _ => {}
                }
            }
        }

        // If ripping, check for progress updates
        if app.screen == Screen::Ripping {
            dashboard::tick(&mut app)?;
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Create stub `wizard.rs` and `dashboard.rs`**

Create `src/tui/wizard.rs`:
```rust
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::App;

pub fn render_scanning(f: &mut Frame, app: &App) {
    // TODO: render scanning screen with spinner
    let text = ratatui::widgets::Paragraph::new(app.status_message.as_str());
    f.render_widget(text, f.area());
}

pub fn render_tmdb_search(f: &mut Frame, app: &App) { todo!() }
pub fn render_show_select(f: &mut Frame, app: &App) { todo!() }
pub fn render_season_episode(f: &mut Frame, app: &App) { todo!() }
pub fn render_playlist_select(f: &mut Frame, app: &App) { todo!() }
pub fn render_confirm(f: &mut Frame, app: &App) { todo!() }

pub fn handle_tmdb_search_input(app: &mut App, key: KeyEvent) { todo!() }
pub fn handle_show_select_input(app: &mut App, key: KeyEvent) { todo!() }
pub fn handle_season_episode_input(app: &mut App, key: KeyEvent) { todo!() }
pub fn handle_playlist_select_input(app: &mut App, key: KeyEvent) { todo!() }
pub fn handle_confirm_input(app: &mut App, key: KeyEvent) { todo!() }
```

Create `src/tui/dashboard.rs`:
```rust
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::App;

pub fn render(f: &mut Frame, app: &App) { todo!() }
pub fn render_done(f: &mut Frame, app: &App) { todo!() }
pub fn handle_input(app: &mut App, key: KeyEvent) { todo!() }
pub fn tick(app: &mut App) -> anyhow::Result<()> { todo!() }
```

- [ ] **Step 3: Add `Clone` derive to `Args` in `main.rs`**

The `App::new` takes ownership of `Args`, but `tui::run` receives `&Args`. Add `Clone` to the derive:

```rust
#[derive(Parser, Debug, Clone)]
```

- [ ] **Step 4: Update `main.rs` to call `tui::run`**

Replace the `todo!("TUI mode")` in main:
```rust
    if use_tui {
        tui::run(&args)
    } else {
        cli::run(&args)
    }
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: Compiles (TUI screens are stubs with `todo!()` — they won't crash unless reached)

- [ ] **Step 6: Commit**

`feat: add TUI framework with app state, event loop, and screen routing`

---

### Task 9: Implement TUI wizard screens

**Files:**
- Modify: `src/tui/wizard.rs`

This is the largest single task. Each wizard screen needs a render function and an input handler.

- [ ] **Step 1: Implement all wizard render and input handler functions**

The wizard screens follow a common pattern:
- **Render**: Build ratatui widgets (Paragraph, List, Table) and render to frame
- **Input handler**: Match key events, update app state, transition screens

Key implementation details:

**TMDb Search Screen (`render_tmdb_search` / `handle_tmdb_search_input`):**
- Show disc info (label, playlist count) at top
- Text input widget for search query (pre-filled from volume label)
- Enter: perform search (blocking `tmdb::search_show`), transition to ShowSelect
- Esc: skip TMDb, go to PlaylistSelect

**Show Select Screen (`render_show_select` / `handle_show_select_input`):**
- `List` widget with show names and years, highlighted cursor
- Up/Down to move cursor, Enter to select
- Esc: back to TmdbSearch

**Season & Episode Screen (`render_season_episode` / `handle_season_episode_input`):**
- Two-phase: first season input, then start episode input
- After season entered: fetch episodes (blocking), show episode list
- Pre-fill from `--season` flag (show as read-only)
- Pre-fill start episode from `guess_start_episode`

**Playlist Select Screen (`render_playlist_select` / `handle_playlist_select_input`):**
- `Table` widget with columns: checkbox, playlist num, duration, episode name, filename
- Space to toggle selection, Enter to confirm
- All selected by default
- 'e' on a row to edit that row's filename (switches to inline text input)

**Confirm Screen (`render_confirm` / `handle_confirm_input`):**
- Summary table of selected playlists and filenames
- Enter to start ripping (transition to Ripping screen, build RipJobs)
- Esc to go back

Each render function should have a consistent layout:
- Title bar at top with screen name
- Content in center
- Key hints at bottom (e.g., "Enter: Confirm | Esc: Back | q: Quit")

Use `ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Table, Row, Cell}` and `ratatui::layout::{Layout, Constraint, Direction}`.

For text input, track `app.input_buffer` and render as a `Paragraph` with a cursor indicator. Handle `KeyCode::Char(c)`, `KeyCode::Backspace`, `KeyCode::Enter`.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`

- [ ] **Step 3: Manual test**

Run: `cargo run`
Expected: TUI wizard launches, screens are navigable

- [ ] **Step 4: Commit**

`feat: implement TUI wizard screens (search, select, season, playlists, confirm)`

---

### Task 10: Implement TUI ripping dashboard

**Files:**
- Modify: `src/tui/dashboard.rs`

- [ ] **Step 1: Implement dashboard render and tick**

Key implementation details:

**`render` function:**
- Title bar: "Ripping: [show info]" with "N/M complete"
- `Table` widget with rows per `RipJob`:
  - Pending: playlist num, episode name, "Pending"
  - Ripping: playlist num, episode name, `Gauge` progress bar, size, ETA
  - Done: playlist num, episode name, "✓ Done", final size
  - Failed: playlist num, episode name, "✗ Failed", error
- Bottom status bar: detailed ffmpeg stats (frame, fps, bitrate, speed) for active rip
- Key hints: "[q] Abort"

**`tick` function** (called every 100ms from event loop):
- If no active rip and there are pending jobs, start the next one:
  - Call `disc::probe_streams`, `rip::build_map_args`, `rip::start_rip`
  - Store the child process handle in app state
  - Update status to `Ripping`
- If there's an active rip:
  - Read available lines from ffmpeg stdout (non-blocking)
  - Parse with `rip::parse_progress_line`
  - Update the `RipProgress` in the current job's status
  - Check if child process has exited (`child.try_wait()`)
  - If exited successfully: update to `Done(file_size)`, advance `current_rip`
  - If exited with error: update to `Failed(message)`, advance `current_rip`
- If all jobs done: transition to `Screen::Done`

**Threading approach for non-blocking stdout reads:**
Use `std::sync::mpsc` channel. Spawn a thread that reads lines from ffmpeg stdout and sends them through the channel. The `tick` function calls `receiver.try_recv()` to get lines without blocking.

```rust
use std::sync::mpsc;
use std::thread;

// Store in App:
// pub rip_child: Option<std::process::Child>,
// pub progress_rx: Option<mpsc::Receiver<String>>,
// pub progress_state: HashMap<String, String>,
```

**`handle_input`:**
- `q`: prompt for confirmation (set a flag, render confirmation overlay)
- If confirmed: kill child process, transition to Done
- Esc on confirmation: cancel abort

**`render_done` function:**
- Summary: "All done! Ripped N playlist(s)"
- List of completed files with sizes
- "Press any key to exit"

- [ ] **Step 2: Add necessary fields to `App` struct in `tui/mod.rs`**

Add these fields to the `App` struct:
```rust
    pub rip_child: Option<std::process::Child>,
    pub progress_rx: Option<mpsc::Receiver<String>>,
    pub progress_state: HashMap<String, String>,
    pub confirm_abort: bool,
```

Initialize them in `App::new`:
```rust
    rip_child: None,
    progress_rx: None,
    progress_state: HashMap::new(),
    confirm_abort: false,
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`

- [ ] **Step 4: Manual test with a disc**

Run: `cargo run`
Expected: Full TUI flow — wizard → dashboard → completion

- [ ] **Step 5: Commit**

`feat: implement TUI ripping dashboard with progress tracking`

---

## Chunk 4: Integration, Cleanup, Polish

### Task 11: Delete Python files and clean up

**Files:**
- Delete: `ripblu.py`
- Delete: `tests/test_ripblu.py`
- Delete: `tests/` directory (if empty)

- [ ] **Step 1: Run full Rust test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 2: Verify CLI mode works**

Run: `cargo run -- --no-tui --dry-run`
Expected: Scans disc, shows playlists, shows dry-run summary

- [ ] **Step 3: Verify TUI mode works**

Run: `cargo run -- --dry-run`
Expected: TUI wizard launches, dry-run shows summary

- [ ] **Step 4: Delete Python files**

```bash
rm ripblu.py tests/test_ripblu.py
rmdir tests 2>/dev/null || true
rm -rf __pycache__ .pytest_cache 2>/dev/null || true
```

- [ ] **Step 5: Update .gitignore for Rust**

Add to `.gitignore`:
```
/target
```

- [ ] **Step 6: Commit**

`chore: remove Python implementation, add Rust .gitignore`

---

### Task 12: Final build and release test

- [ ] **Step 1: Build release binary**

Run: `cargo build --release`
Expected: Binary at `target/release/bluback`

- [ ] **Step 2: Check binary size**

Run: `ls -lh target/release/bluback`
Expected: Reasonable size (likely 5-15 MB)

- [ ] **Step 3: Test release binary with disc**

Run: `./target/release/bluback`
Expected: Full TUI flow works

- [ ] **Step 4: Test release binary in plain text mode**

Run: `./target/release/bluback --no-tui`
Expected: Same behavior as Python script

- [ ] **Step 5: Commit**

`feat: Rust rewrite complete — TUI + CLI modes with full feature parity`
