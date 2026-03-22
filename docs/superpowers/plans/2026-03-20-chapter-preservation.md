# Chapter Preservation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve Blu-ray chapter markers in ripped MKV files by parsing MPLS playlist files and injecting chapters via `mkvpropedit`.

**Architecture:** Parse MPLS files from the mounted disc using the `mpls` crate to extract chapter timestamps. After each successful ffmpeg rip, write an OGM-format chapter file and run `mkvpropedit --chapters` to stamp chapters into the MKV in-place. Gracefully degrade when `mkvpropedit` is unavailable.

**Tech Stack:** `mpls` crate (MPLS parsing), `mkvpropedit` (chapter injection), existing `which` crate (tool detection)

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/chapters.rs` (create) | MPLS chapter extraction, OGM file generation, `mkvpropedit` invocation |
| `src/types.rs` (modify) | Add `ChapterMark` struct |
| `src/disc.rs` (modify) | Add disc mount/unmount helpers, `mkvpropedit` availability check |
| `src/cli.rs` (modify) | Call chapter injection after each successful rip |
| `src/tui/dashboard.rs` (modify) | Call chapter injection after each successful rip |
| `src/tui/mod.rs` (modify) | Store `mkvpropedit` availability and mount point in App state |
| `src/main.rs` (modify) | Register `chapters` module, pass `mkvpropedit` availability to modes |
| `Cargo.toml` (modify) | Add `mpls` dependency |

---

### Task 1: Add `mpls` dependency and `ChapterMark` type

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/types.rs`

- [ ] **Step 1: Add `mpls` dependency to Cargo.toml**

Add after the `toml` line in `[dependencies]`:
```toml
mpls = "0.2"
```

- [ ] **Step 2: Add `ChapterMark` struct to `types.rs`**

Add after the `StreamInfo` struct (line 42):
```rust
#[derive(Debug, Clone)]
pub struct ChapterMark {
    pub index: u32,
    pub start_secs: f64,
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

Suggested message: `add mpls dependency and ChapterMark type`

---

### Task 2: Create `chapters.rs` — MPLS parsing

**Files:**
- Create: `src/chapters.rs`
- Modify: `src/main.rs` (add `mod chapters;`)

- [ ] **Step 1: Write tests for chapter extraction from MPLS bytes**

The `mpls` crate's `Mpls::from()` takes any `Read` impl, so we can test with in-memory bytes. However, MPLS is a complex binary format — constructing test bytes is impractical. Instead, test the timestamp conversion and OGM formatting functions which are pure logic.

In `src/chapters.rs`:
```rust
use crate::types::ChapterMark;
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// Format a chapter timestamp as HH:MM:SS.mmm for OGM chapter format.
fn format_chapter_time(secs: f64) -> String {
    let total_ms = (secs * 1000.0).round() as u64;
    let hrs = total_ms / 3_600_000;
    let mins = (total_ms % 3_600_000) / 60_000;
    let s = (total_ms % 60_000) / 1000;
    let ms = total_ms % 1000;
    format!("{:02}:{:02}:{:02}.{:03}", hrs, mins, s, ms)
}

/// Generate OGM-format chapter text from a list of chapter marks.
pub fn chapters_to_ogm(chapters: &[ChapterMark]) -> String {
    let mut out = String::new();
    for ch in chapters {
        out.push_str(&format!(
            "CHAPTER{:02}={}\nCHAPTER{:02}NAME=Chapter {}\n",
            ch.index,
            format_chapter_time(ch.start_secs),
            ch.index,
            ch.index,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_chapter_time_zero() {
        assert_eq!(format_chapter_time(0.0), "00:00:00.000");
    }

    #[test]
    fn test_format_chapter_time_minutes() {
        assert_eq!(format_chapter_time(202.119), "00:03:22.119");
    }

    #[test]
    fn test_format_chapter_time_hours() {
        assert_eq!(format_chapter_time(3661.5), "01:01:01.500");
    }

    #[test]
    fn test_chapters_to_ogm_single() {
        let chapters = vec![ChapterMark { index: 1, start_secs: 0.0 }];
        let ogm = chapters_to_ogm(&chapters);
        assert_eq!(ogm, "CHAPTER01=00:00:00.000\nCHAPTER01NAME=Chapter 1\n");
    }

    #[test]
    fn test_chapters_to_ogm_multiple() {
        let chapters = vec![
            ChapterMark { index: 1, start_secs: 0.0 },
            ChapterMark { index: 2, start_secs: 202.119 },
            ChapterMark { index: 3, start_secs: 772.689 },
        ];
        let ogm = chapters_to_ogm(&chapters);
        assert!(ogm.contains("CHAPTER01=00:00:00.000"));
        assert!(ogm.contains("CHAPTER02=00:03:22.119"));
        assert!(ogm.contains("CHAPTER03=00:12:52.689"));
        assert!(ogm.contains("CHAPTER03NAME=Chapter 3"));
    }

    #[test]
    fn test_chapters_to_ogm_empty() {
        assert_eq!(chapters_to_ogm(&[]), "");
    }
}
```

- [ ] **Step 2: Add `mod chapters;` to `main.rs`**

Add after `mod cli;` (line 1) in `src/main.rs`:
```rust
mod chapters;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test chapters`
Expected: all 4 tests pass

- [ ] **Step 4: Commit**

Suggested message: `add chapter timestamp conversion and OGM formatting with tests`

---

### Task 3: Add MPLS parsing function to `chapters.rs`

**Files:**
- Modify: `src/chapters.rs`

- [ ] **Step 1: Add the `extract_chapters` function**

Add above the `#[cfg(test)]` block in `src/chapters.rs`:
```rust
/// Extract chapter marks from an MPLS playlist file on a mounted Blu-ray disc.
///
/// `mount_point` is the filesystem root of the mounted disc.
/// `playlist_num` is the zero-padded playlist number (e.g., "00001").
///
/// Returns chapter marks with timestamps relative to the start of the playlist,
/// or None if the MPLS file can't be read or has no entry-point marks.
pub fn extract_chapters(mount_point: &Path, playlist_num: &str) -> Option<Vec<ChapterMark>> {
    let mpls_path = Path::new(mount_point)
        .join("BDMV")
        .join("PLAYLIST")
        .join(format!("{}.mpls", playlist_num));

    let file = std::fs::File::open(&mpls_path).ok()?;
    let mpls_data = mpls::Mpls::from(file).ok()?;

    let entry_marks: Vec<_> = mpls_data
        .marks
        .iter()
        .filter(|m| m.mark_type == mpls::types::MarkType::EntryPoint)
        .collect();

    if entry_marks.is_empty() {
        return None;
    }

    let base_secs = entry_marks[0].time_stamp.seconds();

    let chapters: Vec<ChapterMark> = entry_marks
        .iter()
        .enumerate()
        .map(|(i, mark)| ChapterMark {
            index: (i + 1) as u32,
            start_secs: mark.time_stamp.seconds() - base_secs,
        })
        .collect();

    Some(chapters)
}
```

**Note:** `mpls::types::TimeStamp` wraps a `u32` (45kHz ticks) and provides a `.seconds()` method that converts to `f64`. We use `.seconds()` directly rather than reimplementing the conversion.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles. If `TimeStamp` access differs from `.0`, adjust based on compiler error.

- [ ] **Step 3: Commit**

Suggested message: `add MPLS chapter extraction using mpls crate`

---

### Task 4: Add `mkvpropedit` chapter injection to `chapters.rs`

**Files:**
- Modify: `src/chapters.rs`

- [ ] **Step 1: Add `apply_chapters` function**

Add after `extract_chapters` in `src/chapters.rs`:
```rust
/// Write chapters to an MKV file using mkvpropedit.
///
/// Writes a temporary OGM chapter file next to the output, runs mkvpropedit,
/// and cleans up the temp file. Returns Ok(true) if chapters were applied,
/// Ok(false) if there were no chapters, or Err on failure.
pub fn apply_chapters(outfile: &Path, chapters: &[ChapterMark]) -> anyhow::Result<bool> {
    if chapters.is_empty() {
        return Ok(false);
    }

    let ogm = chapters_to_ogm(chapters);

    let chapter_file = outfile.with_extension("chapters.txt");
    {
        let mut f = std::fs::File::create(&chapter_file)?;
        f.write_all(ogm.as_bytes())?;
    }

    let output = Command::new("mkvpropedit")
        .arg(outfile)
        .args(["--chapters", &chapter_file.to_string_lossy()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    let _ = std::fs::remove_file(&chapter_file);

    match output {
        Ok(o) if o.status.success() => Ok(true),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            anyhow::bail!("mkvpropedit failed: {}", stderr.trim())
        }
        Err(e) => anyhow::bail!("failed to run mkvpropedit: {}", e),
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

Suggested message: `add mkvpropedit chapter injection function`

---

### Task 5: Add disc mount/unmount helpers and `mkvpropedit` check

**Files:**
- Modify: `src/disc.rs`

- [ ] **Step 1: Add `check_mkvpropedit` function**

Add after the existing `check_dependencies` function (after line 62) in `src/disc.rs`:
```rust
/// Check if mkvpropedit is available. Returns true if found on PATH.
pub fn has_mkvpropedit() -> bool {
    which::which("mkvpropedit").is_ok()
}
```

- [ ] **Step 2: Add disc mount helpers**

Add after the `has_mkvpropedit` function in `src/disc.rs`:
```rust
/// Get the mount point of a device if it's already mounted.
pub fn get_mount_point(device: &str) -> Option<String> {
    let output = Command::new("findmnt")
        .args(["-n", "-o", "TARGET", device])
        .output()
        .ok()?;

    if output.status.success() {
        let mount = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if mount.is_empty() {
            None
        } else {
            Some(mount)
        }
    } else {
        None
    }
}

/// Mount a disc using udisksctl. Returns the mount point on success.
pub fn mount_disc(device: &str) -> anyhow::Result<String> {
    let output = Command::new("udisksctl")
        .args(["mount", "-b", device])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to mount {}: {}", device, stderr.trim());
    }

    // udisksctl output: "Mounted /dev/sr0 at /run/media/user/LABEL."
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split(" at ")
        .nth(1)
        .map(|s| s.trim().trim_end_matches('.').to_string())
        .ok_or_else(|| anyhow::anyhow!("Could not parse mount point from udisksctl output"))
}

/// Unmount a disc using udisksctl.
pub fn unmount_disc(device: &str) -> anyhow::Result<()> {
    let status = Command::new("udisksctl")
        .args(["unmount", "-b", device])
        .status()?;

    if !status.success() {
        bail!("Failed to unmount {}", device);
    }
    Ok(())
}

/// Ensure the disc is mounted, returning (mount_point, did_we_mount_it).
/// If it was already mounted, returns the existing mount point.
/// If we mounted it, the caller should unmount when done.
pub fn ensure_mounted(device: &str) -> anyhow::Result<(String, bool)> {
    if let Some(mount) = get_mount_point(device) {
        Ok((mount, false))
    } else {
        let mount = mount_disc(device)?;
        Ok((mount, true))
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

Suggested message: `add mkvpropedit check and disc mount/unmount helpers`

---

### Task 6: Integrate chapter injection into CLI mode

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Add chapter injection after successful rip in `rip_selected`**

In `src/cli.rs`, the successful rip path is at line 587-588:
```rust
        let final_size = std::fs::metadata(outfile)?.len();
        println!("Done: {} ({})", filename, format_size(final_size));
```

Replace with:
```rust
        let final_size = std::fs::metadata(outfile)?.len();
        println!("Done: {} ({})", filename, format_size(final_size));

        // Apply chapter markers if mkvpropedit is available
        if has_mkvpropedit {
            if let Some(ref mount) = mount_point {
                if let Some(chapters) = crate::chapters::extract_chapters(
                    std::path::Path::new(mount),
                    &pl.num,
                ) {
                    match crate::chapters::apply_chapters(outfile, &chapters) {
                        Ok(true) => println!("  Added {} chapter markers", chapters.len()),
                        Ok(false) => {}
                        Err(e) => println!("  Warning: failed to add chapters: {}", e),
                    }
                }
            }
        }
```

- [ ] **Step 2: Add `has_mkvpropedit` and mount logic to `rip_selected`**

At the top of the `rip_selected` function (after the dry_run early return at line 482), add:
```rust
    let has_mkvpropedit = disc::has_mkvpropedit();
    if !has_mkvpropedit {
        println!("Note: mkvpropedit not found, chapters will not be added. Install mkvtoolnix for chapter support.");
    }

    let (mount_point, did_mount) = if has_mkvpropedit {
        match disc::ensure_mounted(device) {
            Ok((mount, did_mount)) => (Some(mount), did_mount),
            Err(e) => {
                println!("Warning: could not mount disc for chapter extraction: {}", e);
                (None, false)
            }
        }
    } else {
        (None, false)
    };
```

And at the end of the function, before the eject logic (before line 597), add:
```rust
    if did_mount {
        let _ = disc::unmount_disc(device);
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

Suggested message: `integrate chapter injection into CLI rip flow`

---

### Task 7: Integrate chapter injection into TUI mode

**Files:**
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/dashboard.rs`

- [ ] **Step 1: Add chapter-related state to `App`**

In `src/tui/mod.rs`, add these fields to the `App` struct (after `eject: bool` at line 88):
```rust
    pub has_mkvpropedit: bool,
    pub mount_point: Option<String>,
    pub did_mount: bool,
```

Initialize them in `App::new()` (after `eject: false,` at line 133):
```rust
            has_mkvpropedit: false,
            mount_point: None,
            did_mount: false,
```

- [ ] **Step 2: Set `has_mkvpropedit` during app init**

In `src/tui/mod.rs`, in the `run` function, after the `App` is created and config is set, add:
```rust
    app.has_mkvpropedit = crate::disc::has_mkvpropedit();
```

Find where `app.config = config.clone();` is set and add the line after it.

- [ ] **Step 3: Mount disc when transitioning to Ripping screen**

In `src/tui/mod.rs`, find where the screen transitions to `Screen::Ripping` (in the Confirm screen's Enter key handler). After the transition, add:
```rust
                        if app.has_mkvpropedit {
                            match crate::disc::ensure_mounted(
                                &app.args.device().to_string_lossy(),
                            ) {
                                Ok((mount, did_mount)) => {
                                    app.mount_point = Some(mount);
                                    app.did_mount = did_mount;
                                }
                                Err(_) => {
                                    app.mount_point = None;
                                    app.did_mount = false;
                                }
                            }
                        }
```

- [ ] **Step 4: Apply chapters after each successful rip in `poll_active_job`**

In `src/tui/dashboard.rs`, in `poll_active_job`, find the success path (line 364-367):
```rust
                if status.success() {
                    let outfile = app.args.output.join(&app.rip_jobs[idx].filename);
                    let file_size = std::fs::metadata(&outfile).map(|m| m.len()).unwrap_or(0);
                    app.rip_jobs[idx].status = PlaylistStatus::Done(file_size);
```

After `app.rip_jobs[idx].status = PlaylistStatus::Done(file_size);`, add:
```rust
                    // Apply chapter markers
                    if app.has_mkvpropedit {
                        if let Some(ref mount) = app.mount_point {
                            let playlist_num = &app.rip_jobs[idx].playlist.num;
                            if let Some(chapters) = crate::chapters::extract_chapters(
                                std::path::Path::new(mount.as_str()),
                                playlist_num,
                            ) {
                                let _ = crate::chapters::apply_chapters(&outfile, &chapters);
                            }
                        }
                    }
```

- [ ] **Step 5: Unmount disc on Done screen transition**

In `src/tui/dashboard.rs`, in `check_all_done`, after `app.screen = Screen::Done;` (line 269), add:
```rust
        if app.did_mount {
            let _ = crate::disc::unmount_disc(&app.args.device().to_string_lossy());
            app.did_mount = false;
        }
```

- [ ] **Step 6: Clean up mount state on rescan**

In `src/tui/mod.rs`, find the `reset_for_rescan` function (or wherever state is reset for Ctrl+R rescan). Add:
```rust
        if app.did_mount {
            let _ = crate::disc::unmount_disc(&app.args.device().to_string_lossy());
        }
        app.mount_point = None;
        app.did_mount = false;
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 8: Commit**

Suggested message: `integrate chapter injection into TUI rip flow`

---

### Task 8: End-to-end verification

- [ ] **Step 1: Run all tests**

Run: `cargo test`
Expected: all existing tests plus new chapter tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: no warnings

- [ ] **Step 3: Test with actual disc (if available)**

With the Stargate SG-1 S3D2 disc in the drive:
```bash
cargo run -- -d /dev/sr0 -o /tmp/bluback-test --no-tui --dry-run
```

Then a real single-episode rip to verify chapters are injected:
```bash
cargo run -- -d /dev/sr0 -o /tmp/bluback-test --no-tui -s 3 -e 7
```

After ripping, verify chapters:
```bash
ffprobe -v quiet -show_chapters /tmp/bluback-test/*.mkv
```

Expected: `[CHAPTER]` sections with correct timestamps

- [ ] **Step 4: Commit**

Suggested message: `verify chapter preservation end-to-end`
