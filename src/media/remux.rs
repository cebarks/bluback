use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use ffmpeg_the_third::{
    self as ffmpeg, codec, format, media, Dictionary, Frame, Packet, Rational, Rescale,
};

use super::error::{classify_aacs_error, MediaError};
use crate::types::{ChapterMark, RipProgress};

/// Handles lossless transcoding of Blu-ray-specific PCM formats (PCM_BLURAY,
/// PCM_DVD) to standard PCM codecs that MKV supports. The conversion is
/// bit-exact — same samples, different container encoding.
struct AudioTranscoder {
    decoder: codec::decoder::Audio,
    encoder: codec::encoder::audio::Encoder,
}

impl AudioTranscoder {
    /// Choose a standard PCM codec that matches the bit depth of the source.
    fn target_codec_id(bits: u32) -> codec::Id {
        if bits <= 16 {
            codec::Id::PCM_S16LE
        } else if bits <= 24 {
            codec::Id::PCM_S24LE
        } else {
            codec::Id::PCM_S32LE
        }
    }

    /// Create a transcoder for an input stream, configuring the output stream
    /// with the target codec's parameters.
    fn new(
        in_stream: &ffmpeg::Stream,
        out_stream: &mut format::stream::StreamMut,
    ) -> Result<Self, MediaError> {
        // Decode from the input codec (e.g. PCM_BLURAY)
        let decoder = codec::context::Context::from_parameters(in_stream.parameters())
            .and_then(|ctx| ctx.decoder().audio())
            .map_err(|e| MediaError::RemuxFailed(format!("Failed to open PCM decoder: {}", e)))?;

        // Pick target codec based on bit depth
        let bits = {
            let coded = in_stream.parameters().bits_per_coded_sample();
            let raw_sample = in_stream.parameters().bits_per_raw_sample();
            if raw_sample > 0 {
                raw_sample
            } else if coded > 0 {
                coded
            } else {
                24 // PCM_BLURAY default
            }
        };
        let target_id = Self::target_codec_id(bits);
        let target_codec = codec::encoder::find(target_id).ok_or_else(|| {
            MediaError::RemuxFailed(format!("PCM encoder {:?} not available", target_id))
        })?;

        log::info!(
            "Transcoding stream {} from {:?} to {:?} ({}-bit)",
            in_stream.index(),
            in_stream.parameters().id(),
            target_id,
            bits
        );

        // Configure encoder to match decoder's audio parameters
        let mut enc_audio = codec::context::Context::new_with_codec(target_codec)
            .encoder()
            .audio()
            .map_err(|e| MediaError::RemuxFailed(format!("Failed to create PCM encoder: {}", e)))?;

        enc_audio.set_rate(decoder.rate() as i32);
        enc_audio.set_format(decoder.format());
        enc_audio.set_ch_layout(decoder.ch_layout());
        enc_audio.set_time_base((1, decoder.rate() as i32));

        let encoder = enc_audio
            .open_as(target_codec)
            .map_err(|e| MediaError::RemuxFailed(format!("Failed to open PCM encoder: {}", e)))?;

        // Set output stream parameters from the opened encoder context
        let enc_ctx: &codec::Context = encoder.as_ref();
        out_stream.copy_parameters_from_context(enc_ctx);

        Ok(Self { decoder, encoder })
    }

    /// Decode input packets and re-encode to the target format.
    /// Returns encoded output packets.
    fn transcode(&mut self, input_packet: &Packet) -> Result<Vec<Packet>, MediaError> {
        self.decoder
            .send_packet(input_packet)
            .map_err(|e| MediaError::RemuxFailed(format!("PCM decode error: {}", e)))?;
        self.drain_encoder()
    }

    /// Flush remaining frames at end of stream.
    fn flush(&mut self) -> Result<Vec<Packet>, MediaError> {
        self.decoder
            .send_eof()
            .map_err(|e| MediaError::RemuxFailed(format!("PCM decoder flush error: {}", e)))?;
        let packets = self.drain_encoder()?;
        self.encoder
            .send_eof()
            .map_err(|e| MediaError::RemuxFailed(format!("PCM encoder flush error: {}", e)))?;
        let mut final_packets = packets;
        let mut out_pkt = Packet::empty();
        while self.encoder.receive_packet(&mut out_pkt).is_ok() {
            final_packets.push(out_pkt.clone());
        }
        Ok(final_packets)
    }

    /// Receive decoded frames and feed them to the encoder, collecting output packets.
    fn drain_encoder(&mut self) -> Result<Vec<Packet>, MediaError> {
        let mut output = Vec::new();
        let mut frame = unsafe { Frame::empty() };
        while self.decoder.receive_frame(&mut frame).is_ok() {
            self.encoder
                .send_frame(&frame)
                .map_err(|e| MediaError::RemuxFailed(format!("PCM encode error: {}", e)))?;
            let mut out_pkt = Packet::empty();
            while self.encoder.receive_packet(&mut out_pkt).is_ok() {
                output.push(out_pkt.clone());
            }
        }
        Ok(output)
    }
}

/// How to select streams from the input for remuxing.
#[derive(Debug, Clone, Default)]
pub enum StreamSelection {
    /// Map every stream from the input.
    #[default]
    All,
    /// Exact stream indices provided by the caller.
    Manual(Vec<usize>),
}

/// All parameters needed to perform a single remux operation.
pub struct RemuxOptions {
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
/// - `Manual` passes through exactly the indices the caller specified.
pub fn select_streams(selection: &StreamSelection, total_streams: usize) -> Vec<usize> {
    match selection {
        StreamSelection::All => (0..total_streams).collect(),
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

/// Open a Blu-ray playlist input context and extract media/stream information.
///
/// Returns the opened FFmpeg input context, the MakemkvconGuard (must be kept alive
/// for the duration of the remux), and the extracted media and stream info.
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
    opts.set("probesize", "100000000");
    opts.set("analyzeduration", "100000000");

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

    let (media_info, stream_info) = crate::media::probe::extract_media_and_stream_info(&ictx);

    Ok((ictx, guard, media_info, stream_info))
}

/// Write a remux operation from an opened input context to an MKV file.
///
/// Takes ownership of the input context and guard. Progress is reported via
/// the `on_progress` callback approximately every 100ms.
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
    log::info!("Remux started: output={}", output.display());

    if output.exists() {
        return Err(MediaError::OutputExists(output.to_path_buf()));
    }

    let nb_input_streams = ictx.nb_streams() as usize;

    let selected = select_streams(&options.stream_selection, nb_input_streams);

    if selected.is_empty() {
        return Err(MediaError::NoStreams);
    }

    // Create output context for MKV
    let output_path = output
        .to_str()
        .ok_or_else(|| MediaError::RemuxFailed("Output path contains invalid UTF-8".into()))?;
    let mut octx = format::output(output_path)?;

    // Map from input stream index -> output stream index
    // -1 means not mapped
    let mut stream_map: Vec<i32> = vec![-1; nb_input_streams];

    // Find video dimensions as fallback for PGS subtitles that may lack them
    // (if ffmpeg's probe phase times out before seeing a subtitle packet)
    let mut fallback_width = 1920;
    let mut fallback_height = 1080;
    for in_idx in 0..nb_input_streams {
        if let Some(stream) = ictx.stream(in_idx) {
            if stream.parameters().medium() == media::Type::Video {
                let w = stream.parameters().width();
                let h = stream.parameters().height();
                if w > 0 && h > 0 {
                    fallback_width = w;
                    fallback_height = h;
                    break;
                }
            }
        }
    }

    let mut transcoders: HashMap<usize, AudioTranscoder> = HashMap::new();
    let mut out_idx_counter: i32 = 0;
    for &in_idx in selected.iter() {
        let in_stream = ictx
            .stream(in_idx)
            .ok_or_else(|| MediaError::RemuxFailed(format!("Input stream {} not found", in_idx)))?;

        let codec_id = in_stream.parameters().id();
        let needs_transcode = codec_id == ffmpeg_the_third::codec::Id::PCM_BLURAY
            || codec_id == ffmpeg_the_third::codec::Id::PCM_DVD;

        // Add output stream and copy codec parameters
        let out_idx = out_idx_counter;
        let mut out_stream = if needs_transcode {
            // For Blu-ray PCM codecs, create the output stream with the target
            // codec — AudioTranscoder::new() will set the actual parameters.
            let target_id = AudioTranscoder::target_codec_id(
                in_stream
                    .parameters()
                    .bits_per_raw_sample()
                    .max(in_stream.parameters().bits_per_coded_sample())
                    .max(16),
            );
            octx.add_stream(target_id)?
        } else {
            let mut s = octx.add_stream(codec_id)?;
            s.set_parameters(in_stream.parameters());
            s.set_time_base(in_stream.time_base());
            s
        };

        // Set up lossless transcoder for Blu-ray PCM → standard PCM
        if needs_transcode {
            let transcoder = AudioTranscoder::new(&in_stream, &mut out_stream)?;
            transcoders.insert(in_idx, transcoder);
        }

        let is_pgs = in_stream.parameters().medium() == media::Type::Subtitle
            && in_stream.parameters().id() == ffmpeg_the_third::codec::Id::HDMV_PGS_SUBTITLE;

        // Clear codec tag for container compatibility (equivalent to codec_tag = 0)
        unsafe {
            let st_ptr = out_stream.as_mut_ptr();
            (*(*st_ptr).codecpar).codec_tag = 0;

            // Fix for "Failed to write header: Invalid argument" with PGS subtitles.
            // The MKV muxer internally checks subtitle dimensions; PGS streams
            // read from Blu-ray transport streams frequently have width/height=0
            // because FFmpeg's probe phase doesn't read far enough to encounter
            // the first subtitle display set packet.
            if is_pgs && ((*(*st_ptr).codecpar).width == 0 || (*(*st_ptr).codecpar).height == 0) {
                (*(*st_ptr).codecpar).width = fallback_width as i32;
                (*(*st_ptr).codecpar).height = fallback_height as i32;
                log::info!(
                    "Injected fallback dimensions {}x{} for PGS subtitle stream {}",
                    fallback_width,
                    fallback_height,
                    in_idx
                );
            }
        }

        stream_map[in_idx] = out_idx;
        out_idx_counter = out_idx + 1;
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

    // Blu-ray transport streams often start at a non-zero PTS (e.g. 10+ minutes
    // into the 90kHz clock). Without normalization the MKV container records
    // duration = max_PTS, which includes that offset as empty seekable time.
    // This mirrors what the ffmpeg CLI does via ts_offset = -start_time.
    let input_start_time = unsafe { (*ictx.as_ptr()).start_time };
    let stream_ts_offsets: Vec<i64> =
        if input_start_time > 0 && input_start_time != ffmpeg::ffi::AV_NOPTS_VALUE {
            (0..nb_input_streams)
                .map(|i| {
                    let tb = ictx
                        .stream(i)
                        .map(|s| s.time_base())
                        .unwrap_or(Rational(1, 90000));
                    input_start_time.rescale(ffmpeg::rescale::TIME_BASE, tb)
                })
                .collect()
        } else {
            vec![0; nb_input_streams]
        };

    if input_start_time > 0 && input_start_time != ffmpeg::ffi::AV_NOPTS_VALUE {
        let offset_secs = input_start_time as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE);
        log::info!(
            "Normalizing timestamps: input start_time={:.3}s",
            offset_secs
        );
    }

    // Inject chapters before writing header
    let chapters_added = inject_chapters(&mut octx, &options.chapters, total_duration_secs)?;

    // Inject MKV metadata tags before writing header
    if let Some(ref meta) = options.metadata {
        let mut md = octx.metadata_mut();
        for (k, v) in &meta.tags {
            md.set(k, v);
        }
        log::debug!("Injected {} metadata tag(s)", meta.tags.len());
    }

    // Write output header, reserving void space for the seek index (Cues)
    // so they can be written at the front of the file for faster seeking.
    let reserve_bytes = (options.reserve_index_space_kb as u64) * 1024;
    let mut muxer_opts = Dictionary::new();
    muxer_opts.set("reserve_index_space", reserve_bytes.to_string());

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
            log::warn!("Remux cancelled: {}", output.display());
            return Err(MediaError::Cancelled);
        }

        match packet.read(&mut ictx) {
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => break,
            Err(e) => {
                log::warn!("Remux failed: {}: {}", output.display(), e);
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

        // Normalize timestamps: subtract the input start_time offset so the
        // output MKV starts at PTS ~0 instead of inheriting the Blu-ray
        // transport stream's clock offset.
        let ts_offset = stream_ts_offsets.get(in_stream_idx).copied().unwrap_or(0);
        if ts_offset != 0 {
            if let Some(pts) = packet.pts() {
                packet.set_pts(Some(pts - ts_offset));
            }
            if let Some(dts) = packet.dts() {
                packet.set_dts(Some(dts - ts_offset));
            }
        }

        // Track video frames and content time (after normalization)
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

        // Branch: transcode or stream-copy
        if let Some(transcoder) = transcoders.get_mut(&in_stream_idx) {
            // Transcode: decode PCM_BLURAY/PCM_DVD → re-encode as standard PCM
            let out_packets = transcoder.transcode(&packet)?;
            for mut out_pkt in out_packets {
                out_pkt.rescale_ts(in_time_base, out_time_base);
                out_pkt.set_stream(out_stream_idx);
                out_pkt.write_interleaved(&mut octx).map_err(|e| {
                    log::warn!("Remux failed: {}: {}", output.display(), e);
                    MediaError::RemuxFailed(format!("Error writing transcoded packet: {}", e))
                })?;
            }
        } else {
            // Stream-copy: rescale timestamps and write directly
            packet.rescale_ts(in_time_base, out_time_base);
            packet.set_stream(out_stream_idx);

            // Write packet (interleaved for proper ordering)
            packet.write_interleaved(&mut octx).map_err(|e| {
                log::warn!("Remux failed: {}: {}", output.display(), e);
                MediaError::RemuxFailed(format!("Error writing packet: {}", e))
            })?;
        }

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
                duration_secs: total_duration_secs as u32,
                bitrate,
                speed,
            });
            last_progress = Instant::now();
        }
    }

    // Flush any transcoded streams before writing the trailer
    for (&in_idx, transcoder) in transcoders.iter_mut() {
        let out_stream_idx = stream_map[in_idx] as usize;
        let in_time_base = ictx
            .stream(in_idx)
            .map(|s| s.time_base())
            .unwrap_or(Rational(1, 90000));
        let out_time_base = octx
            .stream(out_stream_idx)
            .map(|s| s.time_base())
            .unwrap_or(Rational(1, 90000));
        let flushed = transcoder.flush()?;
        for mut out_pkt in flushed {
            out_pkt.rescale_ts(in_time_base, out_time_base);
            out_pkt.set_stream(out_stream_idx);
            out_pkt.write_interleaved(&mut octx).map_err(|e| {
                MediaError::RemuxFailed(format!("Error writing flushed packet: {}", e))
            })?;
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
        duration_secs: total_duration_secs as u32,
        bitrate,
        speed,
    });

    log::info!("Remux completed: {}", output.display());
    Ok(chapters_added)
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
        let result = select_streams(&StreamSelection::All, 5);
        assert_eq!(result, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_stream_selection_manual() {
        let result = select_streams(&StreamSelection::Manual(vec![0, 2, 4]), 5);
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
