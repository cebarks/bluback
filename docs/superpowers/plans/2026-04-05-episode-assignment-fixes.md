# Episode Assignment Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix episode-to-playlist assignment bugs: wrong ordering, no reassignment when specials change, integer truncation in multi-episode detection, and incomplete detection acceptance.

**Architecture:** New `src/index.rs` module parses `BDMV/index.bdmv` to get authoritative title→playlist ordering. Playlists are reordered after scan (before episode assignment). A new `reassign_regular_episodes` helper recalculates non-special assignments whenever specials change. The multi-episode heuristic switches from integer truncation to f64 rounding. CLI mode gets a `TmdbContext.episodes` field so it can reassign after specials are determined.

**Tech Stack:** Rust, `std::io::Read` for binary parsing, existing `mpls` crate pattern for reading from mounted disc.

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src/index.rs` | Create | Parse `index.bdmv`, return title→playlist ordering |
| `src/main.rs` | Modify | Add `mod index;` declaration |
| `src/util.rs` | Modify | Fix multi-episode rounding in `assign_episodes` |
| `src/tui/wizard.rs` | Modify | Add `reassign_regular_episodes`, wire into `s`/`r`/`R`/`A`/detection handlers |
| `src/session.rs` | Modify | Reorder playlists after disc scan using title index |
| `src/cli.rs` | Modify | Add `TmdbContext.episodes`/`start_episode`, reorder playlists, reassign after specials |

---

### Task 1: Write index.bdmv parser

**Files:**
- Create: `src/index.rs`
- Modify: `src/main.rs` (add `mod index;`)

The index.bdmv binary format (big-endian throughout):

**Header (40 bytes):**
- Offset 0x00: magic `"INDX"` (4 bytes)
- Offset 0x04: version (4 bytes, e.g., `"0200"`)
- Offset 0x08: `indexes_start` (u32 BE) — absolute file offset to data section
- Offset 0x0C: `extension_data_start` (u32 BE) — 0 if none
- Offset 0x10: reserved (24 bytes)

**At `indexes_start` offset:**
- AppInfoBDMV: 4-byte length (u32 BE) + `length` bytes of data
- Indexes section: 4-byte length (u32 BE), then:
  - First Playback entry (12 bytes) — skip
  - Top Menu entry (12 bytes) — skip
  - `num_titles` (u16 BE)
  - Title entries (12 bytes each)

**Each title entry (12 bytes):**
- Bytes 0–3 (u32 BE): bits 31–30 = `object_type` (1=HDMV, 2=BD-J), rest = flags/padding
- Bytes 4–11 (type-specific):
  - HDMV: bytes 6–7 = `id_ref` (u16 BE) = MPLS playlist number
  - BD-J: skip (no simple playlist reference)

- [ ] **Step 1: Write tests for index.bdmv parsing**

In `src/index.rs`, add the module with tests:

```rust
use std::path::Path;

/// Parse index.bdmv and return MPLS playlist numbers in title order.
///
/// Reads the disc's `BDMV/index.bdmv` file and extracts the title table.
/// Each HDMV title entry references an MPLS playlist by number.
/// Returns playlist numbers (zero-padded to 5 digits) in the disc author's
/// intended title order, or `None` if the file can't be read/parsed.
pub fn parse_title_order(mount_point: &Path) -> Option<Vec<String>> {
    todo!()
}

/// Reorder a playlist vec using title order from index.bdmv.
///
/// Playlists referenced in `title_order` are moved to the front in that order.
/// Remaining playlists are appended in MPLS number order.
/// If `title_order` is `None`, falls back to sorting all playlists by MPLS number.
pub fn reorder_playlists(playlists: &mut Vec<crate::types::Playlist>, title_order: Option<&[String]>) {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Playlist;

    fn make_pl(num: &str, secs: u32) -> Playlist {
        Playlist {
            num: num.into(),
            duration: String::new(),
            seconds: secs,
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
        }
    }

    // --- reorder_playlists tests ---

    #[test]
    fn test_reorder_with_title_order() {
        let mut pls = vec![
            make_pl("00800", 2640),
            make_pl("00801", 2640),
            make_pl("00802", 2640),
        ];
        let order = vec!["00802".into(), "00800".into(), "00801".into()];
        reorder_playlists(&mut pls, Some(&order));
        assert_eq!(pls[0].num, "00802");
        assert_eq!(pls[1].num, "00800");
        assert_eq!(pls[2].num, "00801");
    }

    #[test]
    fn test_reorder_unindexed_appended_sorted() {
        let mut pls = vec![
            make_pl("00900", 2640),
            make_pl("00800", 2640),
            make_pl("00801", 2640),
        ];
        // Title index only references 00801 and 00800
        let order = vec!["00801".into(), "00800".into()];
        reorder_playlists(&mut pls, Some(&order));
        assert_eq!(pls[0].num, "00801"); // title order
        assert_eq!(pls[1].num, "00800"); // title order
        assert_eq!(pls[2].num, "00900"); // unindexed, appended
    }

    #[test]
    fn test_reorder_fallback_sorts_by_num() {
        let mut pls = vec![
            make_pl("00802", 2640),
            make_pl("00800", 2640),
            make_pl("00801", 2640),
        ];
        reorder_playlists(&mut pls, None);
        assert_eq!(pls[0].num, "00800");
        assert_eq!(pls[1].num, "00801");
        assert_eq!(pls[2].num, "00802");
    }

    #[test]
    fn test_reorder_deduplicates_title_entries() {
        let mut pls = vec![
            make_pl("00001", 2640),
            make_pl("00002", 2640),
        ];
        // Same playlist referenced by multiple titles
        let order = vec!["00002".into(), "00001".into(), "00002".into()];
        reorder_playlists(&mut pls, Some(&order));
        assert_eq!(pls[0].num, "00002");
        assert_eq!(pls[1].num, "00001");
    }

    #[test]
    fn test_reorder_title_order_references_missing_playlist() {
        let mut pls = vec![
            make_pl("00001", 2640),
            make_pl("00002", 2640),
        ];
        // Title order references playlist not in scan results
        let order = vec!["00099".into(), "00002".into(), "00001".into()];
        reorder_playlists(&mut pls, Some(&order));
        assert_eq!(pls[0].num, "00002");
        assert_eq!(pls[1].num, "00001");
    }

    // --- parse_title_order tests (synthetic binary) ---

    fn build_index_bdmv(titles: &[(u8, u16)]) -> Vec<u8> {
        // Build a minimal valid index.bdmv with the given titles.
        // Each title is (object_type, id_ref).
        let mut buf = Vec::new();

        // Header (40 bytes)
        buf.extend_from_slice(b"INDX");     // magic
        buf.extend_from_slice(b"0200");     // version
        let indexes_start: u32 = 40;
        buf.extend_from_slice(&indexes_start.to_be_bytes()); // indexes_start
        buf.extend_from_slice(&0u32.to_be_bytes());          // extension_data_start
        buf.extend_from_slice(&[0u8; 24]);                   // reserved

        // AppInfoBDMV section: length + minimal data
        let app_info_len: u32 = 34;
        buf.extend_from_slice(&app_info_len.to_be_bytes());
        buf.extend_from_slice(&vec![0u8; app_info_len as usize]);

        // Indexes section
        let first_play_top_menu = 12 + 12; // 2 entries, 12 bytes each
        let titles_data = 2 + titles.len() * 12; // u16 count + entries
        let indexes_len = first_play_top_menu + titles_data;
        buf.extend_from_slice(&(indexes_len as u32).to_be_bytes());

        // First Playback (12 bytes, all zeros = no first play)
        buf.extend_from_slice(&[0u8; 12]);
        // Top Menu (12 bytes, all zeros = no top menu)
        buf.extend_from_slice(&[0u8; 12]);

        // Number of titles
        buf.extend_from_slice(&(titles.len() as u16).to_be_bytes());

        // Title entries (12 bytes each)
        for &(obj_type, id_ref) in titles {
            // Bytes 0-3: object_type in bits 31-30
            let word0: u32 = (obj_type as u32) << 30;
            buf.extend_from_slice(&word0.to_be_bytes());
            // Bytes 4-5: playback_type + padding (zeros)
            buf.extend_from_slice(&[0u8; 2]);
            // Bytes 6-7: id_ref (MPLS number)
            buf.extend_from_slice(&id_ref.to_be_bytes());
            // Bytes 8-11: padding
            buf.extend_from_slice(&[0u8; 4]);
        }

        buf
    }

    #[test]
    fn test_parse_synthetic_index() {
        let dir = std::env::temp_dir().join("bluback_test_index");
        let bdmv_dir = dir.join("BDMV");
        std::fs::create_dir_all(&bdmv_dir).unwrap();

        let data = build_index_bdmv(&[
            (1, 800), // HDMV title → 00800.mpls
            (1, 801), // HDMV title → 00801.mpls
            (1, 802), // HDMV title → 00802.mpls
        ]);
        std::fs::write(bdmv_dir.join("index.bdmv"), &data).unwrap();

        let result = parse_title_order(&dir).unwrap();
        assert_eq!(result, vec!["00800", "00801", "00802"]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_mixed_hdmv_bdj() {
        let dir = std::env::temp_dir().join("bluback_test_index_mixed");
        let bdmv_dir = dir.join("BDMV");
        std::fs::create_dir_all(&bdmv_dir).unwrap();

        let data = build_index_bdmv(&[
            (1, 42),  // HDMV
            (2, 0),   // BD-J (id_ref ignored)
            (1, 43),  // HDMV
        ]);
        std::fs::write(bdmv_dir.join("index.bdmv"), &data).unwrap();

        let result = parse_title_order(&dir).unwrap();
        // BD-J titles are skipped
        assert_eq!(result, vec!["00042", "00043"]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_missing_file_returns_none() {
        let dir = std::env::temp_dir().join("bluback_test_index_missing");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(parse_title_order(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_bad_magic_returns_none() {
        let dir = std::env::temp_dir().join("bluback_test_index_bad");
        let bdmv_dir = dir.join("BDMV");
        std::fs::create_dir_all(&bdmv_dir).unwrap();
        std::fs::write(bdmv_dir.join("index.bdmv"), b"NOT_INDX_FILE").unwrap();
        assert!(parse_title_order(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_truncated_returns_none() {
        let dir = std::env::temp_dir().join("bluback_test_index_trunc");
        let bdmv_dir = dir.join("BDMV");
        std::fs::create_dir_all(&bdmv_dir).unwrap();
        // Valid magic but truncated before indexes_start
        std::fs::write(bdmv_dir.join("index.bdmv"), b"INDX0200").unwrap();
        assert!(parse_title_order(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib index -- --test-threads=1`

Expected: compilation error (`todo!()` panics or missing module)

- [ ] **Step 3: Add `mod index;` to main.rs**

In `src/main.rs`, add the module declaration alongside the other modules:

```rust
mod index;
```

- [ ] **Step 4: Implement `parse_title_order`**

```rust
use std::path::Path;

/// Parse index.bdmv and return MPLS playlist numbers in title order.
///
/// Reads the disc's `BDMV/index.bdmv` file and extracts the title table.
/// Each HDMV title entry references an MPLS playlist by number.
/// Returns playlist numbers (zero-padded to 5 digits) in the disc author's
/// intended title order, or `None` if the file can't be read/parsed.
pub fn parse_title_order(mount_point: &Path) -> Option<Vec<String>> {
    let index_path = mount_point.join("BDMV").join("index.bdmv");
    let data = std::fs::read(&index_path).ok()?;

    // Validate header: need at least 40 bytes, magic = "INDX"
    if data.len() < 40 || &data[0..4] != b"INDX" {
        log::debug!("index.bdmv: invalid header or too short");
        return None;
    }

    // Read indexes_start offset (u32 BE at offset 8)
    let indexes_start = u32::from_be_bytes(data[8..12].try_into().ok()?) as usize;
    if indexes_start >= data.len() {
        log::debug!("index.bdmv: indexes_start ({}) beyond file length", indexes_start);
        return None;
    }

    // Skip AppInfoBDMV section: 4-byte length + data
    let pos = indexes_start;
    if pos + 4 > data.len() {
        return None;
    }
    let app_info_len = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
    let pos = pos + 4 + app_info_len;

    // Indexes section: 4-byte length
    if pos + 4 > data.len() {
        return None;
    }
    let _indexes_len = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
    let pos = pos + 4;

    // Skip First Playback (12 bytes) + Top Menu (12 bytes)
    let pos = pos + 24;
    if pos + 2 > data.len() {
        return None;
    }

    // Read number of titles (u16 BE)
    let num_titles = u16::from_be_bytes(data[pos..pos + 2].try_into().ok()?) as usize;
    let pos = pos + 2;

    // Parse title entries (12 bytes each)
    let mut playlist_nums = Vec::new();
    for i in 0..num_titles {
        let entry_start = pos + i * 12;
        if entry_start + 12 > data.len() {
            log::debug!("index.bdmv: truncated at title entry {}", i);
            break;
        }

        // Byte 0-3: object_type in bits 31-30
        let word0 = u32::from_be_bytes(data[entry_start..entry_start + 4].try_into().ok()?);
        let object_type = (word0 >> 30) & 0x03;

        if object_type == 1 {
            // HDMV: id_ref at bytes 6-7 (u16 BE)
            let id_ref =
                u16::from_be_bytes(data[entry_start + 6..entry_start + 8].try_into().ok()?);
            playlist_nums.push(format!("{:05}", id_ref));
        }
        // BD-J (object_type == 2) and others: skip
    }

    if playlist_nums.is_empty() {
        None
    } else {
        Some(playlist_nums)
    }
}

/// Reorder a playlist vec using title order from index.bdmv.
///
/// Playlists referenced in `title_order` are moved to the front in that order.
/// Remaining playlists are appended in MPLS number order.
/// If `title_order` is `None`, falls back to sorting all playlists by MPLS number.
pub fn reorder_playlists(
    playlists: &mut Vec<crate::types::Playlist>,
    title_order: Option<&[String]>,
) {
    match title_order {
        Some(order) => {
            // Build position map from title order, first occurrence wins
            let mut pos_map: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            for (i, num) in order.iter().enumerate() {
                pos_map.entry(num.as_str()).or_insert(i);
            }

            playlists.sort_by(|a, b| {
                match (pos_map.get(a.num.as_str()), pos_map.get(b.num.as_str())) {
                    (Some(pa), Some(pb)) => pa.cmp(pb),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => a.num.cmp(&b.num),
                }
            });
        }
        None => {
            playlists.sort_by(|a, b| a.num.cmp(&b.num));
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib index`

Expected: all 9 tests pass

- [ ] **Step 6: Run full test suite + clippy + fmt**

Run: `cargo test && cargo clippy -- -D warnings && rustup run stable cargo fmt --check`

Expected: all pass, no warnings, no format issues

- [ ] **Step 7: Commit**

```
feat: add index.bdmv parser for title-order playlist reordering
```

---

### Task 2: Fix multi-episode integer truncation in assign_episodes

**Files:**
- Modify: `src/util.rs:206-215` (`assign_episodes` ep_count calculation)

The current code uses integer division which truncates: a 1.5x-median playlist
counts as 1 episode instead of 2. Replace with f64 rounding and remove the
separate 1.5x threshold gate (rounding handles it naturally).

- [ ] **Step 1: Update existing tests for new rounding behavior**

In `src/util.rs`, the existing test `test_assign_double_episode` (line ~960) already passes with rounding since the double playlist is exactly 2x median. But we need a new test for the 1.5x edge case that was previously broken:

```rust
#[test]
fn test_assign_episodes_rounds_instead_of_truncating() {
    // A playlist at ~1.5x median should count as 2 episodes (rounded),
    // not 1 episode (truncated integer division).
    let playlists = vec![
        Playlist {
            num: "00001".into(),
            duration: "0:44:00".into(),
            seconds: 2640,
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
        },
        Playlist {
            num: "00002".into(),
            duration: "0:44:00".into(),
            seconds: 2640,
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
        },
        Playlist {
            num: "00003".into(),
            duration: "1:06:00".into(),
            seconds: 3960, // 1.5x median (2640)
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
        },
    ];
    let episodes: Vec<Episode> = (1..=4)
        .map(|n| Episode {
            episode_number: n,
            name: format!("Episode {}", n),
            runtime: Some(44),
        })
        .collect();
    let result = assign_episodes(&playlists, &episodes, 1);
    assert_eq!(result["00001"].len(), 1);
    assert_eq!(result["00001"][0].episode_number, 1);
    assert_eq!(result["00002"].len(), 1);
    assert_eq!(result["00002"][0].episode_number, 2);
    // 1.5x median rounds to 2 episodes
    assert_eq!(result["00003"].len(), 2);
    assert_eq!(result["00003"][0].episode_number, 3);
    assert_eq!(result["00003"][1].episode_number, 4);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib test_assign_episodes_rounds_instead_of_truncating`

Expected: FAIL — playlist 00003 gets 1 episode instead of 2

- [ ] **Step 3: Fix the ep_count calculation**

In `src/util.rs`, replace the ep_count block (lines 207-215):

Old:
```rust
        let ep_count = if playlists.len() > 1
            && median_secs > 0
            && pl.seconds as f64 >= median_secs as f64 * 1.5
        {
            (pl.seconds / median_secs).max(1)
        } else {
            1
        };
```

New:
```rust
        let ep_count = if playlists.len() > 1 && median_secs > 0 {
            (pl.seconds as f64 / median_secs as f64).round() as u32
        } else {
            1
        }
        .max(1);
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib assign`

Expected: all assign_episodes tests pass, including the new rounding test

- [ ] **Step 5: Run full test suite + clippy + fmt**

Run: `cargo test && cargo clippy -- -D warnings && rustup run stable cargo fmt --check`

Expected: all pass

- [ ] **Step 6: Commit**

```
fix: use f64 rounding for multi-episode detection instead of integer truncation
```

---

### Task 3: Add reassign_regular_episodes helper

**Files:**
- Modify: `src/tui/wizard.rs` (add function near `accept_detection_suggestions`, ~line 889)

This function recalculates regular (non-special) episode assignments. It preserves
special assignments untouched. Called whenever the set of specials changes.

- [ ] **Step 1: Write the function**

Add this function in `src/tui/wizard.rs`, near `accept_detection_suggestions` (before the `// --- Session variants of input handlers ---` comment):

```rust
/// Recalculate regular (non-special) episode assignments.
///
/// Collects episode-length playlists that are not marked as specials,
/// re-runs `assign_episodes` on just those playlists, and merges the
/// result with existing special assignments. This ensures episode numbers
/// shift correctly when specials are added or removed.
fn reassign_regular_episodes(session: &mut crate::session::DriveSession) {
    let non_special_pl: Vec<crate::types::Playlist> = session
        .disc
        .episodes_pl
        .iter()
        .filter(|pl| !session.wizard.specials.contains(&pl.num))
        .cloned()
        .collect();

    let disc_num = session.disc.label_info.as_ref().map(|l| l.disc);
    let start_ep = session
        .wizard
        .start_episode
        .unwrap_or_else(|| crate::util::guess_start_episode(disc_num, non_special_pl.len()));

    let new_assignments =
        crate::util::assign_episodes(&non_special_pl, &session.tmdb.episodes, start_ep);

    // Remove all non-special assignments, keeping special ones intact
    session
        .wizard
        .episode_assignments
        .retain(|k, _| session.wizard.specials.contains(k));

    // Merge in the recalculated regular assignments
    session.wizard.episode_assignments.extend(new_assignments);
}
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test && cargo clippy -- -D warnings && rustup run stable cargo fmt --check`

Expected: all pass (function is defined but not yet called)

- [ ] **Step 3: Commit**

```
feat: add reassign_regular_episodes helper for episode shift on special changes
```

---

### Task 4: Wire reassignment into TUI handlers

**Files:**
- Modify: `src/tui/wizard.rs` — `s` key handler (~line 1349), `r` key handler (~line 1386), `R` key handler (~line 1393), `accept_detection_suggestions` (~line 889), `run_detection_if_enabled` (~line 823)

- [ ] **Step 1: Wire into `s` key (toggle special)**

In `handle_playlist_manager_input_session`, the `KeyCode::Char('s')` handler (around line 1349), add `reassign_regular_episodes(session);` at the end of the handler block, after the `if/else` for toggling special status. The reassignment handles both marking (specials gained a member) and unmarking (specials lost a member):

Old (end of `'s'` handler block):
```rust
                    if let Some(sel) = session.wizard.playlist_selected.get_mut(real_idx) {
                        *sel = true;
                    }
                }
            }
        }
```

New:
```rust
                    if let Some(sel) = session.wizard.playlist_selected.get_mut(real_idx) {
                        *sel = true;
                    }
                }
                reassign_regular_episodes(session);
            }
        }
```

- [ ] **Step 2: Wire into `R` key (reset all)**

The `R` handler clears all assignments and specials. After clearing, re-run the full assignment to restore defaults.

Old:
```rust
        KeyCode::Char('R') => {
            session.wizard.episode_assignments.clear();
            session.wizard.specials.clear();
        }
```

New:
```rust
        KeyCode::Char('R') => {
            session.wizard.specials.clear();
            reassign_regular_episodes(session);
        }
```

Note: we don't need to `clear()` assignments first — `reassign_regular_episodes` already retains only specials (of which there are now none) and replaces the rest.

- [ ] **Step 3: Wire into `accept_detection_suggestions`**

Add `reassign_regular_episodes(session);` at the end of `accept_detection_suggestions`, after the loop that marks specials:

Old (end of function):
```rust
            }
        }
    }
}
```

New:
```rust
            }
        }
    }
    reassign_regular_episodes(session);
}
```

- [ ] **Step 4: Wire into `run_detection_if_enabled`**

The function auto-marks high-confidence specials. After marking, reassign if any specials were added. Find the loop that marks specials (around line 857-872) and add reassignment after it.

At the end of the `if session.auto_detect` block, after the detection loop, add:

```rust
        // Reassign regular episodes to account for auto-marked specials
        if !session.wizard.specials.is_empty() {
            reassign_regular_episodes(session);
        }
```

Place this after the `for det in ...` loop that marks high-confidence specials, but still inside the `if session.auto_detect` block.

- [ ] **Step 5: No change needed for `r` key (reset single)**

The `r` key removes an individual assignment and removes it from specials. After `r`, we do NOT reassign — the user is explicitly clearing one row. If they want to recalculate, they can press `R` to reset all. This keeps the behavior predictable: manual edits (`e`, `r`) don't trigger automatic shifts.

- [ ] **Step 6: Run full test suite + clippy + fmt**

Run: `cargo test && cargo clippy -- -D warnings && rustup run stable cargo fmt --check`

Expected: all pass

- [ ] **Step 7: Commit**

```
fix: reassign episodes when specials change in TUI playlist manager
```

---

### Task 5: Integrate title ordering into TUI session

**Files:**
- Modify: `src/session.rs` — `BackgroundResult::DiscScan` handler (~line 706)

The title index should be read during the chapter-extraction mount (which already
happens after disc scan), and used to reorder both `disc.playlists` and
`disc.episodes_pl` before any episode assignment occurs.

- [ ] **Step 1: Add title ordering to the DiscScan handler**

In `src/session.rs`, in the `BackgroundResult::DiscScan(Ok(...))` handler, after chapter_counts are extracted and before the mount is released, add the title order parsing. Then after unmount, reorder both playlist vecs.

Find the block (around line 739-760):

```rust
                // Extract chapter counts from MPLS files
                let device_str = self.device.to_string_lossy().to_string();
                match crate::disc::ensure_mounted(&device_str) {
                    Ok((mount, did_mount)) => {
                        let nums: Vec<&str> = self
                            .disc
                            .playlists
                            .iter()
                            .map(|pl| pl.num.as_str())
                            .collect();
                        self.disc.chapter_counts = crate::chapters::count_chapters_for_playlists(
                            std::path::Path::new(&mount),
                            &nums,
                        );
                        if did_mount {
                            let _ = crate::disc::unmount_disc(&device_str);
                        }
                    }
                    Err(_) => {
                        self.disc.chapter_counts.clear();
                    }
                }
```

Replace with:

```rust
                // Extract chapter counts and title order from mounted disc
                let device_str = self.device.to_string_lossy().to_string();
                let title_order = match crate::disc::ensure_mounted(&device_str) {
                    Ok((mount, did_mount)) => {
                        let mount_path = std::path::Path::new(&mount);
                        let nums: Vec<&str> = self
                            .disc
                            .playlists
                            .iter()
                            .map(|pl| pl.num.as_str())
                            .collect();
                        self.disc.chapter_counts =
                            crate::chapters::count_chapters_for_playlists(mount_path, &nums);
                        let order = crate::index::parse_title_order(mount_path);
                        if did_mount {
                            let _ = crate::disc::unmount_disc(&device_str);
                        }
                        order
                    }
                    Err(_) => {
                        self.disc.chapter_counts.clear();
                        None
                    }
                };

                // Reorder playlists by title index (or MPLS number as fallback)
                crate::index::reorder_playlists(
                    &mut self.disc.playlists,
                    title_order.as_deref(),
                );
                crate::index::reorder_playlists(
                    &mut self.disc.episodes_pl,
                    title_order.as_deref(),
                );
```

- [ ] **Step 2: Rebuild `playlist_selected` after reorder**

The `playlist_selected` vec is built from the playlist order (one bool per playlist). After reordering, it was already built from the pre-reorder playlist list. Since the reordering happens before `playlist_selected` is initialized (check this), this should be fine. But verify: look at where `playlist_selected` is set relative to the DiscScan handler.

Search for `playlist_selected` in the DiscScan handler to confirm it's set AFTER our reorder point. If it's set before (at line ~735-737), move the reorder to before that point.

The playlist_selected is set at line ~735:
```rust
                self.wizard.playlist_selected = self
                    .disc
                    .playlists
                    .iter()
                    .map(|pl| self.disc.episodes_pl.iter().any(|ep| ep.num == pl.num))
                    .collect();
```

This is BEFORE the chapter extraction block. So we need to either:
- Move the reorder BEFORE `playlist_selected` initialization, or
- Re-derive `playlist_selected` after the reorder

The cleanest approach: do the mount + title order parsing BEFORE `playlist_selected` is computed. Move the entire chapter+index block to before line 735, or add the reorder before line 735 and read the index in a separate early mount.

Actually, the simplest approach: reorder `disc.playlists` and `disc.episodes_pl` immediately after they're set (lines 712-716), then the existing `playlist_selected` logic at line 735 will use the correct order. But the reorder requires a mount point, and the mount happens later.

Best approach: move the reorder to right after the mount+chapter block (where it is), and then re-derive `playlist_selected` after the reorder:

```rust
                // Re-derive playlist_selected after reorder
                self.wizard.playlist_selected = self
                    .disc
                    .playlists
                    .iter()
                    .map(|pl| self.disc.episodes_pl.iter().any(|ep| ep.num == pl.num))
                    .collect();
```

Add this line right after the two `reorder_playlists` calls.

- [ ] **Step 3: Run full test suite + clippy + fmt**

Run: `cargo test && cargo clippy -- -D warnings && rustup run stable cargo fmt --check`

Expected: all pass

- [ ] **Step 4: Commit**

```
feat: reorder TUI playlists by index.bdmv title order
```

---

### Task 6: Integrate title ordering and reassignment into CLI

**Files:**
- Modify: `src/cli.rs` — `TmdbContext` struct (~line 63), `scan_disc` (~line 549), `run` (~line 301), `list_playlists` (~line 71)

CLI needs: (1) playlist reordering after scan, and (2) episode reassignment after specials are determined.

- [ ] **Step 1: Add `episodes` and `start_episode` to `TmdbContext`**

In `src/cli.rs`, modify the `TmdbContext` struct (line 63):

Old:
```rust
struct TmdbContext {
    episode_assignments: EpisodeAssignments,
    season_num: Option<u32>,
    movie_title: Option<(String, String)>,
    show_name: Option<String>,
    date_released: Option<String>,
}
```

New:
```rust
struct TmdbContext {
    episode_assignments: EpisodeAssignments,
    episodes: Vec<Episode>,
    start_episode: u32,
    season_num: Option<u32>,
    movie_title: Option<(String, String)>,
    show_name: Option<String>,
    date_released: Option<String>,
}
```

- [ ] **Step 2: Update TmdbContext initialization in `lookup_tmdb`**

Update the initial struct creation at line 635:

Old:
```rust
    let mut ctx = TmdbContext {
        episode_assignments: HashMap::new(),
        season_num: args.season.or(label_info.as_ref().map(|l| l.season)),
        movie_title: None,
        show_name: None,
        date_released: None,
    };
```

New:
```rust
    let mut ctx = TmdbContext {
        episode_assignments: HashMap::new(),
        episodes: Vec::new(),
        start_episode: 1,
        season_num: args.season.or(label_info.as_ref().map(|l| l.season)),
        movie_title: None,
        show_name: None,
        date_released: None,
    };
```

Then, at each call to `assign_episodes` within `lookup_tmdb`, store the episodes and start_episode before assignment. There are four call sites in `lookup_tmdb`:

**Call site 1 (line ~684, --title synthetic):**
Before `ctx.episode_assignments = assign_episodes(...)`, add:
```rust
            ctx.episodes = synthetic_episodes.clone();
            ctx.start_episode = start_ep;
```

**Call site 2 (line ~747, TMDb interactive):**
Before `ctx.episode_assignments = assign_episodes(...)`, add:
```rust
                ctx.episodes = lookup.episodes.clone();
                ctx.start_episode = start_ep;
```

**Call site 3 (line ~788, re-prompt start episode):**
Before `ctx.episode_assignments = assign_episodes(...)`, add:
```rust
                                ctx.start_episode = new_start;
```
(episodes are already set from call site 2)

**Call site 4 (line ~884, headless fallback):**
Before `ctx.episode_assignments = assign_episodes(...)`, add:
```rust
        ctx.episodes = synthetic_episodes.clone();
        ctx.start_episode = start_ep;
```

- [ ] **Step 3: Add playlist reordering to `scan_disc`**

In `scan_disc` (line ~549), after the scan completes and before `episodes_pl` is built, mount the disc, parse title order, and reorder. Currently the function ends around line 616. Modify it:

After `playlists` is obtained (line ~590) and before `episodes_pl` is built (line ~595), add:

```rust
    // Reorder playlists by title index from index.bdmv
    let mut playlists = playlists; // make mutable
    {
        let device_str = device.to_string();
        match disc::ensure_mounted(&device_str) {
            Ok((mount, did_mount)) => {
                let title_order =
                    crate::index::parse_title_order(std::path::Path::new(&mount));
                crate::index::reorder_playlists(&mut playlists, title_order.as_deref());
                if did_mount {
                    let _ = disc::unmount_disc(&device_str);
                }
            }
            Err(_) => {
                crate::index::reorder_playlists(&mut playlists, None);
            }
        }
    }
```

Note: `scan_disc` returns `playlists` which flows into the chapter extraction mount in `run()`. This means the disc might get mounted twice (once here for index, once later for chapters). To avoid double-mounting, an alternative is to do the reordering in `run()` alongside the chapter extraction. Let me restructure:

Actually, looking at `run()` line 316-332, it already mounts for chapter extraction right after `scan_disc` returns. So move the reorder there instead, alongside the existing mount:

In `run()`, replace the chapter_counts block (lines 316-332):

Old:
```rust
    let chapter_counts = {
        let device_str = device.to_string();
        match disc::ensure_mounted(&device_str) {
            Ok((mount, did_mount)) => {
                let nums: Vec<&str> = all_playlists.iter().map(|pl| pl.num.as_str()).collect();
                let counts = crate::chapters::count_chapters_for_playlists(
                    std::path::Path::new(&mount),
                    &nums,
                );
                if did_mount {
                    let _ = disc::unmount_disc(&device_str);
                }
                counts
            }
            Err(_) => std::collections::HashMap::new(),
        }
    };
```

New:
```rust
    // Mount disc for chapter counts + title order
    let (chapter_counts, title_order) = {
        let device_str = device.to_string();
        match disc::ensure_mounted(&device_str) {
            Ok((mount, did_mount)) => {
                let mount_path = std::path::Path::new(&mount);
                let nums: Vec<&str> = all_playlists.iter().map(|pl| pl.num.as_str()).collect();
                let counts =
                    crate::chapters::count_chapters_for_playlists(mount_path, &nums);
                let order = crate::index::parse_title_order(mount_path);
                if did_mount {
                    let _ = disc::unmount_disc(&device_str);
                }
                (counts, order)
            }
            Err(_) => (std::collections::HashMap::new(), None),
        }
    };

    // Reorder playlists by title index (or MPLS number fallback)
    crate::index::reorder_playlists(&mut all_playlists, title_order.as_deref());
    crate::index::reorder_playlists(&mut episodes_pl, title_order.as_deref());
```

This requires changing `scan_disc` return values to be mutable. Actually, `all_playlists` and `episodes_pl` are already `let` bindings in `run()`. Change them to `let mut`:

At line 312:
```rust
    let (label, label_info, mut all_playlists, mut episodes_pl, movie_mode, probe_cache) =
        scan_disc(args, config)?;
```

- [ ] **Step 4: Add episode reassignment after specials in `run()`**

After `specials_set` is determined (around line 477), add reassignment if there are specials:

```rust
    // Reassign regular episodes after specials are determined
    if !specials_set.is_empty() && !movie_mode {
        let non_special_pl: Vec<Playlist> = episodes_pl
            .iter()
            .filter(|pl| !specials_set.contains(&pl.num))
            .cloned()
            .collect();
        tmdb_ctx.episode_assignments = assign_episodes(
            &non_special_pl,
            &tmdb_ctx.episodes,
            tmdb_ctx.start_episode,
        );
    }
```

This requires `tmdb_ctx` to be mutable. Change line 334:
```rust
    let mut tmdb_ctx = lookup_tmdb(...)?;
```

- [ ] **Step 5: Add title ordering to `list_playlists`**

In `list_playlists` (line ~71), the chapter extraction mount block (lines 107-123) should also parse the title order. Apply the same pattern:

Old:
```rust
    let chapter_counts = {
        let device_str = device.to_string();
        match disc::ensure_mounted(&device_str) {
            Ok((mount, did_mount)) => {
                let nums: Vec<&str> = playlists.iter().map(|pl| pl.num.as_str()).collect();
                let counts = crate::chapters::count_chapters_for_playlists(
                    std::path::Path::new(&mount),
                    &nums,
                );
                if did_mount {
                    let _ = disc::unmount_disc(&device_str);
                }
                counts
            }
            Err(_) => std::collections::HashMap::new(),
        }
    };
```

New:
```rust
    let (chapter_counts, title_order) = {
        let device_str = device.to_string();
        match disc::ensure_mounted(&device_str) {
            Ok((mount, did_mount)) => {
                let mount_path = std::path::Path::new(&mount);
                let nums: Vec<&str> = playlists.iter().map(|pl| pl.num.as_str()).collect();
                let counts =
                    crate::chapters::count_chapters_for_playlists(mount_path, &nums);
                let order = crate::index::parse_title_order(mount_path);
                if did_mount {
                    let _ = disc::unmount_disc(&device_str);
                }
                (counts, order)
            }
            Err(_) => (std::collections::HashMap::new(), None),
        }
    };

    // Reorder playlists by title index (or MPLS number fallback)
    crate::index::reorder_playlists(&mut playlists, title_order.as_deref());
```

Also change `playlists` to mutable at line ~79 where `scan_playlists_with_progress` returns it:
```rust
    let (mut playlists, probe_cache) = crate::media::scan_playlists_with_progress(...)
```

And update `episodes_pl` after reorder since it's built from `playlists` (line ~95):
```rust
    let episodes_pl: Vec<&Playlist> = disc::filter_episodes(&playlists, args.min_duration);
```

This should already work since `episodes_pl` is built after the reorder. Verify the line ordering is correct.

- [ ] **Step 6: Run full test suite + clippy + fmt**

Run: `cargo test && cargo clippy -- -D warnings && rustup run stable cargo fmt --check`

Expected: all pass

- [ ] **Step 7: Commit**

```
fix: reorder CLI playlists by title index and reassign after specials
```

---

### Task 7: Update CLAUDE.md architecture docs

**Files:**
- Modify: `CLAUDE.md` — Architecture section, Key Design Decisions

- [ ] **Step 1: Add index.rs to Architecture > Data Flow**

After the `chapters.rs` entry (item 13 or equivalent), add:

```
N. `index.rs` — Blu-ray `index.bdmv` parser: extracts title→playlist ordering for correct episode assignment
```

- [ ] **Step 2: Add Key Design Decision entry**

Add to the Key Design Decisions section:

```
- **Playlist ordering** — Playlists are reordered after scan using the title table from `BDMV/index.bdmv`, which reflects the disc author's intended playback order. Falls back to MPLS number sort if `index.bdmv` is unavailable or unparseable. Both `disc.playlists` and `disc.episodes_pl` are reordered before any episode assignment occurs.
- **Episode reassignment on special changes** — When playlists are marked/unmarked as specials (`s` key, auto-detection, `A` key), regular episode assignments are recalculated via `reassign_regular_episodes()`. This ensures episode numbers shift correctly instead of leaving gaps. The `r` key (reset single) does NOT trigger reassignment — manual edits are intentional.
```

- [ ] **Step 3: Commit**

```
docs: update CLAUDE.md with playlist ordering and reassignment design
```
