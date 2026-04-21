# BDMV Folder Input Support

**Date:** 2026-04-20
**Version target:** v0.12 (alongside DVD support) or standalone minor release
**Status:** Approved

## Summary

Add support for ripping from BDMV folder backups (decrypted Blu-ray disc images stored on disk) in addition to physical disc drives. Users pass a directory path via `-d /path/to/bdmv_root/` and bluback processes it identically to a physical disc — same scan, TMDb lookup, episode assignment, and remux workflow.

## Motivation

- MakeMKV or other tools may have already created BDMV folder backups that need remuxing to MKV
- Pre-existing disc backups from other sources (purchased digital, shared archives) have the same BDMV structure
- Eliminates the need for a physical drive when processing already-ripped content
- bluback's episode naming, TMDb integration, chapter embedding, and stream selection provide value even when the source is a folder rather than a disc

## Design

### InputSource Enum

Introduce `InputSource` in `src/types.rs`:

```rust
pub enum InputSource {
    Disc { device: PathBuf },
    Folder { path: PathBuf },
}

impl InputSource {
    /// The path string for bluray:{} URLs, ensure_mounted(), and MPLS reading.
    pub fn bluray_path(&self) -> &Path;

    pub fn is_folder(&self) -> bool;
}
```

**Resolution:** Performed once, early in `run_inner()` (CLI) and `Session::new()` (TUI):

- If `-d` path is a directory containing `BDMV/`: `InputSource::Folder { path }`
- If `-d` path is a block device (or auto-detected via DriveMonitor): `InputSource::Disc { device }`
- If `-d` path is a directory without `BDMV/`: error — "directory does not contain a BDMV structure"
- If no `-d` and no config device: existing auto-detect logic (DriveMonitor for TUI, `detect_optical_drives()` for CLI)

The resolved `InputSource` replaces the raw `device: &str` in function signatures that need to branch on input type. Functions that only need the path for `bluray:{path}` or filesystem reads call `.bluray_path()`.

### Volume Label Detection with bdmt_*.xml

New function for parsing disc metadata from the BDMV structure:

```rust
/// Parse disc title from BDMV/META/DL/bdmt_*.xml
pub fn parse_bdmt_title(bdmv_root: &Path) -> Option<String>
```

Reads `bdmv_root/BDMV/META/DL/bdmt_*.xml`, parses the `<di:name>` element from the disclib XML schema. Uses `quick-xml` crate (event reader API) for proper XML parsing with correct error handling.

**Language preference for multiple bdmt files:** If `bdmt_eng.xml` exists, prefer it. Otherwise use the first `bdmt_*.xml` found.

**Label resolution priority chain (unified for disc and folder input):**

1. `--title` CLI flag (bypasses label entirely, existing behavior)
2. `bdmt_*.xml` `<di:name>` from BDMV metadata (new — requires mount point for discs, direct path for folders)
3. `lsblk`/`diskutil` volume label (disc only)
4. Folder basename (folder only)

This applies to **both** disc and folder inputs — physical disc rips also benefit from richer `bdmt_*.xml` titles instead of truncated volume labels.

**Timing for physical discs:** The `lsblk` volume label is read early (before scanning) for display on the scanning screen. `bdmt_*.xml` requires a mount point, which isn't available until `ensure_mounted()` runs during the scan. The display label (`disc.label`) should be *upgraded* after scanning if `bdmt_*.xml` provides a richer title — this affects TMDb search pre-population and filename generation but not the initial scanning screen display. However, the parsed `disc.label_info` (`LabelInfo` with season/disc numbers extracted via `parse_volume_label()`) should remain derived from the original volume label, since `bdmt_*.xml` titles use a different format that won't match the existing regex patterns. For folder input, `bdmt_*.xml` is available immediately since the path is the mount point.

**New dependency:** `quick-xml` (event reader, no serde features needed).

### AACS Preflight and Disc-Specific Operation Bypass

Operations skipped for `InputSource::Folder`:

| Operation | Why skip |
|-----------|----------|
| `aacs::preflight()` | Content already decrypted |
| `disc::set_max_speed()` | No physical drive |
| `disc::mount_disc()` / `unmount_disc()` | Nothing to mount |
| `disc::eject()` | Nothing to eject |
| `lock_device()` (flock) | No block device to lock |

Operations unchanged (work for both variants):

| Operation | Why unchanged |
|-----------|---------------|
| `scan_playlists_with_progress()` | `bluray:{path}` works for folders via libbluray |
| `ensure_mounted()` | Already returns `(path, false)` for directories with `BDMV/` |
| Chapter extraction, MPLS reading | Filesystem-based, source-agnostic |
| Index parsing (`parse_title_order`) | Filesystem-based |
| Remuxing (`write_remux`) | `bluray:{path}` input URL works for both |
| Verification, hooks, history | All downstream of remux |

**Scan subprocess isolation (Linux):** The forked subprocess used to avoid kernel D-state hangs is unnecessary for folder input but harmless. Left as-is for the initial implementation to keep the code path unified. A future optimization could bypass the fork for folder input to avoid the unnecessary subprocess + 120s timeout overhead.

**Note on `start_disc_scan()` in `session.rs`:** This function contains a polling loop that calls `get_volume_label()` repeatedly until a non-empty label is returned, plus calls to `set_max_speed()`. For folder input, `get_volume_label()` would fail or return empty (it uses `lsblk`/`diskutil` which don't work on directories), causing the loop to spin forever. The scan thread must be entirely bypassed for folder input — either by branching within `start_disc_scan()` to skip the polling loop and construct the label from `parse_bdmt_title()` or folder basename, or by providing a separate `start_folder_scan()` entry point.

### CLI Integration

- `-d` accepts either a block device or a directory path. No new flag needed for single-folder input.
- `-d` help text updated: `"Blu-ray device or BDMV folder path"`
- `--batch` with folder input errors: "batch mode is not supported with folder input (use --batch-dir for multi-volume folders in a future release)"
- `--check` currently runs before device resolution, so it won't know about folder input. If `-d` is provided alongside `--check`, resolve the `InputSource` first and note that AACS checks are skipped for folders. If no `-d`, `--check` runs as-is.

### TUI Integration

- When `-d` points to a folder, skip DriveMonitor polling — go straight to scanning.
- The scan thread must bypass the `get_volume_label()` polling loop and `set_max_speed()` call (see note above).
- Label comes from `parse_bdmt_title()` or folder basename, available immediately.
- Scanning screen works identically, just resolves faster (no AACS handshake).
- Full wizard flow unchanged: TMDb search, season, playlist manager, confirm, ripping.
- Rescan (`Ctrl+R`) re-scans the same folder.
- Done screen: skip eject, skip "insert next disc" prompt.
- `--multi-drive` disabled for folder input.

### Multi-Volume Batch (Future Extension — Design Only)

Not implemented in this release. Documented here to confirm the `InputSource` design supports it.

**Envisioned UX:**
```
bluback --batch-dir /path/to/series_backup/
```

Discovers all subdirectories (one level deep) containing `BDMV/`, sorts them naturally (`vol.01` < `vol.02` < `vol.10`), processes each sequentially. Episode numbers auto-advance across volumes using existing batch continuation logic.

**Why this works with current design:**
- `InputSource::Folder` holds a single folder path
- `--batch-dir` handler would resolve a list of `InputSource::Folder` values and iterate
- `bdmt_*.xml` titles with embedded disc numbers (`Vol.1 Disc1`) feed into starting episode calculation
- No changes to the enum shape or label resolution needed

## Testing Strategy

**Unit tests:**
- `InputSource` resolution: directory with `BDMV/` → `Folder`, block device path → `Disc`, directory without `BDMV/` → error
- `parse_bdmt_title()`: valid XML with `<di:name>`, missing META directory, malformed XML, multiple language files (eng preferred), BOM handling (the XML files have UTF-8 BOM)
- Label resolution priority chain: bdmt title present vs absent, with and without `--title` override
- Batch + folder conflict detection

**Integration tests:**
- Create a minimal synthetic BDMV folder structure in test fixtures (index.bdmv + MPLS + META/DL/bdmt_eng.xml)
- Verify folder detection and label extraction end-to-end
- CLI flag conflict tests for `--batch` with folder input

**Manual testing:**
- Process the Shangri-La Frontier backup with `bluback -d /path/to/vol.01/BD_VIDEO`
- Verify TMDb search works with bdmt_*.xml title
- Verify chapters, stream selection, and metadata embedding work correctly
- Test with `--title` override
- Test TUI and CLI modes

## Files to Modify

| File | Change |
|------|--------|
| `src/types.rs` | Add `InputSource` enum with methods |
| `src/disc.rs` | `parse_bdmt_title()`, update `get_volume_label()` flow, skip disc ops for folders |
| `src/main.rs` | Resolve `InputSource` early, skip AACS preflight and `try_lock_device()` for folders, pass through pipeline |
| `src/cli.rs` | Accept `InputSource`, skip disc-specific prompts |
| `src/session.rs` | Accept `InputSource` in session creation, skip DriveMonitor for folders, bypass `start_disc_scan()` polling loop + `set_max_speed()` for folders |
| `src/aacs.rs` | Make `preflight()` conditional on input type |
| `src/tui/mod.rs` | Skip eject/DriveMonitor for folder input |
| `src/tui/dashboard.rs` | Skip eject on done screen for folders |
| `src/tui/wizard.rs` | No changes expected (works via `.bluray_path()`) |
| `src/check.rs` | Note folder input in `--check` output |
| `Cargo.toml` | Add `quick-xml` dependency |
| `Cargo.lock` | Updated automatically by `quick-xml` addition |

## Known Limitations

- **History duplicate detection across input types:** A disc ripped via physical drive and the same disc processed via folder backup may not match in history's duplicate detection (different volume labels). The TMDb ID secondary check mitigates this; no special handling needed.

## Out of Scope

- `--batch-dir` multi-volume batch mode (future extension)
- ISO file input (`.iso` files)
- Encrypted folder input (would require AACS on folder — not a real scenario)
- DVD folder input (separate feature in v0.12 DVD milestone)
