# MPLS Refactor: Unified Parsing + On-Disc Clip Sizes

## Problem

The dashboard's "Size" column for pending/ripping jobs shows an estimate computed from `bitrate_bps / 8 * seconds`. This is inaccurate — Blu-ray bitrates vary across playlists and the probed bitrate may not reflect the actual remux output. Meanwhile, the disc filesystem has the real data: MPLS playlists reference `.m2ts` clip files in `BDMV/STREAM/`, and we can `stat` those files for exact on-disc sizes.

Additionally, `chapters.rs` parses MPLS files redundantly — once during the metadata discovery mount window (for chapter counts) and again during ripping (for chapter extraction in `prepare_remux_options`). The first pass should extract everything we need.

## Design

### New struct: `MplsInfo`

```rust
pub struct MplsInfo {
    pub chapters: Vec<ChapterMark>,
    pub clip_size: u64,
}
```

Carries everything extracted from a single MPLS parse. `clip_size` is the sum of `stat()` sizes for all referenced `.m2ts` clip files.

### New function: `parse_mpls_info`

```rust
pub fn parse_mpls_info(mount_point: &Path, playlist_num: &str) -> Option<MplsInfo>
```

Single-parse function that:
1. Opens `BDMV/PLAYLIST/{playlist_num}.mpls`
2. Parses via `mpls::Mpls::from(file)`
3. Extracts chapter marks from entry-point marks (existing logic)
4. Iterates `play_list.play_items`, reads each `play_item.clip.file_name`, stats `BDMV/STREAM/{file_name}.m2ts`, sums sizes
5. Logs `debug!` for any missing clip files (catches UHD/non-standard layouts)
6. Returns `MplsInfo { chapters, clip_size }`

Only the primary clip per PlayItem is used (not `angles`). Multi-angle discs have alternate clips per angle, but bluback always remuxes angle 0. A code comment documents this.

### Refactored `extract_chapters`

Thin wrapper:

```rust
pub fn extract_chapters(mount_point: &Path, playlist_num: &str) -> Option<Vec<ChapterMark>> {
    parse_mpls_info(mount_point, playlist_num).map(|info| info.chapters)
}
```

Signature unchanged. Callers (`prepare_remux_options`) unaffected.

### New bulk function: `collect_mpls_info`

```rust
pub fn collect_mpls_info(
    mount_point: &Path,
    playlist_nums: &[&str],
) -> HashMap<String, MplsInfo>
```

Replaces `count_chapters_for_playlists`. Iterates all playlist numbers, calls `parse_mpls_info` for each, returns the full map.

Named `collect_mpls_info` (not `scan_playlists_from_disc`) to avoid confusion with `media::scan_playlists_with_progress()`.

### `count_chapters_for_playlists` removed

All 3 call sites switch to `collect_mpls_info`. Chapter counts are derived from `info.chapters.len()` at the call site.

### DiscState changes

```rust
pub struct DiscState {
    // ... existing fields ...
    pub chapter_counts: HashMap<String, usize>,
    pub clip_sizes: HashMap<String, u64>,  // NEW
}
```

Separate field alongside `chapter_counts`. Both populated from the same `collect_mpls_info` result at the 3 call sites:

1. **TUI** `session.rs` `handle_background_result` (~line 738)
2. **CLI** `cli.rs` `list_playlists` (~line 109)
3. **CLI** `cli.rs` `run_interactive` (~line 320)

Pattern at each site:

```rust
let mpls_info = crate::chapters::collect_mpls_info(mount_path, &nums);
self.disc.chapter_counts = mpls_info.iter()
    .map(|(k, v)| (k.clone(), v.chapters.len()))
    .collect();
self.disc.clip_sizes = mpls_info.iter()
    .map(|(k, v)| (k.clone(), v.clip_size))
    .collect();
```

### RipJob estimated_size: real sizes first

In `tui/wizard.rs` where `RipJob`s are constructed (~line 1585), look up `clip_sizes` first:

```rust
let estimated_size = session
    .disc
    .clip_sizes
    .get(&pl.num)
    .copied()
    .filter(|&sz| sz > 0)
    .unwrap_or_else(|| {
        // Fallback: bitrate-based estimate
        const FALLBACK_BYTERATE: u64 = 2_500_000;
        let byterate = session.wizard.media_infos
            .get(&pl.num)
            .map(|info| info.bitrate_bps / 8)
            .filter(|&br| br > 0)
            .unwrap_or(FALLBACK_BYTERATE);
        pl.seconds as u64 * byterate
    });
```

No view snapshot propagation needed — `clip_sizes` is read directly from `session.disc` at job construction time.

## Call sites unchanged

- `workflow::prepare_remux_options` — still calls `extract_chapters` (thin wrapper), no change needed
- Dashboard rendering — already uses `job.estimated_size`, no change needed
- CLI rip path — does not use `RipJob` or `estimated_size`

## Known limitations

**Partial clips**: If a playlist uses only a portion of a clip via `in_time`/`out_time` (e.g., seamless branching in movies), the on-disc file size overstates the output MKV size. Proportional scaling would require parsing `.clpi` files for clip total duration, which the `mpls` crate doesn't support. This is uncommon for episodic TV (bluback's primary use case) and the `~` prefix already communicates the value is approximate. Accepted as-is.

**Multi-angle**: Only the primary clip (angle 0) is statted. This matches bluback's remux behavior.

**UHD/SSIF**: Some UHD discs store streams in `BDMV/STREAM/SSIF/`. If the standard path fails, `clip_size` will be 0 and the bitrate fallback kicks in. The `debug!` log helps diagnose these cases.

## Files changed

| File | Change |
|------|--------|
| `src/chapters.rs` | Add `MplsInfo`, `parse_mpls_info`, `collect_mpls_info`. Refactor `extract_chapters` as wrapper. Remove `count_chapters_for_playlists`. Add tests. |
| `src/tui/mod.rs` | Add `clip_sizes: HashMap<String, u64>` to `DiscState` |
| `src/session.rs` | Switch TUI call site from `count_chapters_for_playlists` to `collect_mpls_info`, populate both `chapter_counts` and `clip_sizes` |
| `src/cli.rs` | Switch both CLI call sites similarly |
| `src/tui/wizard.rs` | Use `clip_sizes` lookup with bitrate fallback for `RipJob.estimated_size` |

## Testing

- `parse_mpls_info` with missing path → `None`
- `parse_mpls_info` with missing MPLS file → `None`
- `collect_mpls_info` bulk collection (delegates correctly)
- `extract_chapters` wrapper returns only chapters
- Missing `BDMV/STREAM/` directory → chapters returned, `clip_size` = 0
- Existing tests for chapter extraction continue to pass
