# Lazy Probe & Size Estimate Fix

**Date:** 2026-04-19
**Status:** Approved

## Problem

1. **Unnecessary synchronous probe** — When transitioning from PlaylistManager to Confirm, bluback synchronously probes every unprobed selected playlist (`wizard.rs:1657-1673`), freezing the UI. This probe isn't needed: the remux opens the same FFmpeg context anyway, and size estimates can come from `clip_sizes` (actual on-disc TS file sizes) instead of probed bitrate.

2. **Size estimate discrepancy** — The Confirm screen uses bitrate-based estimation (`bitrate_bps / 8 * duration`), while the RipJob builder uses `clip_sizes` with a 0.97 TS-to-MKV correction factor. These show different numbers for the same playlist.

## Solution

### 1. Remove synchronous probe block

Delete the block at `wizard.rs:1657-1673` that force-probes unprobed playlists before the Confirm transition. The on-demand probe via `t` key (track picker) remains for users who want to inspect or manually select tracks.

### 2. Split `remux()` into open + write phases

Factor `remux()` in `media/remux.rs` into two phases:

- **`open_remux_input(device, playlist) -> Result<(InputContext, MakemkvconGuard, MediaInfo, StreamInfo), MediaError>`** — Opens the FFmpeg input context, extracts `MediaInfo` and `StreamInfo` from the stream metadata, and returns all along with the AACS guard. The guard must stay alive for the duration of the remux. Both `format::Input` and `MakemkvconGuard` are `Send`, so they can be moved into the spawned rip thread.
- **`write_remux(ictx, guard, output, options, on_progress) -> Result<usize, MediaError>`** — Takes the already-open input context and guard, performs the packet copy, chapter injection, and progress reporting. Output path is passed as a separate `&Path` parameter (not part of `RemuxOptions`). Returns chapters added. The `OutputExists` check moves here (or to the caller between open and write).

A helper function `extract_media_and_stream_info(ictx: &format::Input) -> (MediaInfo, StreamInfo)` in `media/probe.rs` is factored out of the existing `probe_playlist()` logic so both `probe_playlist()` and `open_remux_input()` share it.

### 3. Resolve filenames at rip time

The rip loop (both TUI `dashboard.rs` and CLI `cli.rs`) follows this sequence per playlist:

1. Call `open_remux_input()` to get the FFmpeg context, `MediaInfo`, and `StreamInfo`
2. Resolve stream selection from `StreamInfo` (for `--tracks`/`StreamFilter` in CLI, or manual selections in TUI)
3. Use the `MediaInfo` to resolve the final filename via `playlist_filename()`
4. Re-run `check_overwrite()` against the final resolved filename
5. Call `write_remux()` with the open context, resolved output path, and options

**Confirm screen filenames:** Shown with whatever info is available. If the user's filename format includes media template variables (`{resolution}`, `{codec}`, etc.) and the playlist hasn't been probed via `t`, those render as literal placeholders (e.g., `S01E01_{resolution}.mkv`). This is the existing `render_template` behavior for unknown variables — a natural visual hint that the final filename will differ.

**History records:** The initial `record_file()` call uses the provisional filename. After `open_remux_input()` resolves the final filename, update the history record if the filename changed.

### 4. Fix Confirm screen size estimates

Switch `render_confirm_view()` to use `clip_sizes` with the 0.97 TS-to-MKV correction factor, matching the RipJob builder. Add `clip_sizes: &HashMap<String, u64>` to `ConfirmView`. Fall back to bitrate-based estimation only when `clip_sizes` has no entry.

Estimation priority (same as RipJob builder):
1. `clip_sizes[playlist_num] * 0.97`
2. `media_infos[playlist_num].bitrate_bps / 8 * duration`
3. `FALLBACK_BYTERATE (2,500,000) * duration`

### 5. CLI mode

The CLI rip loop (`cli.rs:1660-1663`) currently does a separate `probe_playlist()` call per playlist for stream selection. With the remux split, this is replaced by `open_remux_input()` — `StreamInfo` from the open phase is used for `--tracks` parsing (`parse_track_spec`) and `StreamFilter::apply()`, eliminating the redundant device open.

### 6. `RemuxOptions` and `prepare_remux_options()` refactor

`RemuxOptions` loses `device`, `playlist`, and `output` fields since they're handled by the open/write split. It becomes the write-phase config: chapters, stream selection, cancel flag, reserve index space, metadata.

`prepare_remux_options()` in `workflow.rs` is split accordingly: the output path construction and overwrite checks move to the rip-time resolution step (after `open_remux_input()`), while chapter extraction and stream selection setup remain (chapters require the disc mount point, which is available before the open phase).

**Chapter extraction timing:** Chapters are extracted from MPLS files before `write_remux()`. This requires the disc to be mounted, which happens before the rip loop starts (existing `ensure_mounted()` call). No change to mount timing.

## What doesn't change

- On-demand probing via `t` key in PlaylistManager (unchanged)
- MPLS-based stream counts from scan (unchanged)
- `clip_sizes` population during disc mount (unchanged)
- Chapter extraction from MPLS files (unchanged)
- `probe_playlist()` still exists for on-demand probing — it now calls `extract_media_and_stream_info()` internally

## Testing

- Update Confirm screen rendering tests to use `clip_sizes`
- `extract_media_and_stream_info` logic is already tested via `probe_playlist` tests
- No existing tests call `remux()` directly (requires hardware) — helper function tests (`select_streams`, `chapter_to_millis`, etc.) are unaffected
- No hardware or network tests required
