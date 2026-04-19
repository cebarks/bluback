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

- **`open_remux_input(device, playlist) -> Result<(InputContext, MakemkvconGuard, MediaInfo), MediaError>`** — Opens the FFmpeg input context, extracts `MediaInfo` from the stream metadata, and returns both along with the AACS guard. The guard must stay alive for the duration of the remux.
- **`write_remux(ictx, guard, options, on_progress) -> Result<usize, MediaError>`** — Takes the already-open input context and guard, performs the packet copy, chapter injection, and progress reporting. Returns chapters added.

A helper function `extract_media_info(ictx: &format::Input) -> MediaInfo` in `media/probe.rs` is factored out of the existing `probe_playlist()` logic so both code paths share it.

### 3. Resolve filenames at rip time

The rip loop (both TUI `dashboard.rs` and CLI `cli.rs`) follows this sequence per playlist:

1. Call `open_remux_input()` to get the FFmpeg context and `MediaInfo`
2. Use the `MediaInfo` to resolve the final filename via `playlist_filename()`
3. Set the output path on `RemuxOptions`
4. Call `write_remux()` with the open context

The Confirm screen shows filenames with whatever info is available. If the user's filename format includes media template variables (`{resolution}`, `{codec}`, etc.) and the playlist hasn't been probed via `t`, those render as literal placeholders (e.g., `S01E01_{resolution}.mkv`). This is the existing `render_template` behavior for unknown variables — no code change needed, it's a natural visual hint that the final filename will differ.

### 4. Fix Confirm screen size estimates

Switch `render_confirm_view()` to use `clip_sizes` with the 0.97 TS-to-MKV correction factor, matching the RipJob builder. Add `clip_sizes: &HashMap<String, u64>` to `ConfirmView`. Fall back to bitrate-based estimation only when `clip_sizes` has no entry.

Estimation priority (same as RipJob builder):
1. `clip_sizes[playlist_num] * 0.97`
2. `media_infos[playlist_num].bitrate_bps / 8 * duration`
3. `FALLBACK_BYTERATE (2,500,000) * duration`

### 5. CLI mode

The CLI rip loop (`cli.rs:1660-1663`) currently does a separate `probe_playlist()` call per playlist for stream selection. With the remux split, this is replaced by the `open_remux_input()` call — stream selection resolves from the already-open context, eliminating the redundant device open.

The CLI confirmation summary also switches to `clip_sizes` for size estimates.

### 6. `RemuxOptions` changes

`RemuxOptions.output` becomes `Option<PathBuf>` or is removed from the struct and passed to `write_remux()` directly, since the output path isn't known until after `open_remux_input()` returns `MediaInfo`.

The `device` and `playlist` fields move out of `RemuxOptions` since they're consumed by `open_remux_input()`. `RemuxOptions` becomes the write-phase config: output path, chapters, stream selection, cancel flag, reserve index space, metadata.

## What doesn't change

- On-demand probing via `t` key in PlaylistManager (unchanged)
- `StreamInfo` extraction for track picker (still from `probe_playlist()` or on-demand probe)
- MPLS-based stream counts from scan (unchanged)
- `clip_sizes` population during disc mount (unchanged)
- Chapter extraction from MPLS files (unchanged)

## Testing

- Update tests asserting on `remux()` return type (`Result<usize>` to `Result<(usize, MediaInfo)>` or equivalent)
- Update Confirm screen rendering tests to use `clip_sizes`
- Existing `extract_media_info` logic is already tested via `probe_playlist` tests
- No hardware or network tests required
