use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use ffmpeg_the_third::{self as ffmpeg, format, media, Dictionary, Packet, Rational};

use super::error::{classify_aacs_error, MediaError};
use crate::types::{AudioStream, ChapterMark, RipProgress, StreamInfo};

/// How to select streams from the input for remuxing.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // Public API — Manual variant used when media module is consumed directly
pub enum StreamSelection {
    /// Map every stream from the input.
    #[default]
    All,
    /// Video + surround audio (stereo as secondary) + all subtitles.
    PreferSurround,
    /// Exact stream indices provided by the caller.
    Manual(Vec<usize>),
}

/// All parameters needed to perform a single remux operation.
pub struct RemuxOptions {
    pub device: String,
    pub playlist: String,
    pub output: PathBuf,
    pub chapters: Vec<ChapterMark>,
    pub stream_selection: StreamSelection,
    pub cancel: Arc<AtomicBool>,
    /// KB of void space to reserve after the MKV header for the seek index.
    pub reserve_index_space_kb: u32,
    /// MKV metadata tags to embed in the output file.
    pub metadata: Option<crate::types::MkvMetadata>,
}

/// Determine which input stream indices to include in the output.
///
/// - `All` maps every stream index `0..total_streams`.
/// - `PreferSurround` maps all video streams, surround audio (plus stereo
///   as a secondary track), and all subtitle streams.
/// - `Manual` passes through exactly the indices the caller specified.
pub fn select_streams(
    selection: &StreamSelection,
    info: &StreamInfo,
    total_streams: usize,
) -> Vec<usize> {
    match selection {
        StreamSelection::All => (0..total_streams).collect(),

        StreamSelection::PreferSurround => {
            let mut indices = Vec::new();

            // We don't know which indices are video/subtitle from StreamInfo alone,
            // so include every index that isn't accounted for by audio_streams.
            let audio_indices: Vec<usize> = info.audio_streams.iter().map(|s| s.index).collect();

            // Add all non-audio streams (video, subtitle, data, etc.)
            for i in 0..total_streams {
                if !audio_indices.contains(&i) {
                    indices.push(i);
                }
            }

            // Add surround audio streams
            let surround_idx = info.audio_streams.iter().position(|s| s.is_surround());
            let stereo_idx = info.audio_streams.iter().position(|s| s.channels == 2);

            if let Some(idx) = surround_idx {
                indices.push(info.audio_streams[idx].index);
                if let Some(si) = stereo_idx {
                    if si != idx {
                        indices.push(info.audio_streams[si].index);
                    }
                }
            } else if !info.audio_streams.is_empty() {
                // No surround found, take the first audio stream
                indices.push(info.audio_streams[0].index);
            }

            indices.sort_unstable();
            indices.dedup();
            indices
        }

        StreamSelection::Manual(indices) => indices.clone(),
    }
}

/// Convert a chapter's start time to milliseconds, returning (start_ms, end_ms).
///
/// `end_ms` is set equal to `start_ms` here -- actual end times are computed
/// by `compute_chapter_ends` using the next chapter's start or the total duration.
fn chapter_to_millis(chapter: &ChapterMark, _total_duration: Option<f64>) -> (i64, i64) {
    let start_ms = (chapter.start_secs * 1000.0).round() as i64;
    (start_ms, start_ms)
}

/// Compute end timestamps (in milliseconds) for each chapter.
///
/// Each chapter ends where the next one begins (minus 1ms to avoid overlap).
/// The last chapter ends at `total_duration_secs`.
fn compute_chapter_ends(chapters: &[ChapterMark], total_duration_secs: f64) -> Vec<i64> {
    let total_ms = (total_duration_secs * 1000.0).round() as i64;

    chapters
        .iter()
        .enumerate()
        .map(|(i, _)| {
            if i + 1 < chapters.len() {
                let next_start_ms = (chapters[i + 1].start_secs * 1000.0).round() as i64;
                // End 1ms before the next chapter starts
                (next_start_ms - 1).max(0)
            } else {
                total_ms
            }
        })
        .collect()
}

/// Inject chapter metadata into the output format context.
///
/// Uses ffmpeg-the-third's `add_chapter` API which handles the underlying
/// `av_mallocz` + `av_dynarray_add` for AVChapter allocation.
/// Returns the number of chapters successfully injected.
fn inject_chapters(
    octx: &mut format::context::Output,
    chapters: &[ChapterMark],
    total_duration_secs: f64,
) -> Result<usize, MediaError> {
    if chapters.is_empty() || total_duration_secs <= 0.0 {
        return Ok(0);
    }

    let ends = compute_chapter_ends(chapters, total_duration_secs);
    // Time base of 1/1000 means timestamps are in milliseconds
    let time_base = Rational(1, 1000);
    let mut added = 0usize;

    for (i, chapter) in chapters.iter().enumerate() {
        let (start_ms, _) = chapter_to_millis(chapter, Some(total_duration_secs));
        let end_ms = ends[i];

        // Skip chapters that start beyond the stream duration
        if start_ms >= end_ms {
            continue;
        }

        let title = format!("Chapter {}", i + 1);

        // add_chapter can fail if timestamps exceed the format context's
        // duration. Log and continue rather than aborting the entire rip.
        match octx.add_chapter(i as i64, time_base, start_ms, end_ms, title) {
            Ok(_) => added += 1,
            Err(e) => {
                log::warn!(
                    "could not add chapter {} (start={}ms end={}ms duration={:.1}s): {}",
                    i + 1,
                    start_ms,
                    end_ms,
                    total_duration_secs,
                    e
                );
            }
        }
    }

    Ok(added)
}

/// Lossless remux of a Blu-ray playlist to MKV via FFmpeg library API.
///
/// This replaces the old approach of spawning an `ffmpeg` CLI process.
/// All streams are copied (`-c copy` equivalent) -- no re-encoding.
///
/// Progress is reported via the `on_progress` callback approximately every 100ms.
/// The `cancel` flag in `options` is checked each packet iteration; if set, the
/// function writes a trailer for a clean close and returns `Err(MediaError::Cancelled)`.
pub fn remux<F>(options: RemuxOptions, on_progress: F) -> Result<usize, MediaError>
where
    F: Fn(&RipProgress),
{
    super::ensure_init();

    log::info!(
        "Remux started: playlist={}, output={}",
        options.playlist,
        options.output.display()
    );

    if options.output.exists() {
        return Err(MediaError::OutputExists(options.output.clone()));
    }

    // Open input: bluray:{device} with playlist option
    let input_url = format!("bluray:{}", options.device);
    let mut opts = Dictionary::new();
    opts.set("playlist", &options.playlist);

    let mut ictx = format::input_with_dictionary(&input_url, opts).map_err(|e| {
        if let Some(aacs_err) = classify_aacs_error(&e) {
            return aacs_err;
        }
        MediaError::Ffmpeg(e)
    })?;

    let nb_input_streams = ictx.nb_streams() as usize;
    if nb_input_streams == 0 {
        return Err(MediaError::NoStreams);
    }

    // Build stream info for selection (we need to probe audio streams)
    let stream_info = build_stream_info(&ictx);
    let selected = select_streams(&options.stream_selection, &stream_info, nb_input_streams);

    if selected.is_empty() {
        return Err(MediaError::NoStreams);
    }

    // Create output context for MKV
    let output_path = options
        .output
        .to_str()
        .ok_or_else(|| MediaError::RemuxFailed("Output path contains invalid UTF-8".into()))?;
    let mut octx = format::output(output_path)?;

    // Map from input stream index -> output stream index
    // -1 means not mapped
    let mut stream_map: Vec<i32> = vec![-1; nb_input_streams];

    for (out_idx, &in_idx) in (0_i32..).zip(selected.iter()) {
        let in_stream = ictx
            .stream(in_idx)
            .ok_or_else(|| MediaError::RemuxFailed(format!("Input stream {} not found", in_idx)))?;

        // Add output stream and copy codec parameters
        let codec_id = in_stream.parameters().id();
        let mut out_stream = octx.add_stream(codec_id)?;
        out_stream.set_parameters(in_stream.parameters());
        out_stream.set_time_base(in_stream.time_base());

        // Clear codec tag for container compatibility (equivalent to codec_tag = 0)
        unsafe {
            let st_ptr = out_stream.as_mut_ptr();
            (*(*st_ptr).codecpar).codec_tag = 0;
        }

        stream_map[in_idx] = out_idx;
        // TODO: Per-stream metadata titles (e.g. "English - DTS-HD MA 5.1")
        // to be implemented alongside per-stream track selection in v0.10
    }

    // Get total duration from input context (in AV_TIME_BASE units, i.e. microseconds)
    let total_duration_secs = {
        let dur = ictx.duration();
        if dur > 0 {
            dur as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE)
        } else {
            0.0
        }
    };

    // Inject chapters before writing header
    let chapters_added = inject_chapters(&mut octx, &options.chapters, total_duration_secs)?;

    // Inject MKV metadata tags before writing header
    if let Some(ref meta) = options.metadata {
        let mut dict = Dictionary::new();
        for (k, v) in &meta.tags {
            dict.set(k, v);
        }
        octx.set_metadata(dict);
        log::debug!("Injected {} metadata tag(s)", meta.tags.len());
    }

    // Write output header, reserving void space for the seek index (Cues)
    // so they can be written at the front of the file for faster seeking.
    let reserve_bytes = (options.reserve_index_space_kb as u64) * 1024;
    let mut muxer_opts = Dictionary::new();
    muxer_opts.set("reserve_index_space", &reserve_bytes.to_string());
    octx.write_header_with(muxer_opts)
        .map_err(|e| MediaError::RemuxFailed(format!("Failed to write header: {}", e)))?;

    // Packet remux loop
    let wall_start = Instant::now();
    let mut last_progress = Instant::now();
    let mut video_frames: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut last_pts_secs: f64 = 0.0;

    let mut packet = Packet::empty();

    loop {
        if options.cancel.load(Ordering::Relaxed) {
            let _ = octx.write_trailer();
            log::warn!("Remux cancelled: {}", options.output.display());
            return Err(MediaError::Cancelled);
        }

        match packet.read(&mut ictx) {
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => break,
            Err(e) => {
                log::warn!("Remux failed: {}: {}", options.output.display(), e);
                return Err(MediaError::RemuxFailed(format!(
                    "Error reading packet: {}",
                    e
                )));
            }
        }

        let in_stream_idx = packet.stream();
        let out_stream_idx = match stream_map.get(in_stream_idx) {
            Some(&idx) if idx >= 0 => idx as usize,
            _ => continue, // Stream not selected, skip
        };

        // Get time bases for rescaling
        let in_time_base = ictx
            .stream(in_stream_idx)
            .map(|s| s.time_base())
            .unwrap_or(Rational(1, 90000));
        let out_time_base = octx
            .stream(out_stream_idx)
            .map(|s| s.time_base())
            .unwrap_or(Rational(1, 90000));

        // Track video frames and content time
        let in_stream = ictx.stream(in_stream_idx);
        let is_video = in_stream
            .map(|s| s.parameters().medium() == media::Type::Video)
            .unwrap_or(false);

        if is_video {
            video_frames += 1;
            if let Some(pts) = packet.pts() {
                let tb = in_time_base;
                if tb.1 != 0 {
                    last_pts_secs = pts as f64 * tb.0 as f64 / tb.1 as f64;
                }
            }
        }

        total_bytes += packet.size() as u64;

        // Rescale timestamps and set output stream index
        packet.rescale_ts(in_time_base, out_time_base);
        packet.set_stream(out_stream_idx);

        // Write packet (interleaved for proper ordering)
        packet.write_interleaved(&mut octx).map_err(|e| {
            log::warn!("Remux failed: {}: {}", options.output.display(), e);
            MediaError::RemuxFailed(format!("Error writing packet: {}", e))
        })?;

        // Report progress every ~100ms
        if last_progress.elapsed().as_millis() >= 100 {
            let elapsed = wall_start.elapsed().as_secs_f64();
            let fps = if elapsed > 0.0 {
                video_frames as f64 / elapsed
            } else {
                0.0
            };
            let speed = if elapsed > 0.0 {
                last_pts_secs / elapsed
            } else {
                0.0
            };
            let bitrate = if elapsed > 0.0 {
                format!(
                    "{:.1}kbits/s",
                    (total_bytes as f64 * 8.0) / elapsed / 1000.0
                )
            } else {
                "0kbits/s".into()
            };

            on_progress(&RipProgress {
                frame: video_frames,
                fps,
                total_size: total_bytes,
                out_time_secs: last_pts_secs as u32,
                bitrate,
                speed,
            });
            last_progress = Instant::now();
        }
    }

    // Write trailer to finalize the output file
    octx.write_trailer()
        .map_err(|e| MediaError::RemuxFailed(format!("Failed to write trailer: {}", e)))?;

    // Send final progress
    let elapsed = wall_start.elapsed().as_secs_f64();
    let fps = if elapsed > 0.0 {
        video_frames as f64 / elapsed
    } else {
        0.0
    };
    let speed = if elapsed > 0.0 {
        last_pts_secs / elapsed
    } else {
        0.0
    };
    let bitrate = if elapsed > 0.0 {
        format!(
            "{:.1}kbits/s",
            (total_bytes as f64 * 8.0) / elapsed / 1000.0
        )
    } else {
        "0kbits/s".into()
    };

    on_progress(&RipProgress {
        frame: video_frames,
        fps,
        total_size: total_bytes,
        out_time_secs: last_pts_secs as u32,
        bitrate,
        speed,
    });

    log::info!("Remux completed: {}", options.output.display());
    Ok(chapters_added)
}

/// Build a StreamInfo from an open input context by inspecting codec parameters.
fn build_stream_info(ictx: &format::context::Input) -> StreamInfo {
    let mut audio_streams = Vec::new();
    let mut subtitle_count = 0u32;

    for stream in ictx.streams() {
        let params = stream.parameters();
        match params.medium() {
            media::Type::Audio => {
                let channels = params.ch_layout().channels() as u16;

                let channel_layout = match channels {
                    1 => "mono".into(),
                    2 => "stereo".into(),
                    6 => "5.1".into(),
                    8 => "7.1".into(),
                    n => format!("{} channels", n),
                };

                let codec_id = params.id();
                let codec_name = codec_id.name();
                let lang = stream.metadata().get("language").map(String::from);

                audio_streams.push(AudioStream {
                    index: stream.index(),
                    codec: codec_name.to_string(),
                    channels,
                    channel_layout,
                    language: lang,
                    profile: None,
                });
            }
            media::Type::Subtitle => {
                subtitle_count += 1;
            }
            _ => {}
        }
    }

    StreamInfo {
        audio_streams,
        subtitle_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chapter_to_time_base() {
        let chapter = ChapterMark {
            index: 0,
            start_secs: 1.5,
        };
        let (start_ms, _end_ms) = chapter_to_millis(&chapter, None);
        assert_eq!(start_ms, 1500);

        let chapter2 = ChapterMark {
            index: 1,
            start_secs: 0.0,
        };
        let (start_ms2, _) = chapter_to_millis(&chapter2, None);
        assert_eq!(start_ms2, 0);

        let chapter3 = ChapterMark {
            index: 2,
            start_secs: 123.456,
        };
        let (start_ms3, _) = chapter_to_millis(&chapter3, None);
        assert_eq!(start_ms3, 123456);
    }

    #[test]
    fn test_chapter_end_uses_next_chapter() {
        let chapters = vec![
            ChapterMark {
                index: 0,
                start_secs: 0.0,
            },
            ChapterMark {
                index: 1,
                start_secs: 300.0,
            },
            ChapterMark {
                index: 2,
                start_secs: 600.0,
            },
        ];
        let ends = compute_chapter_ends(&chapters, 900.0);

        // First chapter ends 1ms before second chapter starts
        assert_eq!(ends[0], 299999);
        // Second chapter ends 1ms before third chapter starts
        assert_eq!(ends[1], 599999);
        // Last chapter ends at total duration
        assert_eq!(ends[2], 900000);
    }

    #[test]
    fn test_chapter_ends_single() {
        let chapters = vec![ChapterMark {
            index: 0,
            start_secs: 0.0,
        }];
        let ends = compute_chapter_ends(&chapters, 1800.5);

        assert_eq!(ends.len(), 1);
        assert_eq!(ends[0], 1800500);
    }

    #[test]
    fn test_stream_selection_all_maps_everything() {
        let info = StreamInfo::default();
        let result = select_streams(&StreamSelection::All, &info, 5);
        assert_eq!(result, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_stream_selection_prefer_surround() {
        // Simulate: stream 0 = video, 1 = surround audio, 2 = stereo audio, 3 = subtitle
        let info = StreamInfo {
            audio_streams: vec![
                AudioStream {
                    index: 1,
                    codec: "truehd".into(),
                    channels: 8,
                    channel_layout: "7.1".into(),
                    language: Some("eng".into()),
                    profile: None,
                },
                AudioStream {
                    index: 2,
                    codec: "aac".into(),
                    channels: 2,
                    channel_layout: "stereo".into(),
                    language: Some("eng".into()),
                    profile: None,
                },
            ],
            subtitle_count: 1,
        };

        let result = select_streams(&StreamSelection::PreferSurround, &info, 4);
        // Should include: 0 (video), 1 (surround), 2 (stereo), 3 (subtitle)
        assert_eq!(result, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_stream_selection_prefer_surround_no_surround() {
        // Only stereo audio available
        let info = StreamInfo {
            audio_streams: vec![AudioStream {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                channel_layout: "stereo".into(),
                language: None,
                profile: None,
            }],
            subtitle_count: 0,
        };

        let result = select_streams(&StreamSelection::PreferSurround, &info, 2);
        // Should include: 0 (video), 1 (first audio as fallback)
        assert_eq!(result, vec![0, 1]);
    }

    #[test]
    fn test_stream_selection_manual() {
        let info = StreamInfo::default();
        let result = select_streams(&StreamSelection::Manual(vec![0, 2, 4]), &info, 5);
        assert_eq!(result, vec![0, 2, 4]);
    }

    #[test]
    fn test_chapter_ends_empty() {
        let ends = compute_chapter_ends(&[], 100.0);
        assert!(ends.is_empty());
    }

    #[test]
    fn test_chapter_to_millis_fractional() {
        let chapter = ChapterMark {
            index: 0,
            start_secs: 0.001,
        };
        let (start_ms, _) = chapter_to_millis(&chapter, None);
        assert_eq!(start_ms, 1);
    }
}
