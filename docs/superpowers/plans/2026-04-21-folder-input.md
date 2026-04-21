# BDMV Folder Input Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow bluback to rip from BDMV folder backups on disk, not just physical Blu-ray drives.

**Architecture:** Introduce an `InputSource` enum (`Disc`/`Folder`) resolved once early in the pipeline. Add `quick-xml` for parsing disc titles from `BDMV/META/DL/bdmt_*.xml`. Skip AACS preflight, device locking, max speed, mount/unmount, and eject for folder input. The `bluray:{path}` URL already works with libbluray for folders — most of the change is in the orchestration layer.

**Tech Stack:** Rust, quick-xml, existing ffmpeg-the-third/libbluray bindings

**Spec:** `docs/superpowers/specs/2026-04-20-folder-input-design.md`

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/types.rs` | `InputSource` enum definition and methods |
| `src/disc.rs` | `parse_bdmt_title()`, updated label resolution, folder-aware guards |
| `src/main.rs` | `InputSource` resolution, conditional AACS/lock skip, batch+folder conflict |
| `src/cli.rs` | Folder-aware `scan_disc()`, `list_playlists()`, `run_batch()` guard |
| `src/session.rs` | Folder-aware `start_disc_scan()` bypass, eject skip |
| `src/tui/coordinator.rs` | Folder-aware session spawning, skip DriveMonitor for folder |
| `src/tui/dashboard.rs` | Folder-aware done screen hints (hide eject) |
| `src/check.rs` | Folder-aware check output |
| `Cargo.toml` | Add `quick-xml` dependency |

---

### Task 1: Add `InputSource` enum to `src/types.rs`

**Files:**
- Modify: `src/types.rs`

- [ ] **Step 1: Write tests for `InputSource`**

Add at the bottom of the existing `#[cfg(test)] mod tests` block in `src/types.rs`:

```rust
#[test]
fn input_source_bluray_path_disc() {
    let src = InputSource::Disc { device: PathBuf::from("/dev/sr0") };
    assert_eq!(src.bluray_path(), Path::new("/dev/sr0"));
    assert!(!src.is_folder());
}

#[test]
fn input_source_bluray_path_folder() {
    let src = InputSource::Folder { path: PathBuf::from("/mnt/backup/vol01") };
    assert_eq!(src.bluray_path(), Path::new("/mnt/backup/vol01"));
    assert!(src.is_folder());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test input_source_bluray_path -- --test-threads=1`
Expected: compilation error — `InputSource` not defined

- [ ] **Step 3: Implement `InputSource` enum**

Add after the `LabelInfo` struct (around line 41) in `src/types.rs`:

```rust
#[derive(Debug, Clone)]
pub enum InputSource {
    Disc { device: PathBuf },
    Folder { path: PathBuf },
}

impl InputSource {
    pub fn bluray_path(&self) -> &std::path::Path {
        match self {
            InputSource::Disc { device } => device,
            InputSource::Folder { path } => path,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, InputSource::Folder { .. })
    }
}
```

Add `use std::path::{Path, PathBuf};` if not already imported (check existing imports — `PathBuf` is likely not imported in types.rs yet).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test input_source_bluray_path -- --test-threads=1`
Expected: both tests PASS

- [ ] **Step 5: Run clippy and commit**

Run: `cargo clippy -- -D warnings`

```bash
git add src/types.rs
git commit -m "feat: add InputSource enum for disc vs folder input"
```

---

### Task 2: Add `parse_bdmt_title()` with `quick-xml`

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/disc.rs`

- [ ] **Step 1: Add `quick-xml` dependency**

Add to `[dependencies]` in `Cargo.toml`:

```toml
quick-xml = "0.37"
```

Run: `cargo check` to verify the dependency resolves.

- [ ] **Step 2: Write tests for `parse_bdmt_title()`**

Add to the `#[cfg(test)] mod tests` block in `src/disc.rs`:

```rust
#[test]
fn parse_bdmt_title_valid_xml() {
    let dir = tempfile::tempdir().unwrap();
    let meta_dir = dir.path().join("BDMV/META/DL");
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(
        meta_dir.join("bdmt_eng.xml"),
        r#"<?xml version="1.0" encoding="utf-8"?>
<disclib xmlns="urn:BDA:bdmv;disclib">
  <di:discinfo xmlns:di="urn:BDA:bdmv;discinfo">
    <di:title>
      <di:name>My Great Movie</di:name>
    </di:title>
  </di:discinfo>
</disclib>"#,
    )
    .unwrap();
    assert_eq!(
        parse_bdmt_title(dir.path()),
        Some("My Great Movie".to_string())
    );
}

#[test]
fn parse_bdmt_title_with_bom() {
    let dir = tempfile::tempdir().unwrap();
    let meta_dir = dir.path().join("BDMV/META/DL");
    std::fs::create_dir_all(&meta_dir).unwrap();
    let mut content = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
    content.extend_from_slice(
        br#"<?xml version="1.0" encoding="utf-8"?>
<disclib xmlns="urn:BDA:bdmv;disclib">
  <di:discinfo xmlns:di="urn:BDA:bdmv;discinfo">
    <di:title>
      <di:name>BOM Test Title</di:name>
    </di:title>
  </di:discinfo>
</disclib>"#,
    );
    std::fs::write(meta_dir.join("bdmt_jpn.xml"), content).unwrap();
    assert_eq!(
        parse_bdmt_title(dir.path()),
        Some("BOM Test Title".to_string())
    );
}

#[test]
fn parse_bdmt_title_prefers_eng() {
    let dir = tempfile::tempdir().unwrap();
    let meta_dir = dir.path().join("BDMV/META/DL");
    std::fs::create_dir_all(&meta_dir).unwrap();
    let make_xml = |name: &str| {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<disclib xmlns="urn:BDA:bdmv;disclib">
  <di:discinfo xmlns:di="urn:BDA:bdmv;discinfo">
    <di:title>
      <di:name>{}</di:name>
    </di:title>
  </di:discinfo>
</disclib>"#,
            name
        )
    };
    std::fs::write(meta_dir.join("bdmt_jpn.xml"), make_xml("日本語タイトル")).unwrap();
    std::fs::write(meta_dir.join("bdmt_eng.xml"), make_xml("English Title")).unwrap();
    assert_eq!(
        parse_bdmt_title(dir.path()),
        Some("English Title".to_string())
    );
}

#[test]
fn parse_bdmt_title_no_meta_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("BDMV")).unwrap();
    assert_eq!(parse_bdmt_title(dir.path()), None);
}

#[test]
fn parse_bdmt_title_malformed_xml() {
    let dir = tempfile::tempdir().unwrap();
    let meta_dir = dir.path().join("BDMV/META/DL");
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(meta_dir.join("bdmt_eng.xml"), "not xml at all").unwrap();
    assert_eq!(parse_bdmt_title(dir.path()), None);
}

#[test]
fn parse_bdmt_title_missing_name_element() {
    let dir = tempfile::tempdir().unwrap();
    let meta_dir = dir.path().join("BDMV/META/DL");
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(
        meta_dir.join("bdmt_eng.xml"),
        r#"<?xml version="1.0" encoding="utf-8"?>
<disclib xmlns="urn:BDA:bdmv;disclib">
  <di:discinfo xmlns:di="urn:BDA:bdmv;discinfo">
    <di:title></di:title>
  </di:discinfo>
</disclib>"#,
    )
    .unwrap();
    assert_eq!(parse_bdmt_title(dir.path()), None);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test parse_bdmt_title -- --test-threads=1`
Expected: compilation error — `parse_bdmt_title` not defined

- [ ] **Step 4: Implement `parse_bdmt_title()`**

Add to `src/disc.rs`, after the existing `parse_volume_label()` function (around line 319):

```rust
/// Parse disc title from BDMV/META/DL/bdmt_*.xml metadata.
///
/// Prefers `bdmt_eng.xml` if present; otherwise uses the first bdmt file found.
/// Returns the content of the `<di:name>` element.
pub fn parse_bdmt_title(bdmv_root: &std::path::Path) -> Option<String> {
    let meta_dir = bdmv_root.join("BDMV").join("META").join("DL");
    if !meta_dir.is_dir() {
        return None;
    }

    let entries: Vec<_> = std::fs::read_dir(&meta_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("bdmt_") && name.ends_with(".xml")
        })
        .collect();

    if entries.is_empty() {
        return None;
    }

    // Prefer English, fall back to first found
    let target = entries
        .iter()
        .find(|e| e.file_name().to_string_lossy() == "bdmt_eng.xml")
        .or_else(|| entries.first())?;

    parse_bdmt_xml(&target.path())
}

fn parse_bdmt_xml(path: &std::path::Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;

    // Strip UTF-8 BOM if present
    let data = if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &data[3..]
    } else {
        &data
    };

    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_reader(data);
    let mut in_name = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let local = e.local_name();
                if local.as_ref() == b"name" {
                    in_name = true;
                }
            }
            Ok(Event::Text(e)) if in_name => {
                let text = e.unescape().ok()?.trim().to_string();
                if !text.is_empty() {
                    return Some(text);
                }
                in_name = false;
            }
            Ok(Event::End(_)) if in_name => {
                in_name = false;
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }

    None
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test parse_bdmt_title -- --test-threads=1`
Expected: all 6 tests PASS

- [ ] **Step 6: Run clippy and commit**

Run: `cargo clippy -- -D warnings`

```bash
git add Cargo.toml Cargo.lock src/disc.rs
git commit -m "feat: parse disc title from BDMV/META/DL/bdmt_*.xml"
```

---

### Task 3: Add `resolve_input_source()` to `src/disc.rs`

**Files:**
- Modify: `src/disc.rs`

- [ ] **Step 1: Write tests for `resolve_input_source()`**

Add to the `#[cfg(test)] mod tests` block in `src/disc.rs`:

```rust
#[test]
fn resolve_input_source_folder_with_bdmv() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("BDMV")).unwrap();
    let src = resolve_input_source(dir.path()).unwrap();
    assert!(src.is_folder());
    assert_eq!(src.bluray_path(), dir.path());
}

#[test]
fn resolve_input_source_folder_without_bdmv() {
    let dir = tempfile::tempdir().unwrap();
    let result = resolve_input_source(dir.path());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("BDMV"), "error should mention BDMV: {}", msg);
}

#[test]
fn resolve_input_source_block_device_path() {
    // Non-directory path treated as disc device
    let src = resolve_input_source(std::path::Path::new("/dev/sr0")).unwrap();
    assert!(!src.is_folder());
    assert_eq!(src.bluray_path(), std::path::Path::new("/dev/sr0"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test resolve_input_source -- --test-threads=1`
Expected: compilation error — `resolve_input_source` not defined

- [ ] **Step 3: Implement `resolve_input_source()`**

Add to `src/disc.rs`:

```rust
use crate::types::InputSource;

pub fn resolve_input_source(path: &std::path::Path) -> anyhow::Result<InputSource> {
    if path.is_dir() {
        if path.join("BDMV").is_dir() {
            Ok(InputSource::Folder {
                path: path.to_path_buf(),
            })
        } else {
            bail!(
                "Directory '{}' does not contain a BDMV structure. \
                 Expected a 'BDMV/' subdirectory.",
                path.display()
            )
        }
    } else {
        Ok(InputSource::Disc {
            device: path.to_path_buf(),
        })
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test resolve_input_source -- --test-threads=1`
Expected: all 3 tests PASS

- [ ] **Step 5: Run clippy and commit**

Run: `cargo clippy -- -D warnings`

```bash
git add src/disc.rs
git commit -m "feat: add resolve_input_source() for disc vs folder detection"
```

---

### Task 4: Add `resolve_label()` for unified label resolution

**Files:**
- Modify: `src/disc.rs`

- [ ] **Step 1: Write tests for `resolve_label()`**

Add to the `#[cfg(test)] mod tests` block in `src/disc.rs`:

```rust
#[test]
fn resolve_label_folder_with_bdmt() {
    let dir = tempfile::tempdir().unwrap();
    let meta_dir = dir.path().join("BDMV/META/DL");
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(
        meta_dir.join("bdmt_eng.xml"),
        r#"<?xml version="1.0" encoding="utf-8"?>
<disclib xmlns="urn:BDA:bdmv;disclib">
  <di:discinfo xmlns:di="urn:BDA:bdmv;discinfo">
    <di:title><di:name>Test Show Vol.1</di:name></di:title>
  </di:discinfo>
</disclib>"#,
    )
    .unwrap();
    let src = InputSource::Folder { path: dir.path().to_path_buf() };
    let label = resolve_label(&src, None);
    assert_eq!(label, "Test Show Vol.1");
}

#[test]
fn resolve_label_folder_without_bdmt_uses_basename() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("BDMV")).unwrap();
    let src = InputSource::Folder { path: dir.path().to_path_buf() };
    let label = resolve_label(&src, None);
    // tempdir basename varies, just check it's non-empty
    assert!(!label.is_empty());
}

#[test]
fn resolve_label_folder_mount_point_override() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("BDMV")).unwrap();
    let src = InputSource::Folder { path: dir.path().to_path_buf() };
    let label = resolve_label(&src, Some("/mnt/disc"));
    // Mount point is for disc label upgrade, folder ignores it
    assert!(!label.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test resolve_label -- --test-threads=1`
Expected: compilation error — `resolve_label` not defined

- [ ] **Step 3: Implement `resolve_label()`**

Add to `src/disc.rs`:

```rust
/// Resolve the best available label for an input source.
///
/// For folders: tries bdmt_*.xml title, falls back to folder basename.
/// For discs: tries bdmt_*.xml from mount_point (if available), falls back to lsblk/diskutil label.
pub fn resolve_label(source: &InputSource, mount_point: Option<&str>) -> String {
    match source {
        InputSource::Folder { path } => {
            if let Some(title) = parse_bdmt_title(path) {
                return title;
            }
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        }
        InputSource::Disc { device } => {
            // Try bdmt_*.xml from mount point if available
            if let Some(mp) = mount_point {
                if let Some(title) = parse_bdmt_title(std::path::Path::new(mp)) {
                    return title;
                }
            }
            get_volume_label(&device.to_string_lossy())
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test resolve_label -- --test-threads=1`
Expected: all 3 tests PASS

- [ ] **Step 5: Run clippy and commit**

Run: `cargo clippy -- -D warnings`

```bash
git add src/disc.rs
git commit -m "feat: add resolve_label() with bdmt_*.xml priority"
```

---

### Task 5: Wire `InputSource` into `main.rs`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Update device resolution to produce `InputSource`**

In `src/main.rs`, the device resolution block at lines 489-507 currently sets `args.device` to a `PathBuf`. After this block, add `InputSource` resolution and conditional AACS/lock skipping. Replace the section from line 467 (aacs preflight) through line 514 (end of lock) with:

```rust
    // Resolve input source from device path (if available at this point)
    let input_source = args.device.as_ref().map(|d| disc::resolve_input_source(d));
    let is_folder_input = match &input_source {
        Some(Ok(src)) => src.is_folder(),
        _ => false,
    };

    // AACS preflight: skip for folder input (already decrypted)
    if !is_folder_input {
        aacs::preflight(aacs_backend)?;
    }

    // Suppress libbluray's BD_DEBUG stderr output unless verbose mode is on.
    // Must be set before any ffmpeg/libbluray calls.
    if !config.verbose_libbluray() {
        std::env::set_var("BD_DEBUG_MASK", "0");
    }
```

Keep the `--settings` block and output dir resolution as-is (lines 476-487).

Then update the device resolution block (lines 489-507). After the existing device resolution, add folder validation:

```rust
    // If device was set and is a folder, validate BDMV structure
    if let Some(ref dev) = args.device {
        if dev.is_dir() && !dev.join("BDMV").is_dir() {
            anyhow::bail!(
                "Directory '{}' does not contain a BDMV structure. Expected a 'BDMV/' subdirectory.",
                dev.display()
            );
        }
    }

    // Re-resolve input source after device resolution
    let is_folder_input = args
        .device
        .as_ref()
        .map(|d| d.is_dir())
        .unwrap_or(false);
```

Update the lock block (lines 510-514) to skip for folders:

```rust
    // Acquire per-device lock to prevent multiple bluback processes from contending
    let _device_lock = if let Some(ref dev) = args.device {
        if !is_folder_input {
            Some(disc::try_lock_device(&dev.to_string_lossy())?)
        } else {
            None
        }
    } else {
        None
    };
```

Update the batch conflict check. Find where `batch` is resolved (line 544) and add after it:

```rust
    if batch && is_folder_input {
        anyhow::bail!(
            "batch mode is not supported with folder input \
             (use --batch-dir for multi-volume folders in a future release)"
        );
    }
```

- [ ] **Step 2: Update `-d` help text**

Change line 42 from:
```rust
    /// Blu-ray device path [default: auto-detect]
```
to:
```rust
    /// Blu-ray device or BDMV folder path [default: auto-detect]
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: no errors (downstream functions still receive `args.device()` as before)

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: all existing tests pass (no behavioral changes yet)

- [ ] **Step 5: Run clippy and commit**

Run: `cargo clippy -- -D warnings`

```bash
git add src/main.rs
git commit -m "feat: resolve InputSource in main, skip AACS/lock for folders"
```

---

### Task 6: Update `cli.rs` for folder-aware scanning

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Write test for batch + folder conflict**

Add to the `#[cfg(test)] mod tests` block in `src/cli.rs` (or create an integration test in `tests/`):

The batch+folder conflict is handled in main.rs (Task 5), so we don't need a unit test here. Instead, verify the `scan_disc` changes work for both disc and folder paths.

- [ ] **Step 2: Update `scan_disc()` to skip disc-specific ops for folders**

In `src/cli.rs`, modify `scan_disc()` (line 883). Add a folder check after the device string is obtained:

```rust
fn scan_disc(
    args: &Args,
    config: &crate::config::Config,
) -> anyhow::Result<(
    String,
    Option<LabelInfo>,
    Vec<Playlist>,
    bool,
    crate::types::ProbeCache,
    u32,
    std::collections::HashSet<String>,
)> {
    let device = args.device().to_string_lossy();
    let is_folder = args.device().is_dir();

    if !args.device().exists() {
        anyhow::bail!("No Blu-ray {} found at {}", if is_folder { "folder" } else { "device" }, device);
    }

    // Skip disc-specific speed setting for folder input
    if !is_folder && config.should_max_speed(args.no_max_speed) {
        disc::set_max_speed(&device);
    }

    // Label resolution: for folders, use bdmt_*.xml or folder basename
    // For discs, use lsblk label (bdmt upgrade happens after mount, below)
    let label = if is_folder {
        disc::resolve_label(
            &crate::types::InputSource::Folder { path: args.device().to_path_buf() },
            None,
        )
    } else {
        disc::get_volume_label(&device)
    };
    let label_info = disc::parse_volume_label(&label);
    if !label.is_empty() {
        println!("Volume label: {}", label);
    }
    // ... rest of function unchanged
```

- [ ] **Step 3: Update `list_playlists()` similarly**

In `src/cli.rs`, modify `list_playlists()` (line 74). Add folder awareness:

Replace lines 74-88:
```rust
pub fn list_playlists(args: &Args, config: &crate::config::Config) -> anyhow::Result<()> {
    let device = args.device().to_string_lossy();
    let is_folder = args.device().is_dir();

    if !args.device().exists() {
        anyhow::bail!("No Blu-ray {} found at {}", if is_folder { "folder" } else { "device" }, device);
    }

    if !is_folder && config.should_max_speed(args.no_max_speed) {
        disc::set_max_speed(&device);
    }

    let label = if is_folder {
        disc::resolve_label(
            &crate::types::InputSource::Folder { path: args.device().to_path_buf() },
            None,
        )
    } else {
        disc::get_volume_label(&device)
    };
    if !label.is_empty() {
        println!("Volume label: {}", label);
    }
```

Also update the "Scanning disc" progress message (line 101) to be input-aware:

```rust
    let source_name = if is_folder { "folder" } else { "disc" };
    eprint!("Scanning {} at {}...", source_name, device);
```

And the AACS negotiation progress callback (line 107-110) — for folders this won't fire (no AACS), but the message text should still be correct:

```rust
        Some(&|elapsed, timeout| {
            eprint!(
                "\rScanning {} at {} (AACS negotiation {}s/{}s)...",
                source_name, device, elapsed, timeout
            );
        }),
```

- [ ] **Step 4: Update eject calls in `run()` to skip for folders**

In `src/cli.rs`, find the eject call near line 2198 in `rip_selected()`. Wrap with a folder check:

The eject in `rip_selected` uses `args.device()`, so add:

```rust
    if !args.device().is_dir() && should_eject {
        // existing eject logic
    }
```

- [ ] **Step 5: Guard `run_batch()` for folders**

The batch+folder conflict is already caught in `main.rs` (Task 5), but add a defensive check at the top of `run_batch()` (line 2520):

```rust
    if args.device().is_dir() {
        anyhow::bail!("batch mode is not supported with folder input");
    }
```

- [ ] **Step 6: Verify compilation and tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 7: Run clippy and commit**

Run: `cargo clippy -- -D warnings`

```bash
git add src/cli.rs
git commit -m "feat: folder-aware scanning and label resolution in CLI mode"
```

---

### Task 7: Update `session.rs` for folder-aware TUI scanning

**Files:**
- Modify: `src/session.rs`

- [ ] **Step 1: Write test for folder scan thread bypass**

Add to the `#[cfg(test)] mod tests` block in `src/session.rs`:

```rust
#[test]
fn start_disc_scan_folder_skips_volume_label_poll() {
    // A folder-based session should not poll get_volume_label
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let (msg_tx, _msg_rx) = std::sync::mpsc::channel();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("BDMV")).unwrap();

    let mut session = DriveSession::new(
        dir.path().to_path_buf(),
        crate::config::Config::default(),
        crate::streams::StreamFilter::default(),
        cmd_rx,
        msg_tx,
    );
    // Verify the device is detected as a folder
    assert!(session.device.is_dir());
}
```

- [ ] **Step 2: Modify `start_disc_scan()` to handle folder input**

In `src/session.rs`, modify `start_disc_scan()` (line 667). The key change is that for folder input, we skip the `get_volume_label()` polling loop and `set_max_speed()`, and instead resolve the label immediately via `disc::resolve_label()`:

```rust
    pub fn start_disc_scan(&mut self) {
        let device = self.device.clone();
        let is_folder = device.is_dir();
        let max_speed = self.config.should_max_speed(self.no_max_speed);
        let min_probe_duration = self.config.min_probe_duration(self.min_probe_duration_arg);
        let auto_detect = self.auto_detect;
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::Builder::new()
            .name(format!("scan-{}", self.device.display()))
            .spawn(move || {
                let dev_str = device.to_string_lossy().to_string();

                let label = if is_folder {
                    // Folder input: resolve label immediately, no polling needed
                    crate::disc::resolve_label(
                        &crate::types::InputSource::Folder { path: device.clone() },
                        None,
                    )
                } else {
                    // Disc input: poll for disc presence every 2 seconds until found
                    loop {
                        let l = crate::disc::get_volume_label(&dev_str);
                        if !l.is_empty() {
                            break l;
                        }
                        let msg = format!("{} — no disc", dev_str);
                        if tx.send(BackgroundResult::WaitingForDisc(msg)).is_err() {
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                };

                let _ = tx.send(BackgroundResult::DiscFound(dev_str.clone()));
                if !is_folder && max_speed {
                    crate::disc::set_max_speed(&dev_str);
                }
                let tx_progress = tx.clone();
                let tx_probe = tx.clone();
                let result = (|| -> anyhow::Result<DiscScanResult> {
                    let (playlists, probe_cache, skip_set) =
                        crate::media::scan_playlists_with_progress(
                            &dev_str,
                            min_probe_duration,
                            auto_detect,
                            Some(&move |elapsed, timeout| {
                                let _ = tx_progress
                                    .send(BackgroundResult::ScanProgress(elapsed, timeout));
                            }),
                            Some(&move |current, total, num| {
                                let _ = tx_probe.send(BackgroundResult::ProbeProgress(
                                    current,
                                    total,
                                    num.to_string(),
                                ));
                            }),
                        )
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    Ok((dev_str, label, playlists, probe_cache, skip_set))
                })();
                let _ = tx.send(BackgroundResult::DiscScan(result));
            })
            .expect("failed to spawn scan thread");

        self.pending_rx = Some(rx);
        self.disc.scan_log.push("Scanning for disc...".into());
        self.status_message = "Scanning for disc...".into();
        self.screen = Screen::Scanning;
    }
```

- [ ] **Step 3: Skip eject in `handle_key()` for folder input**

In `src/session.rs`, modify the Ctrl+E handler (line 1070-1084) to skip eject for folders:

```rust
        // Ctrl+E: eject this session's disc (not during ripping, text input, or folder input)
        if key.code == KeyCode::Char('e')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && !input_active
            && self.screen != Screen::Ripping
            && !self.device.is_dir()
        {
            // existing eject logic unchanged
        }
```

- [ ] **Step 4: Skip eject in `RenderSnapshot` for folder input**

Find where `eject` is set on the `RenderSnapshot` (search for `eject: self.eject` in `session.rs`, around line 503). Ensure it's set to `false` for folder input:

```rust
            eject: self.eject && !self.device.is_dir(),
```

- [ ] **Step 5: Verify compilation and tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 6: Run clippy and commit**

Run: `cargo clippy -- -D warnings`

```bash
git add src/session.rs
git commit -m "feat: folder-aware TUI scan thread (skip polling, eject)"
```

---

### Task 8: Update TUI coordinator and dashboard for folder input

**Files:**
- Modify: `src/tui/coordinator.rs`
- Modify: `src/tui/dashboard.rs`

- [ ] **Step 1: Skip DriveMonitor for folder input in coordinator**

In `src/tui/coordinator.rs`, the `Coordinator::new()` function (line 51) always spawns a DriveMonitor. When a `-d` folder path is given, DriveMonitor is unnecessary. Modify the constructor:

```rust
    pub fn new(
        args: Args,
        config: Config,
        config_path: PathBuf,
        stream_filter: crate::streams::StreamFilter,
    ) -> Self {
        let (drive_tx, drive_rx) = mpsc::channel();

        // Only spawn DriveMonitor for disc input (not folder input)
        let is_folder = args
            .device
            .as_ref()
            .map(|d| d.is_dir())
            .unwrap_or(false);
        if !is_folder {
            DriveMonitor::spawn(Duration::from_secs(2), drive_tx);
        }

        Self {
            sessions: Vec::new(),
            active_tab: 0,
            config,
            config_path,
            args,
            stream_filter,
            quit: false,
            overlay: None,
            drive_event_rx: drive_rx,
            assigned_episodes: HashMap::new(),
            history_db_path: None,
            history_db: None,
        }
    }
```

- [ ] **Step 2: Disable multi-drive Ctrl+N for folder input**

In `handle_key()` in `coordinator.rs`, find the Ctrl+N handler (around line 360-388 where it spawns a new session for auto-detected drives). Add a folder guard:

```rust
        // Ctrl+N: new session — not available for folder input
        if key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::CONTROL) {
            let is_folder = self.args.device.as_ref().map(|d| d.is_dir()).unwrap_or(false);
            if is_folder {
                return; // multi-drive not applicable for folder input
            }
            // ... existing logic
        }
```

- [ ] **Step 3: Update done screen hints to hide eject for folder input**

In `src/tui/dashboard.rs`, find the done screen hint rendering (around line 484-488). The `eject` field on the view snapshot already controls whether eject is available (from Task 7 step 4). Update the hint to conditionally show `[Ctrl+E] Eject`:

Check how the existing hint is rendered. The current code at line 484 is:
```rust
    let hint = Paragraph::new(
        "[Enter/Ctrl+R] Rescan  [Ctrl+E] Eject  [Ctrl+S] Settings  [any other key] Exit",
    )
```

Replace with:
```rust
    let hint_text = if view.eject {
        "[Enter/Ctrl+R] Rescan  [Ctrl+E] Eject  [Ctrl+S] Settings  [any other key] Exit"
    } else {
        "[Enter/Ctrl+R] Rescan  [Ctrl+S] Settings  [any other key] Exit"
    };
    let hint = Paragraph::new(hint_text)
```

- [ ] **Step 4: Update batch auto-eject to skip for folders**

In `src/tui/dashboard.rs`, find the batch auto-eject code (around line 757-764). Add a folder guard:

```rust
        if session.batch && !session.device.is_dir() {
            let device = session.device.to_string_lossy();
            if let Err(e) = crate::disc::eject_disc(&device) {
                // ...
            }
        }
```

- [ ] **Step 5: Verify compilation and tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 6: Run clippy and commit**

Run: `cargo clippy -- -D warnings`

```bash
git add src/tui/coordinator.rs src/tui/dashboard.rs
git commit -m "feat: skip DriveMonitor and eject for folder input in TUI"
```

---

### Task 9: Update `check.rs` for folder-aware validation

**Files:**
- Modify: `src/check.rs`

- [ ] **Step 1: Update `--check` to handle folder input**

In `src/main.rs`, the `--check` dispatch at line 421-423 currently runs before device resolution. Move the check dispatch after device resolution (after line 507) and pass the device info:

In `src/main.rs`, replace the check dispatch:

```rust
    // Move --check dispatch here (after device resolution)
    if args.check {
        let is_folder = args.device.as_ref().map(|d| d.is_dir()).unwrap_or(false);
        return Ok(check::run_check(&config, &config_path, is_folder));
    }
```

Remove the earlier `--check` dispatch at line 421-423.

In `src/check.rs`, update the `run_check()` signature:

```rust
pub fn run_check(config: &crate::config::Config, config_path: &std::path::Path, is_folder_input: bool) -> i32 {
```

Then wrap the AACS-related checks (libaacs, KEYDB.cfg, libmmbd, makemkvcon) in a condition:

```rust
    if is_folder_input {
        results.push(CheckResult {
            label: "Input mode".into(),
            status: CheckStatus::Pass,
            detail: "BDMV folder (AACS checks skipped)".into(),
        });
    } else {
        // Check 3: libaacs (required)
        // ... existing libaacs, KEYDB.cfg, libmmbd, makemkvcon checks
    }
```

- [ ] **Step 2: Verify compilation and tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 3: Run clippy and commit**

Run: `cargo clippy -- -D warnings`

```bash
git add src/main.rs src/check.rs
git commit -m "feat: folder-aware --check output (skip AACS checks)"
```

---

### Task 10: Integration tests for folder input

**Files:**
- Create: `tests/folder_input.rs`
- Modify: `tests/fixtures/` (add BDMV fixture structure)

- [ ] **Step 1: Create minimal BDMV fixture structure**

```bash
mkdir -p tests/fixtures/bdmv_sample/BDMV/META/DL
mkdir -p tests/fixtures/bdmv_sample/BDMV/PLAYLIST
mkdir -p tests/fixtures/bdmv_sample/BDMV/STREAM
mkdir -p tests/fixtures/bdmv_sample/BDMV/CLIPINF
```

Create `tests/fixtures/bdmv_sample/BDMV/META/DL/bdmt_eng.xml`:
```xml
<?xml version="1.0" encoding="utf-8"?>
<disclib xmlns="urn:BDA:bdmv;disclib">
  <di:discinfo xmlns:di="urn:BDA:bdmv;discinfo">
    <di:title>
      <di:name>Test Show Vol.1 Disc1</di:name>
    </di:title>
  </di:discinfo>
</disclib>
```

- [ ] **Step 2: Write integration tests**

Create `tests/folder_input.rs`:

```rust
use std::path::Path;

#[test]
fn folder_with_bdmv_is_detected_as_folder_input() {
    let fixture = Path::new("tests/fixtures/bdmv_sample");
    assert!(fixture.join("BDMV").is_dir());

    // The resolve function should detect this as folder input
    let result = bluback::disc::resolve_input_source(fixture);
    assert!(result.is_ok());
    let source = result.unwrap();
    assert!(source.is_folder());
}

#[test]
fn folder_without_bdmv_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let result = bluback::disc::resolve_input_source(dir.path());
    assert!(result.is_err());
}

#[test]
fn bdmt_title_extracted_from_fixture() {
    let fixture = Path::new("tests/fixtures/bdmv_sample");
    let title = bluback::disc::parse_bdmt_title(fixture);
    assert_eq!(title, Some("Test Show Vol.1 Disc1".to_string()));
}

#[test]
fn resolve_label_uses_bdmt_for_folder() {
    let fixture = Path::new("tests/fixtures/bdmv_sample");
    let source = bluback::types::InputSource::Folder {
        path: fixture.to_path_buf(),
    };
    let label = bluback::disc::resolve_label(&source, None);
    assert_eq!(label, "Test Show Vol.1 Disc1");
}
```

Note: These tests require that `resolve_input_source`, `parse_bdmt_title`, and `resolve_label` are public. They already are from the implementations above. Also verify that `src/lib.rs` or the crate root re-exports these modules. If bluback is a binary crate only, these tests should use `use` paths appropriately or be converted to unit tests. Check `src/lib.rs` — if it doesn't exist, add one that re-exports the relevant modules:

```rust
// src/lib.rs
pub mod disc;
pub mod types;
```

If `src/lib.rs` already exists, just ensure `disc` and `types` are re-exported.

- [ ] **Step 3: Run integration tests**

Run: `cargo test --test folder_input`
Expected: all 4 tests PASS

- [ ] **Step 4: Run clippy and commit**

Run: `cargo clippy -- -D warnings`

```bash
git add tests/folder_input.rs tests/fixtures/bdmv_sample/
git commit -m "test: add integration tests for BDMV folder input"
```

---

### Task 11: CLI flag conflict test for `--batch` with folder input

**Files:**
- Modify: `tests/folder_input.rs` (or new `tests/cli_folder_conflicts.rs`)

- [ ] **Step 1: Write conflict test**

The `--batch` + folder conflict is validated at runtime in `main.rs` (not via clap `conflicts_with`), so we test it by checking the error message. Add to `tests/folder_input.rs`:

```rust
#[test]
fn batch_flag_with_folder_input_is_rejected() {
    let fixture_path = std::path::Path::new("tests/fixtures/bdmv_sample")
        .canonicalize()
        .expect("fixture exists");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bluback"))
        .args([
            "--batch",
            "-d",
            fixture_path.to_str().unwrap(),
            "--no-tui",
        ])
        .output()
        .expect("failed to run bluback");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("batch mode is not supported with folder input"),
        "expected batch+folder error, got: {}",
        stderr
    );
    assert!(!output.status.success());
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --test folder_input batch_flag`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/folder_input.rs
git commit -m "test: add batch+folder conflict integration test"
```

---

### Task 12: Update label upgrade for physical discs after scan

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/session.rs`

This task implements the bdmt_*.xml label *upgrade* for physical disc input — after scanning and mounting, we check if bdmt_*.xml provides a richer title than the lsblk label.

**IMPORTANT:** The display label (`disc.label` / `label`) should be upgraded to the richer bdmt title, but `label_info` (parsed `LabelInfo` with season/disc numbers) MUST remain derived from the original `lsblk` volume label. The bdmt title format (e.g., `シャングリラ・フロンティア Vol.1 Disc1`) won't match the `SHOW_S1D2` regex patterns used by `parse_volume_label()`.

- [ ] **Step 1: Upgrade label in CLI `scan_disc()` after scanning**

In `src/cli.rs`, the `scan_disc()` function currently doesn't call `ensure_mounted()` — that happens later in `run()` for chapter extraction. We need to try a bdmt label upgrade after `scan_playlists_with_progress()` returns, since the scan may have caused libbluray to mount the disc.

After the `scan_playlists_with_progress` block (around line 937) and before the return, add the label upgrade. The `label_info` is already parsed from the original `lsblk` label above and must NOT be re-parsed:

```rust
    // Upgrade display label from bdmt_*.xml if available (richer than lsblk label)
    // label_info stays derived from the original lsblk label (bdmt format won't match regex)
    let label = if !is_folder {
        match disc::ensure_mounted(&device) {
            Ok((mount, _did_mount)) => {
                let upgraded = disc::parse_bdmt_title(std::path::Path::new(&mount));
                upgraded.unwrap_or(label)
            }
            Err(_) => label,
        }
    } else {
        label
    };
```

Note: `ensure_mounted` is cheap if the disc is already mounted (just checks `get_mount_point`). Place this BEFORE the `return Ok(...)` but AFTER the `label_info` is parsed from the original label.

- [ ] **Step 2: Upgrade label in TUI `poll_background()` after scan**

In `src/session.rs`, find `poll_background()` where `BackgroundResult::DiscScan(Ok(...))` is handled (around line 822). After the label and label_info are stored on `disc`, add the bdmt upgrade. Only upgrade `disc.label`, NOT `disc.label_info`:

```rust
    // Existing code stores label and label_info:
    // self.disc.label = label.clone();
    // self.disc.label_info = disc::parse_volume_label(&label);

    // Upgrade display label from bdmt_*.xml if mount point is available
    // label_info stays from original lsblk label (bdmt format won't match regex)
    if !self.device.is_dir() {
        if let Some(ref mp) = self.disc.mount_point {
            if let Some(bdmt_title) = crate::disc::parse_bdmt_title(std::path::Path::new(mp)) {
                self.disc.label = bdmt_title;
            }
        }
    }
```

- [ ] **Step 3: Verify compilation and tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Run clippy and commit**

Run: `cargo clippy -- -D warnings`

```bash
git add src/cli.rs src/session.rs
git commit -m "feat: upgrade disc label from bdmt_*.xml after scan"
```

---

### Task 13: Final validation

**Files:** none (validation only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: no warnings

- [ ] **Step 3: Run formatter**

Run: `rustup run stable cargo fmt`

- [ ] **Step 4: Manual testing with real BDMV folder**

Test with the Shangri-La Frontier backup:

```bash
# CLI mode, single volume
cargo run -- -d "/run/media/anten/media/[BDMV] Shangri-La Frontier/vol.01/BD_VIDEO" --list-playlists

# CLI mode with --title override
cargo run -- -d "/run/media/anten/media/[BDMV] Shangri-La Frontier/vol.01/BD_VIDEO" --title "Shangri-La Frontier" --season 1 --no-tui --dry-run

# TUI mode
cargo run -- -d "/run/media/anten/media/[BDMV] Shangri-La Frontier/vol.01/BD_VIDEO"

# Verify batch+folder conflict
cargo run -- -d "/run/media/anten/media/[BDMV] Shangri-La Frontier/vol.01/BD_VIDEO" --batch
# Expected: error about batch not supported with folder input

# Verify --check with folder
cargo run -- -d "/run/media/anten/media/[BDMV] Shangri-La Frontier/vol.01/BD_VIDEO" --check
# Expected: AACS checks skipped, shows "BDMV folder" mode
```

- [ ] **Step 5: Commit any final fixes and tag**

```bash
git add -A
git commit -m "chore: final validation fixes for folder input"
```
