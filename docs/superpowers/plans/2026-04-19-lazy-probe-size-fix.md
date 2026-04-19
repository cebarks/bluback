# Lazy Probe & Size Estimate Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the unnecessary synchronous probe before the Confirm screen, extract MediaInfo/StreamInfo from the remux input context instead, and fix the size estimate discrepancy between the Confirm and Ripping screens.

**Architecture:** Split `remux()` into `open_remux_input()` (opens FFmpeg context, extracts media info) and `write_remux()` (packet copy + chapter injection). Callers resolve filenames and stream selection from the open phase's output before writing. Confirm screen switches from bitrate-based size estimates to `clip_sizes`.

**Tech Stack:** Rust, ffmpeg-the-third, ratatui

**Spec:** `docs/superpowers/specs/2026-04-19-lazy-probe-size-fix-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/media/probe.rs` | Modify | Extract shared `extract_media_and_stream_info()` from `probe_playlist()` |
| `src/media/remux.rs` | Modify | Split `remux()` into `open_remux_input()` + `write_remux()`, restructure `RemuxOptions` |
| `src/media/mod.rs` | Modify | Re-export new public functions |
| `src/workflow.rs` | Modify | Update `prepare_remux_options()` for new `RemuxOptions`, add `estimate_size()` |
| `src/types.rs` | Modify | Add `clip_sizes` to `ConfirmView` |
| `src/tui/wizard.rs` | Modify | Remove sync probe block, fix Confirm screen size estimates, add `RipMessage` enum |
| `src/tui/dashboard.rs` | Modify | Use split remux API, resolve filenames at rip time via `RipThreadContext` |
| `src/session.rs` | Modify | Add `clip_sizes` to `build_confirm_view()`, add `rip_thread_context()` helper |
| `src/cli.rs` | Modify | Use split remux API, remove separate `probe_playlist()` calls |

---

### Task 1: Extract `extract_media_and_stream_info()` from `probe_playlist()`

**Files:**
- Modify: `src/media/probe.rs`

Factor the stream iteration and MediaInfo/StreamInfo extraction logic (currently lines 577-704 of `probe_playlist()`) into a standalone public function.

- [ ] **Step 1: Add `extract_media_and_stream_info()` function**

Add this function above `probe_playlist()` in `src/media/probe.rs`. The body is lines 577-704 of the current `probe_playlist()`, extracted verbatim:

```rust
/// Extract MediaInfo and StreamInfo from an already-open FFmpeg input context.
///
/// Shared by `probe_playlist()` and `remux::open_remux_input()`.
pub fn extract_media_and_stream_info(
    ctx: &ffmpeg_the_third::format::context::Input,
) -> (crate::types::MediaInfo, crate::types::StreamInfo) {
    use ffmpeg_the_third::media::Type as MediaType;

    let mut media_info = crate::types::MediaInfo::default();
    let mut video_streams = Vec::new();
    let mut audio_streams = Vec::new();
    let mut subtitle_streams = Vec::new();
    let mut first_audio_done = false;

    for stream in ctx.streams() {
        let params = stream.parameters();
        match params.medium() {
            MediaType::Video => {
                let codec_id = params.id();
                let width = params.width();
                let height = params.height();
                let resolution = if height > 0 {
                    format!("{}x{}", width, height)
                } else {
                    String::new()
                };
                let rate = stream.rate();
                let framerate = format_framerate((rate.numerator(), rate.denominator()));

                let bits_raw = params.bits_per_raw_sample();
                let bit_depth = if bits_raw > 0 {
                    bits_raw.to_string()
                } else {
                    let bits_coded = params.bits_per_coded_sample();
                    if bits_coded > 0 {
                        bits_coded.to_string()
                    } else {
                        String::new()
                    }
                };

                let profile_raw = params.profile();
                let profile = Profile::from((codec_id, profile_raw));
                let color_trc = params.color_transfer_characteristic();
                let color_transfer_str = color_trc.name().unwrap_or("").to_string();
                let side_data_types = extract_side_data_types(&stream);
                let side_data_refs: Vec<&str> =
                    side_data_types.iter().map(|s| s.as_str()).collect();
                let hdr = classify_hdr(&color_transfer_str, &side_data_refs);

                if media_info.codec.is_empty() {
                    media_info.codec = codec_id.name().to_string();
                    media_info.width = width;
                    media_info.height = height;
                    media_info.resolution = if height > 0 {
                        format!("{}p", height)
                    } else {
                        String::new()
                    };
                    media_info.aspect_ratio = format_aspect_ratio(width, height);
                    media_info.framerate = framerate.clone();
                    media_info.bit_depth = bit_depth.clone();
                    media_info.profile = format_video_profile(profile);
                    media_info.hdr = hdr.clone();
                }

                video_streams.push(crate::types::VideoStream {
                    index: stream.index(),
                    codec: codec_id.name().to_string(),
                    resolution,
                    hdr,
                    framerate,
                    bit_depth,
                });
            }
            MediaType::Audio => {
                let codec_id = params.id();
                let codec_name = codec_id.name().to_string();
                let ch_layout = params.ch_layout();
                let channels = ch_layout.channels() as u16;
                let layout_desc = ch_layout.description();
                let channel_layout = format_channel_layout(channels, &layout_desc);
                let language = stream.metadata().get("language").map(|s| s.to_string());
                let profile_raw = params.profile();
                let profile = format_codec_profile(Profile::from((codec_id, profile_raw)));

                if !first_audio_done {
                    first_audio_done = true;
                    let prof = Profile::from((codec_id, profile_raw));
                    media_info.audio = match &prof {
                        Profile::DTS(dts) => format_dts_profile(dts).to_string(),
                        _ => codec_name.clone(),
                    };
                    media_info.channels = channel_layout.clone();
                    media_info.audio_lang = language.clone().unwrap_or_default();
                }

                audio_streams.push(crate::types::AudioStream {
                    index: stream.index(),
                    codec: codec_name,
                    channels,
                    channel_layout,
                    language,
                    profile,
                });
            }
            MediaType::Subtitle => {
                let codec_id = params.id();
                let language = stream.metadata().get("language").map(|s| s.to_string());
                let forced = false;

                subtitle_streams.push(crate::types::SubtitleStream {
                    index: stream.index(),
                    codec: codec_id.name().to_string(),
                    language,
                    forced,
                });
            }
            _ => {}
        }
    }

    let bitrate = ctx.bit_rate();
    media_info.bitrate_bps = if bitrate > 0 { bitrate as u64 } else { 0 };

    let stream_info = crate::types::StreamInfo {
        video_streams,
        audio_streams,
        subtitle_streams,
    };

    (media_info, stream_info)
}
```

- [ ] **Step 2: Refactor `probe_playlist()` to use the new function**

Replace lines 577-706 of `probe_playlist()` with:

```rust
pub fn probe_playlist(
    device: &str,
    playlist_num: &str,
) -> Result<(MediaInfo, StreamInfo), MediaError> {
    let mut guard = crate::aacs::MakemkvconGuard::new();
    let ctx = guard.track_open(|| open_bluray(device, Some(playlist_num)))?;

    Ok(extract_media_and_stream_info(&ctx))
}
```

- [ ] **Step 3: Verify**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: All pass — pure refactor, no behavior change.

- [ ] **Step 4: Commit**

```bash
git add src/media/probe.rs
git commit -m "refactor: extract media/stream info extraction into shared helper"
```

---

### Task 2: Split `remux()` into `open_remux_input()` + `write_remux()`

**Files:**
- Modify: `src/media/remux.rs`
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Restructure `RemuxOptions`**

Remove `device`, `playlist`, and `output` from `RemuxOptions` (lines 23-34 of `src/media/remux.rs`). These are now parameters to the split functions:

```rust
pub struct RemuxOptions {
    pub chapters: Vec<ChapterMark>,
    pub stream_selection: StreamSelection,
    pub cancel: Arc<AtomicBool>,
    pub reserve_index_space_kb: u32,
    pub metadata: Option<crate::types::MkvMetadata>,
}
```

- [ ] **Step 2: Add `open_remux_input()`**

Add above the existing `remux()` function:

```rust
/// Open the FFmpeg input context for a Blu-ray playlist and extract media/stream info.
///
/// Returns the input context, AACS guard (must stay alive through write phase),
/// and extracted MediaInfo + StreamInfo. Both `format::Input` and
/// `MakemkvconGuard` are `Send`, so they can be moved into a spawned thread.
pub fn open_remux_input(
    device: &str,
    playlist: &str,
) -> Result<
    (
        format::context::Input,
        crate::aacs::MakemkvconGuard,
        crate::types::MediaInfo,
        crate::types::StreamInfo,
    ),
    MediaError,
> {
    super::ensure_init();

    let mut guard = crate::aacs::MakemkvconGuard::new();
    let input_url = format!("bluray:{}", device);
    let mut opts = Dictionary::new();
    opts.set("playlist", playlist);

    let ictx = guard.track_open(|| {
        format::input_with_dictionary(&input_url, opts).map_err(|e| {
            if let Some(aacs_err) = classify_aacs_error(&e) {
                return aacs_err;
            }
            MediaError::Ffmpeg(e)
        })
    })?;

    let nb_streams = ictx.nb_streams() as usize;
    if nb_streams == 0 {
        return Err(MediaError::NoStreams);
    }

    let (media_info, stream_info) =
        crate::media::probe::extract_media_and_stream_info(&ictx);

    Ok((ictx, guard, media_info, stream_info))
}
```

- [ ] **Step 3: Add `write_remux()`**

This is the existing `remux()` body from line 169 onward, but receiving the input context, guard, and output path as parameters. The function signature:

```rust
/// Write phase of remux: copies packets from an already-open input context to MKV output.
///
/// `ictx` and `_guard` come from `open_remux_input()`. The guard's lifetime must
/// span this call to keep the AACS session alive.
pub fn write_remux<F>(
    mut ictx: format::context::Input,
    _guard: crate::aacs::MakemkvconGuard,
    output: &std::path::Path,
    options: RemuxOptions,
    on_progress: F,
) -> Result<usize, MediaError>
where
    F: Fn(&RipProgress),
{
    log::info!(
        "Remux started: output={}",
        output.display()
    );

    if output.exists() {
        return Err(MediaError::OutputExists(output.to_path_buf()));
    }

    // Everything from line 169 of current remux() onward:
    // - select_streams (line 174)
    // - create output context (line 180-185, using `output` param)
    // - stream mapping (line 189-211)
    // - duration extraction (line 213-221)
    // - timestamp normalization (line 227-241)
    // - chapter injection (line 252)
    // - metadata injection (line 255-261)
    // - header write with reserve_index_space (line 263-280)
    // - packet loop (line 280-389)
    // - trailer + final progress (line 391-428)
    //
    // Three substitutions:
    // - `options.output` → `output` (log messages at end)
    // - Remove the device-open block (lines 152-167) — done by open_remux_input
    // - Remove the nb_streams == 0 check (lines 170-172) — done by open_remux_input
```

Copy lines 169-428 of the current `remux()` as the body. Replace `options.output.display()` with `output.display()` in the final log line (line 426).

- [ ] **Step 4: Replace old `remux()` with convenience wrapper**

Replace the existing `remux()` function with a thin wrapper that calls both phases. This preserves backward compatibility until callers are migrated:

```rust
/// Convenience wrapper: opens input + writes output in one call.
pub fn remux<F>(
    device: &str,
    playlist: &str,
    output: &std::path::Path,
    options: RemuxOptions,
    on_progress: F,
) -> Result<(usize, crate::types::MediaInfo, crate::types::StreamInfo), MediaError>
where
    F: Fn(&RipProgress),
{
    let (ictx, guard, media_info, stream_info) = open_remux_input(device, playlist)?;
    let chapters_added = write_remux(ictx, guard, output, options, on_progress)?;
    Ok((chapters_added, media_info, stream_info))
}
```

- [ ] **Step 5: Update `src/media/mod.rs` re-exports**

Add the new public functions:

```rust
pub use remux::{open_remux_input, write_remux, RemuxOptions, StreamSelection};
```

- [ ] **Step 6: Verify compilation**

Run: `cargo check 2>&1 | head -40`
Expected: Compilation errors in `dashboard.rs`, `cli.rs`, and `workflow.rs` because `RemuxOptions` no longer has `device`/`playlist`/`output` fields. This is expected — callers are updated in subsequent tasks.

- [ ] **Step 7: Commit (WIP)**

```bash
git add src/media/remux.rs src/media/mod.rs
git commit -m "refactor: split remux into open_remux_input + write_remux phases

WIP — callers updated in subsequent commits."
```

---

### Task 3: Update `prepare_remux_options()` and add `estimate_size()`

**Files:**
- Modify: `src/workflow.rs`

- [ ] **Step 1: Update `prepare_remux_options()` signature**

Remove `device` and `output` parameters. Keep `playlist` for chapter extraction only:

```rust
#[allow(clippy::too_many_arguments)]
pub fn prepare_remux_options(
    playlist: &Playlist,
    mount_point: Option<&str>,
    stream_selection: StreamSelection,
    cancel: Arc<AtomicBool>,
    reserve_index_space_kb: u32,
    metadata: Option<crate::types::MkvMetadata>,
) -> RemuxOptions {
    let chapters = mount_point
        .and_then(|mount| {
            crate::chapters::extract_chapters(std::path::Path::new(mount), &playlist.num)
        })
        .unwrap_or_default();

    RemuxOptions {
        chapters,
        stream_selection,
        cancel,
        reserve_index_space_kb,
        metadata,
    }
}
```

- [ ] **Step 2: Add `estimate_size()` helper**

Add below `prepare_remux_options()`:

```rust
const TS_TO_MKV_FACTOR: f64 = 0.97;
const FALLBACK_BYTERATE: u64 = 2_500_000;

/// Estimate output MKV size for a playlist.
///
/// Priority: on-disc clip size (with TS→MKV correction) > probed bitrate > fallback (~20 Mbps).
pub fn estimate_size(
    playlist: &Playlist,
    clip_size: Option<u64>,
    media_info: Option<&MediaInfo>,
) -> u64 {
    clip_size
        .filter(|&sz| sz > 0)
        .map(|sz| (sz as f64 * TS_TO_MKV_FACTOR) as u64)
        .or_else(|| {
            media_info
                .map(|info| info.bitrate_bps / 8)
                .filter(|&br| br > 0)
                .map(|br| playlist.seconds as u64 * br)
        })
        .unwrap_or_else(|| playlist.seconds as u64 * FALLBACK_BYTERATE)
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check 2>&1 | head -40`
Expected: Still errors in `dashboard.rs` and `cli.rs` (they pass removed params to `prepare_remux_options`).

- [ ] **Step 4: Commit (WIP)**

```bash
git add src/workflow.rs
git commit -m "refactor: update prepare_remux_options for split remux API, add estimate_size"
```

---

### Task 4: Fix Confirm screen size estimates and remove sync probe block

**Files:**
- Modify: `src/types.rs:486-499` — add `clip_sizes` to `ConfirmView`
- Modify: `src/session.rs:446-477` — populate `clip_sizes` in `build_confirm_view()`
- Modify: `src/tui/wizard.rs:834-922` — use `clip_sizes` for size estimation
- Modify: `src/tui/wizard.rs:1657-1673` — remove synchronous probe block

- [ ] **Step 1: Add `clip_sizes` to `ConfirmView`**

In `src/types.rs`, add the field to `ConfirmView` (line 498):

```rust
pub struct ConfirmView {
    pub filenames: Vec<String>,
    pub playlists: Vec<Playlist>,
    #[allow(dead_code)]
    pub episode_assignments: EpisodeAssignments,
    #[allow(dead_code)]
    pub list_cursor: usize,
    pub movie_mode: bool,
    pub label: String,
    pub output_dir: String,
    pub dry_run: bool,
    pub media_infos: HashMap<String, MediaInfo>,
    pub clip_sizes: HashMap<String, u64>,
}
```

- [ ] **Step 2: Populate `clip_sizes` in `build_confirm_view()`**

In `src/session.rs`, update `build_confirm_view()` (around line 466-476) to include `clip_sizes`:

```rust
Some(ConfirmView {
    filenames: self.wizard.filenames.clone(),
    playlists: selected_playlists,
    episode_assignments: self.wizard.episode_assignments.clone(),
    list_cursor: self.wizard.list_cursor,
    movie_mode: self.tmdb.movie_mode,
    label: self.disc.label.clone(),
    output_dir: self.output_dir.display().to_string(),
    dry_run: false,
    media_infos: self.wizard.media_infos.clone(),
    clip_sizes: self.disc.clip_sizes.clone(),
})
```

- [ ] **Step 3: Update `render_confirm_view()` to use `estimate_size()`**

In `src/tui/wizard.rs`, replace the size estimation in `render_confirm_view()` (lines 865-884). Replace the `FALLBACK_BYTERATE` constant and bitrate-based calculation with a call to `workflow::estimate_size()`:

```rust
    let mut total_seconds: u32 = 0;
    let mut total_est_bytes: u64 = 0;

    let rows: Vec<Row> = view
        .playlists
        .iter()
        .zip(view.filenames.iter())
        .map(|(pl, name)| {
            total_seconds += pl.seconds;
            let est_bytes = crate::workflow::estimate_size(
                pl,
                view.clip_sizes.get(&pl.num).copied(),
                view.media_infos.get(&pl.num),
            );
            total_est_bytes += est_bytes;
            Row::new(vec![
                pl.num.clone(),
                pl.duration.clone(),
                format!("~{}", crate::util::format_size(est_bytes)),
                name.clone(),
            ])
        })
        .collect();
```

- [ ] **Step 4: Remove the synchronous probe block**

In `src/tui/wizard.rs`, delete the sync probe block (lines 1657-1674). This is the block starting with `let unprobed_selected: Vec<String> = selected_indices` and ending with the closing `}` of `if !unprobed_selected.is_empty()`.

- [ ] **Step 5: Update RipJob estimated_size to use `estimate_size()`**

In `src/tui/wizard.rs` `handle_confirm_input_session()` (around line 1766-1785), replace the inline size estimation with the shared helper:

```rust
                let estimated_size = crate::workflow::estimate_size(
                    &pl,
                    session.disc.clip_sizes.get(&pl.num).copied(),
                    session.wizard.media_infos.get(&pl.num),
                );
```

- [ ] **Step 6: Verify**

Run: `cargo check 2>&1 | head -40`
Expected: Remaining errors should only be in `dashboard.rs` and `cli.rs` (from tasks 5 and 6).

- [ ] **Step 7: Commit**

```bash
git add src/types.rs src/session.rs src/tui/wizard.rs
git commit -m "fix: use clip_sizes for Confirm screen size estimates, remove sync probe block"
```

---

### Task 5: Update TUI dashboard to use split remux API

**Files:**
- Modify: `src/tui/dashboard.rs`
- Modify: `src/session.rs` — add `rip_thread_context()` helper

This is the most complex task. The dashboard spawns a rip thread per playlist. With the split API, the thread now:
1. Opens the input context (`open_remux_input`)
2. Resolves stream selection from `StreamInfo` (when not pre-cached)
3. Resolves the final filename with `MediaInfo`
4. Checks overwrite
5. Calls `write_remux`

Communication with the main thread uses a new `RipMessage` enum instead of the current `Result<RipProgress, MediaError>`.

- [ ] **Step 1: Define `RipMessage` enum**

Add at the top of `src/tui/dashboard.rs`:

```rust
enum RipMessage {
    Progress(crate::types::RipProgress),
    MediaReady {
        media_info: crate::types::MediaInfo,
        stream_info: crate::types::StreamInfo,
        final_filename: String,
    },
    ChaptersAdded(usize),
    Error(crate::media::MediaError),
}
```

- [ ] **Step 2: Define `RipThreadContext` struct**

Add in `src/tui/dashboard.rs`. This captures everything the rip thread needs to resolve filenames and stream selection:

```rust
struct RipThreadContext {
    device: String,
    playlist: crate::types::Playlist,
    output_dir: std::path::PathBuf,
    // For filename resolution:
    episodes: Vec<crate::types::Episode>,
    season: u32,
    movie_mode: bool,
    is_special: bool,
    movie_title: Option<(String, String)>,
    show_name: String,
    label: String,
    label_info: Option<crate::types::LabelInfo>,
    config: crate::config::Config,
    format_override: Option<String>,
    format_preset_override: Option<String>,
    part: Option<u32>,
    // For stream selection fallback:
    cached_track_selection: Option<Vec<usize>>,
    stream_filter: crate::streams::StreamFilter,
    // Overwrite:
    overwrite: bool,
    estimated_size: u64,
}
```

- [ ] **Step 3: Add `rip_thread_context()` helper on `DriveSession`**

In `src/session.rs`, add a method that builds the context struct from session state:

```rust
    pub fn rip_thread_context(&self, job_idx: usize) -> crate::tui::dashboard::RipThreadContext {
        let pl = &self.rip.jobs[job_idx].playlist;
        let selected_count = self.rip.jobs.len();

        let movie_title = if self.tmdb.movie_mode {
            self.tmdb.selected_movie
                .and_then(|i| self.tmdb.movie_results.get(i))
                .map(|m| {
                    let year = m.release_date.as_deref()
                        .and_then(|d| d.get(..4))
                        .unwrap_or("")
                        .to_string();
                    (m.title.clone(), year)
                })
        } else {
            None
        };

        let part = if self.tmdb.movie_mode && selected_count > 1 {
            self.rip.jobs.iter()
                .position(|j| j.playlist.num == pl.num)
                .map(|p| p as u32 + 1)
        } else {
            None
        };

        let show_name = if !self.tmdb.show_name.is_empty() {
            self.tmdb.show_name.clone()
        } else {
            self.disc.label_info.as_ref()
                .map(|l| l.show.clone())
                .unwrap_or_else(|| "Unknown".to_string())
        };

        crate::tui::dashboard::RipThreadContext {
            device: self.device.to_string_lossy().to_string(),
            playlist: pl.clone(),
            output_dir: self.output_dir.clone(),
            episodes: self.wizard.episode_assignments
                .get(&pl.num)
                .cloned()
                .unwrap_or_default(),
            season: self.wizard.season_num.unwrap_or(0),
            movie_mode: self.tmdb.movie_mode,
            is_special: self.wizard.specials.contains(&pl.num),
            movie_title,
            show_name,
            label: self.disc.label.clone(),
            label_info: self.disc.label_info.clone(),
            config: self.config.clone(),
            format_override: self.format.clone(),
            format_preset_override: self.format_preset.clone(),
            part,
            cached_track_selection: self.wizard.track_selections.get(&pl.num).cloned(),
            stream_filter: self.stream_filter.clone(),
            overwrite: self.config.overwrite() || self.overwrite,
            estimated_size: self.rip.jobs[job_idx].estimated_size,
        }
    }
```

Note: `RipThreadContext` must be `pub` in `dashboard.rs` for `session.rs` to reference it. Alternatively, define it in `types.rs` or `workflow.rs`. The simplest path: define it in `src/workflow.rs` since it's a workflow concern, and both `session.rs` and `dashboard.rs` can access it.

Move the struct definition to `src/workflow.rs` and update the import in both files.

- [ ] **Step 4: Add filename resolution function to `workflow.rs`**

Add a method on `RipThreadContext` (or a free function) that resolves the filename:

```rust
impl RipThreadContext {
    pub fn resolve_filename(&self, media_info: Option<&MediaInfo>) -> String {
        build_output_filename(
            &self.playlist,
            &self.episodes,
            self.season,
            self.movie_mode,
            self.is_special,
            self.movie_title.as_ref().map(|(t, y)| (t.as_str(), y.as_str())),
            &self.show_name,
            &self.label,
            self.label_info.as_ref(),
            &self.config,
            self.format_override.as_deref(),
            self.format_preset_override.as_deref(),
            media_info,
            self.part,
        )
    }
}
```

- [ ] **Step 5: Rewrite `start_next_job_session()`**

Replace the body of `start_next_job_session()` in `src/tui/dashboard.rs`. The new flow:

```rust
fn start_next_job_session(
    session: &mut crate::session::DriveSession,
    history_db: &Option<crate::history::HistoryDb>,
) -> bool {
    let next_idx = session
        .rip
        .jobs
        .iter()
        .position(|j| matches!(j.status, PlaylistStatus::Pending));

    let Some(idx) = next_idx else {
        return false;
    };

    session.rip.current_rip = idx;

    // Build thread context with all data needed for filename/stream resolution
    let rip_ctx = session.rip_thread_context(idx);

    // Build RemuxOptions (chapters, cancel flag, metadata, etc.)
    // Stream selection is StreamSelection::All initially — resolved in thread from StreamInfo
    let cancel = session.rip.cancel.clone();
    cancel.store(false, std::sync::atomic::Ordering::Relaxed);

    let metadata = /* ... same metadata building as current lines 831-869 ... */;

    let job_playlist = session.rip.jobs[idx].playlist.clone();
    let mount_point = session.disc.mount_point.clone();
    let options = crate::workflow::prepare_remux_options(
        &job_playlist,
        mount_point.as_deref(),
        crate::media::StreamSelection::All, // placeholder — resolved in thread
        cancel,
        session.config.reserve_index_space(),
        metadata,
    );

    // Record file start in history (with provisional filename)
    // ... same history recording as current lines 882-912 ...

    session.rip.chapters_added.store(0, std::sync::atomic::Ordering::Relaxed);
    let chapters_added_arc = session.rip.chapters_added.clone();

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // Phase 1: Open input context
        let (ictx, guard, media_info, stream_info) =
            match crate::media::remux::open_remux_input(
                &rip_ctx.device,
                &rip_ctx.playlist.num,
            ) {
                Ok(result) => result,
                Err(e) => {
                    let _ = tx.send(RipMessage::Error(e));
                    return;
                }
            };

        // Resolve final filename with media info
        let final_filename = rip_ctx.resolve_filename(Some(&media_info));
        let outfile = rip_ctx.output_dir.join(&final_filename);

        // Send media info back to main thread
        let _ = tx.send(RipMessage::MediaReady {
            media_info,
            stream_info: stream_info.clone(),
            final_filename: final_filename.clone(),
        });

        // Create output directory if needed
        if let Some(parent) = outfile.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                let _ = tx.send(RipMessage::Error(
                    crate::media::MediaError::RemuxFailed(format!(
                        "Failed to create output directory: {}", e
                    ))
                ));
                return;
            }
        }

        // Check overwrite
        match crate::workflow::check_overwrite(
            &outfile,
            rip_ctx.overwrite,
            Some(rip_ctx.estimated_size).filter(|&s| s > 0),
        ) {
            Ok(crate::workflow::OverwriteAction::Proceed) => {}
            Ok(crate::workflow::OverwriteAction::DeleteAndProceed(_)) => {}
            Ok(crate::workflow::OverwriteAction::PartialReplace(_)) => {}
            Ok(crate::workflow::OverwriteAction::Skip(_)) => {
                // Can't easily skip from inside the thread — send error
                // The main thread will handle this via the error message
                let _ = tx.send(RipMessage::Error(
                    crate::media::MediaError::OutputExists(outfile),
                ));
                return;
            }
            Err(e) => {
                let _ = tx.send(RipMessage::Error(
                    crate::media::MediaError::RemuxFailed(format!("Overwrite check: {}", e)),
                ));
                return;
            }
        }

        // Resolve stream selection
        let final_stream_selection = if let Some(indices) = rip_ctx.cached_track_selection {
            crate::media::StreamSelection::Manual(indices)
        } else if !rip_ctx.stream_filter.is_empty() {
            crate::media::StreamSelection::Manual(rip_ctx.stream_filter.apply(&stream_info))
        } else {
            crate::media::StreamSelection::All
        };

        let mut options = options;
        options.stream_selection = final_stream_selection;

        // Phase 2: Write remux
        let tx_progress = tx.clone();
        let result = crate::media::remux::write_remux(
            ictx,
            guard,
            &outfile,
            options,
            |progress| {
                let _ = tx_progress.send(RipMessage::Progress(progress.clone()));
            },
        );

        match result {
            Ok(added) => {
                chapters_added_arc.store(added, std::sync::atomic::Ordering::Relaxed);
                let _ = tx.send(RipMessage::ChaptersAdded(added));
            }
            Err(e) => {
                let _ = tx.send(RipMessage::Error(e));
            }
        }
    });

    session.rip.progress_rx_msg = Some(rx);
    session.rip.jobs[idx].status = PlaylistStatus::Ripping(crate::types::RipProgress::default());
    true
}
```

Note: `session.rip.progress_rx` currently has type `Option<mpsc::Receiver<Result<RipProgress, MediaError>>>`. This changes to `Option<mpsc::Receiver<RipMessage>>`. Update the field name and type in `RipState` (in `tui/mod.rs` or `types.rs`, wherever `RipState` is defined). Use `progress_rx` (same name) to minimize churn.

- [ ] **Step 6: Update `poll_active_job_session()` to handle `RipMessage`**

Update the polling function (around line 939) to match on `RipMessage` variants instead of `Result<RipProgress, MediaError>`:

```rust
fn poll_active_job_session(
    session: &mut crate::session::DriveSession,
    history_db: &Option<crate::history::HistoryDb>,
) -> bool {
    let rx = match session.rip.progress_rx {
        Some(ref rx) => rx,
        None => return false,
    };

    let mut changed = false;
    loop {
        match rx.try_recv() {
            Ok(RipMessage::Progress(progress)) => {
                let idx = session.rip.current_rip;
                session.rip.jobs[idx].status = PlaylistStatus::Ripping(progress);
                changed = true;
            }
            Ok(RipMessage::MediaReady { media_info, stream_info, final_filename }) => {
                let idx = session.rip.current_rip;
                let pl_num = session.rip.jobs[idx].playlist.num.clone();
                session.wizard.media_infos.insert(pl_num.clone(), media_info);
                session.wizard.stream_infos.insert(pl_num, stream_info);
                // Update job filename if it changed
                if session.rip.jobs[idx].filename != final_filename {
                    session.rip.jobs[idx].filename = final_filename;
                    // TODO: update history record if filename changed
                }
                changed = true;
            }
            Ok(RipMessage::ChaptersAdded(_added)) => {
                // chapters_added_arc already updated via AtomicUsize
                // Mark job complete
                let idx = session.rip.current_rip;
                let outfile = session.output_dir.join(&session.rip.jobs[idx].filename);
                let final_size = std::fs::metadata(&outfile).map(|m| m.len()).unwrap_or(0);
                session.rip.jobs[idx].status = PlaylistStatus::Done(final_size);
                session.rip.progress_rx = None;
                // ... existing completion logic (history, hooks, verify) ...
                changed = true;
            }
            Ok(RipMessage::Error(e)) => {
                let idx = session.rip.current_rip;
                session.rip.jobs[idx].status = PlaylistStatus::Failed(e.to_string());
                // Clean up partial file
                let outfile = session.output_dir.join(&session.rip.jobs[idx].filename);
                if outfile.exists() {
                    let _ = std::fs::remove_file(&outfile);
                }
                session.rip.progress_rx = None;
                // ... existing error handling ...
                changed = true;
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                // Thread exited without sending completion/error
                let idx = session.rip.current_rip;
                if matches!(session.rip.jobs[idx].status, PlaylistStatus::Ripping(_)) {
                    session.rip.jobs[idx].status =
                        PlaylistStatus::Failed("Remux thread disconnected".into());
                }
                session.rip.progress_rx = None;
                changed = true;
                break;
            }
        }
    }
    changed
}
```

The existing completion logic (history recording, hooks, verification) from the current `poll_active_job_session` moves into the `ChaptersAdded` and `Error` arms. Copy the existing code blocks for these.

- [ ] **Step 7: Update `RipState.progress_rx` type**

Find `progress_rx` field definition (in `src/tui/mod.rs` or `src/types.rs`) and change its type:

```rust
// Old:
pub progress_rx: Option<mpsc::Receiver<Result<RipProgress, crate::media::MediaError>>>,
// New:
pub progress_rx: Option<mpsc::Receiver<crate::tui::dashboard::RipMessage>>,
```

Make `RipMessage` `pub` in `dashboard.rs`.

- [ ] **Step 8: Move overwrite check + directory creation out of main thread**

The current `start_next_job_session` does overwrite check and dir creation on the main thread (lines 769-813). Since the final filename may differ from the provisional one, these checks move into the thread (already shown in step 5). Remove the main-thread overwrite check and dir creation from `start_next_job_session`.

- [ ] **Step 9: Verify**

Run: `cargo check`
Expected: Errors only in `cli.rs` (updated in Task 6).

- [ ] **Step 10: Commit**

```bash
git add src/tui/dashboard.rs src/session.rs src/workflow.rs src/tui/mod.rs
git commit -m "feat: use split remux API in TUI dashboard, resolve filenames at rip time"
```

---

### Task 6: Update CLI to use split remux API

**Files:**
- Modify: `src/cli.rs:1570-1800`

The CLI rip loop currently calls `probe_playlist()` per playlist for stream selection (lines 1660-1663), then `remux()` (line 1757). Replace with `open_remux_input()` + stream resolution + `write_remux()`.

- [ ] **Step 1: Replace the probe + remux calls in the rip loop**

In the `for (i, &idx) in selected.iter().enumerate()` loop (starting line 1570), replace the probe and remux calls:

```rust
        // Phase 1: Open input context
        let (ictx, guard, media_info, stream_info) =
            crate::media::remux::open_remux_input(device, &pl.num)?;

        // Resolve stream selection from StreamInfo (replaces separate probe_playlist call)
        let stream_selection = if let Some(tracks) = tracks_spec {
            match crate::streams::parse_track_spec(tracks, &stream_info) {
                Ok(indices) => {
                    let errors = crate::streams::validate_track_selection(&indices, &stream_info);
                    if !errors.is_empty() {
                        eprintln!("Warning: Playlist {}: {}", pl.num, errors.join(", "));
                    }
                    crate::media::StreamSelection::Manual(indices)
                }
                Err(e) => {
                    anyhow::bail!("Invalid --tracks spec: {}", e);
                }
            }
        } else if !stream_filter.is_empty() {
            let indices = stream_filter.apply(&stream_info);
            let errors = crate::streams::validate_track_selection(&indices, &stream_info);
            if !errors.is_empty() {
                eprintln!("Warning: Playlist {}: {}", pl.num, errors.join(", "));
            }
            crate::media::StreamSelection::Manual(indices)
        } else {
            crate::media::StreamSelection::All
        };

        // Compute expected stream counts for verification
        let (expected_video, expected_audio, expected_subtitle) = match &stream_selection {
            crate::media::StreamSelection::Manual(indices) => {
                crate::streams::count_selected_streams(indices, &stream_info)
            }
            crate::media::StreamSelection::All => {
                (pl.video_streams, pl.audio_streams, pl.subtitle_streams)
            }
        };

        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut options = crate::workflow::prepare_remux_options(
            pl,
            mount_point.as_deref(),
            stream_selection,
            cancel,
            config.reserve_index_space(),
            metadata_per_playlist[i].clone(),
        );

        // ... existing history recording (lines 1726-1749) ...

        let pl_seconds = pl.seconds;
        let is_tty = crate::atty_stdout();
        let last_print = std::cell::Cell::new(std::time::Instant::now());
        let started = std::cell::Cell::new(false);
        let pl_num = pl.num.clone();

        // Phase 2: Write remux
        let result = crate::media::remux::write_remux(
            ictx,
            guard,
            outfile,
            options,
            |progress| {
                // ... existing progress callback (lines 1758-1794) ...
            },
        );
```

- [ ] **Step 2: Remove the old `on_demand` probe call**

Delete lines 1658-1663 (the `probe_cache.get().or_else(|| probe_playlist(...))` block). The `on_demand` variable is no longer used — stream info comes from `open_remux_input()`.

- [ ] **Step 3: Update result handling**

The old `remux()` returned `Result<usize, MediaError>` (chapters added). `write_remux()` returns the same `Result<usize, MediaError>`. The match block (lines 1803-1810) stays the same:

```rust
        match result {
            Ok(chapters_added) => {
                // ... existing success handling ...
            }
            Err(e) => {
                // ... existing error handling ...
            }
        }
```

No change needed here.

- [ ] **Step 4: Remove `probe_cache` parameter from the rip function**

The rip function signature (line 1490-1516) takes `probe_cache: &crate::types::ProbeCache`. Since we no longer use it, remove the parameter. Update the caller in `cli.rs` that passes `&probe_cache`.

Search for the call site — it's in the interactive and headless rip paths. Update both callers.

- [ ] **Step 5: Verify full compilation**

Run: `cargo check`
Expected: Clean compilation — all callers updated.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 7: Run clippy and format**

Run: `rustup run stable cargo fmt && cargo clippy -- -D warnings`
Expected: Clean.

- [ ] **Step 8: Commit**

```bash
git add src/cli.rs
git commit -m "feat: use split remux API in CLI, remove redundant probe_playlist calls"
```

---

### Task 7: Clean up and remove convenience wrapper

**Files:**
- Modify: `src/media/remux.rs` — remove `remux()` wrapper if no callers remain
- Modify: `src/media/mod.rs` — update re-exports

- [ ] **Step 1: Check for remaining callers of `remux()`**

Search for all calls to `crate::media::remux::remux` or `media::remux(`:

Run: `grep -rn 'remux::remux\b\|media::remux(' src/ --include='*.rs'`

If no callers remain (dashboard and CLI both migrated), remove the wrapper.

- [ ] **Step 2: Remove `remux()` wrapper if unused**

Delete the `remux()` convenience function from `src/media/remux.rs`.

Update `src/media/mod.rs` to remove `remux` from re-exports if it was listed.

- [ ] **Step 3: Final verification**

Run: `cargo test && rustup run stable cargo fmt && cargo clippy -- -D warnings`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/media/remux.rs src/media/mod.rs
git commit -m "chore: remove unused remux convenience wrapper"
```

---

### Task 8: Update tests

**Files:**
- Modify: test files that reference `RemuxOptions` or the old `remux()` signature

- [ ] **Step 1: Find affected tests**

Run: `grep -rn 'RemuxOptions\|fn remux\|FALLBACK_BYTERATE' src/ tests/ --include='*.rs'`

Update any tests that construct `RemuxOptions` (remove `device`/`playlist`/`output` fields). Update any tests that reference `FALLBACK_BYTERATE` in wizard.rs (now in `workflow.rs`).

- [ ] **Step 2: Update `RemuxOptions` construction in tests**

For any test constructing `RemuxOptions`, remove the three dropped fields:

```rust
// Old:
RemuxOptions {
    device: "test".into(),
    playlist: "00001".into(),
    output: PathBuf::from("test.mkv"),
    chapters: vec![],
    stream_selection: StreamSelection::All,
    cancel: Arc::new(AtomicBool::new(false)),
    reserve_index_space_kb: 500,
    metadata: None,
}

// New:
RemuxOptions {
    chapters: vec![],
    stream_selection: StreamSelection::All,
    cancel: Arc::new(AtomicBool::new(false)),
    reserve_index_space_kb: 500,
    metadata: None,
}
```

- [ ] **Step 3: Add test for `estimate_size()`**

In `src/workflow.rs` tests module, add:

```rust
#[test]
fn test_estimate_size_clip_size_priority() {
    let pl = Playlist {
        num: "00001".into(),
        duration: "1:00:00".into(),
        seconds: 3600,
        ..Default::default()
    };
    // Clip size takes priority
    let result = estimate_size(&pl, Some(10_000_000_000), None);
    assert_eq!(result, (10_000_000_000f64 * 0.97) as u64);
}

#[test]
fn test_estimate_size_bitrate_fallback() {
    let pl = Playlist {
        num: "00001".into(),
        duration: "1:00:00".into(),
        seconds: 3600,
        ..Default::default()
    };
    let mi = MediaInfo {
        bitrate_bps: 40_000_000, // 40 Mbps
        ..Default::default()
    };
    // No clip size → falls back to bitrate
    let result = estimate_size(&pl, None, Some(&mi));
    assert_eq!(result, 3600 * (40_000_000 / 8));
}

#[test]
fn test_estimate_size_default_fallback() {
    let pl = Playlist {
        num: "00001".into(),
        duration: "1:00:00".into(),
        seconds: 3600,
        ..Default::default()
    };
    // No clip size, no bitrate → fallback
    let result = estimate_size(&pl, None, None);
    assert_eq!(result, 3600 * 2_500_000);
}

#[test]
fn test_estimate_size_zero_clip_size_skipped() {
    let pl = Playlist {
        num: "00001".into(),
        duration: "1:00:00".into(),
        seconds: 3600,
        ..Default::default()
    };
    let mi = MediaInfo {
        bitrate_bps: 40_000_000,
        ..Default::default()
    };
    // clip_size = 0 is treated as unavailable
    let result = estimate_size(&pl, Some(0), Some(&mi));
    assert_eq!(result, 3600 * (40_000_000 / 8));
}
```

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "test: update tests for split remux API, add estimate_size tests"
```

---

### Task 9: Final verification and format

- [ ] **Step 1: Run all checks**

```bash
cargo test && rustup run stable cargo fmt && cargo clippy -- -D warnings
```

Expected: All pass.

- [ ] **Step 2: Review all changes**

Run: `git diff main --stat` and `git log --oneline main..HEAD`

Verify the commit history matches the plan.

- [ ] **Step 3: Commit any final adjustments**

If fmt or clippy required changes:

```bash
git add -A
git commit -m "style: format and lint fixes"
```
