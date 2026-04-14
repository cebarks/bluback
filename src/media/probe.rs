use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::time::Duration;

use ffmpeg_the_third::codec::profile::Profile;
use ffmpeg_the_third::ffi;
use ffmpeg_the_third::media::Type as MediaType;
use regex::Regex;

use super::{ensure_init, MediaError};
use crate::types::{MediaInfo, Playlist, ProbeCache, StreamInfo};

use crate::util::duration_to_seconds;

/// Timeout for AACS/libbluray operations (seconds).
/// libmmbd + makemkvcon typically completes in 20-35 seconds.
/// If AACS hasn't completed by 60s, it's likely a SCSI hang (USB bridge issue).
#[cfg(target_os = "linux")]
const SCAN_TIMEOUT_SECS: u64 = 60;

/// Open a bluray device with an optional playlist selection via `input_with_dictionary`.
/// Returns the format context for the opened device+playlist.
fn open_bluray(
    device: &str,
    playlist_num: Option<&str>,
) -> Result<ffmpeg_the_third::format::context::Input, MediaError> {
    ensure_init();

    let url = format!("bluray:{}", device);

    match playlist_num {
        Some(num) => {
            let mut opts = ffmpeg_the_third::Dictionary::new();
            opts.set("playlist", num);
            ffmpeg_the_third::format::input_with_dictionary(&url, opts).map_err(|e| {
                match super::error::classify_aacs_error(&e) {
                    Some(me) => me,
                    None => MediaError::Ffmpeg(e),
                }
            })
        }
        None => ffmpeg_the_third::format::input(&url).map_err(|e| {
            match super::error::classify_aacs_error(&e) {
                Some(me) => me,
                None => MediaError::Ffmpeg(e),
            }
        }),
    }
}

/// Scan a Blu-ray device for available playlists, probing full stream info
/// for episode-length playlists (those meeting `min_probe_duration`).
///
/// Returns the playlist list, a probe cache mapping playlist number to
/// `(MediaInfo, StreamInfo)` for every playlist that was successfully probed,
/// and a set of playlist numbers that were pre-classified and skipped.
/// Playlists below `min_probe_duration` are left with zero stream counts and are
/// not in the cache.
///
/// Because the FFmpeg API doesn't expose playlist enumeration directly, libbluray
/// prints playlist info to stderr at AV_LOG_INFO level. We install a custom log
/// callback via `av_log_set_callback` to capture those lines, parse them with a
/// regex, and build `Playlist` structs.
///
/// The open runs in a forked subprocess to isolate the process from kernel
/// D-state hangs (SCSI ioctls through USB bridges can block indefinitely and
/// prevent the process from exiting). On timeout the child is SIGKILL'd and
/// the parent continues cleanly. The child writes captured log lines back
/// through a pipe.
#[allow(clippy::type_complexity)]
pub fn scan_playlists_with_progress(
    device: &str,
    min_probe_duration: u32,
    auto_detect: bool,
    on_progress: Option<&dyn Fn(u64, u64)>,
    on_probe_progress: Option<&dyn Fn(usize, usize, &str)>,
) -> Result<(Vec<Playlist>, ProbeCache, std::collections::HashSet<String>), MediaError> {
    ensure_init();

    let playlist_re = Regex::new(r"playlist (\d+)\.mpls \((\d+:\d+:\d+)\)").expect("valid regex");

    // Capture log lines and parse playlists. Platform-specific isolation:
    // - Linux: fork subprocess to prevent kernel D-state hangs from SCSI ioctls
    // - macOS: scan directly — fork() crashes due to Objective-C runtime, and
    //   macOS IOKit doesn't have the same D-state issue with USB bridges
    let (lines, scan_error) = scan_with_log_capture(device, on_progress)?;

    let mut playlists: Vec<Playlist> = lines
        .iter()
        .filter_map(|line| parse_playlist_log_line(&playlist_re, line))
        .collect();

    if let Some(err_msg) = scan_error {
        if !playlists.is_empty() {
            log::info!("Scan complete: found {} playlists", playlists.len());
            return Ok((playlists, HashMap::new(), std::collections::HashSet::new()));
        }
        return Err(MediaError::AacsAuthFailed(err_msg));
    }

    let skip_set = if auto_detect {
        crate::detection::pre_classify_playlists(&playlists, min_probe_duration)
    } else {
        std::collections::HashSet::new()
    };

    // Probe full stream info for episode-length playlists (above min_probe_duration).
    // This replaces the old count_streams loop — a single probe_playlist call
    // gets both stream counts and full MediaInfo/StreamInfo, avoiding redundant
    // device opens downstream.
    let probe_indices: Vec<usize> = playlists
        .iter()
        .enumerate()
        .filter(|(_, pl)| pl.seconds >= min_probe_duration && !skip_set.contains(&pl.num))
        .map(|(i, _)| i)
        .collect();
    let probe_total = probe_indices.len();
    let mut probe_cache = HashMap::new();
    for (step, pi) in probe_indices.into_iter().enumerate() {
        if let Some(cb) = &on_probe_progress {
            cb(step + 1, probe_total, &playlists[pi].num);
        }
        let num = playlists[pi].num.clone();
        match probe_playlist(device, &num) {
            Ok((media, streams)) => {
                let pl = &mut playlists[pi];
                pl.video_streams = streams.video_streams.len() as u32;
                pl.audio_streams = streams.audio_streams.len() as u32;
                pl.subtitle_streams = streams.subtitle_streams.len() as u32;
                probe_cache.insert(num, (media, streams));
            }
            Err(e) => {
                log::warn!("Failed to probe playlist {}: {}", num, e);
            }
        }
    }

    log::info!("Scan complete: found {} playlists", playlists.len());
    Ok((playlists, probe_cache, skip_set))
}

/// Run the disc scan with log capture. Returns (captured_lines, optional_error_message).
#[cfg(target_os = "macos")]
fn scan_with_log_capture(
    device: &str,
    _on_progress: Option<&dyn Fn(u64, u64)>,
) -> Result<(Vec<String>, Option<String>), MediaError> {
    // Activate thread-local capture buffer
    THREAD_LOG_BUFFER.with(|buf| {
        *buf.borrow_mut() = Some(Vec::new());
    });

    // Set log level and callback (idempotent — same function pointer for all threads)
    ffmpeg_the_third::log::set_level(ffmpeg_the_third::log::Level::Info);
    unsafe {
        ffmpeg_the_third::ffi::av_log_set_callback(Some(log_capture_callback));
    }

    let result = open_bluray(device, None);

    // Collect captured lines and deactivate capture
    let lines: Vec<String> = THREAD_LOG_BUFFER
        .with(|buf| buf.borrow_mut().take().unwrap_or_default())
        .into_iter()
        .filter(|l| !l.is_empty())
        .collect();

    match result {
        Ok(_) => Ok((lines, None)),
        Err(e) => Ok((lines, Some(e.to_string()))),
    }
}

/// Run the disc scan in a forked subprocess for D-state isolation.
#[cfg(target_os = "linux")]
fn scan_with_log_capture(
    device: &str,
    on_progress: Option<&dyn Fn(u64, u64)>,
) -> Result<(Vec<String>, Option<String>), MediaError> {
    let mut pipe_fds = [0i32; 2];
    if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
        return Err(MediaError::AacsAuthFailed("pipe creation failed".into()));
    }
    unsafe {
        libc::fcntl(pipe_fds[0], libc::F_SETFD, libc::FD_CLOEXEC);
        libc::fcntl(pipe_fds[1], libc::F_SETFD, libc::FD_CLOEXEC);
    }
    let (pipe_read, pipe_write) = (pipe_fds[0], pipe_fds[1]);

    let child_pid = unsafe { libc::fork() };
    if child_pid < 0 {
        unsafe {
            libc::close(pipe_read);
            libc::close(pipe_write);
        }
        return Err(MediaError::AacsAuthFailed("fork failed".into()));
    }

    if child_pid == 0 {
        // === CHILD PROCESS ===
        // New process group so parent can kill makemkvcon along with us
        unsafe { libc::setpgid(0, 0) };
        unsafe { libc::close(pipe_read) };

        THREAD_LOG_BUFFER.with(|buf| {
            *buf.borrow_mut() = Some(Vec::new());
        });
        ffmpeg_the_third::log::set_level(ffmpeg_the_third::log::Level::Info);
        unsafe {
            ffmpeg_the_third::ffi::av_log_set_callback(Some(log_capture_callback));
        }

        let result = open_bluray(device, None);

        let lines: Vec<String> =
            THREAD_LOG_BUFFER.with(|buf| buf.borrow_mut().take().unwrap_or_default());
        let mut buf = String::new();
        for line in lines.iter() {
            buf.push_str(line);
            buf.push('\n');
        }

        let status = if result.is_ok() { 0u8 } else { 1u8 };
        buf.push(status as char);

        if let Err(ref e) = result {
            buf.push_str(&e.to_string());
        }

        let bytes = buf.as_bytes();
        unsafe {
            libc::write(
                pipe_write,
                bytes.as_ptr() as *const libc::c_void,
                bytes.len(),
            );
            libc::close(pipe_write);
            libc::_exit(0);
        }
    }

    // === PARENT PROCESS ===
    unsafe { libc::close(pipe_write) };
    crate::aacs::register_scan_pgid(child_pid);

    let poll_interval = Duration::from_secs(5);
    let start = std::time::Instant::now();
    let child_result = loop {
        let mut status = 0i32;
        let ret = unsafe { libc::waitpid(child_pid, &mut status, libc::WNOHANG) };
        if ret > 0 {
            break read_pipe_to_string(pipe_read);
        }

        std::thread::sleep(poll_interval);
        let elapsed = start.elapsed().as_secs();
        if elapsed >= SCAN_TIMEOUT_SECS {
            // Kill entire process group (child + makemkvcon)
            unsafe {
                libc::kill(-child_pid, libc::SIGKILL);
            }
            unsafe { libc::close(pipe_read) };
            return Err(MediaError::AacsTimeout);
        }
        if let Some(cb) = on_progress {
            cb(elapsed, SCAN_TIMEOUT_SECS);
        }
    };
    unsafe { libc::close(pipe_read) };

    // Kill any remaining processes in the child's process group (e.g., makemkvcon).
    // Must be SIGKILL — makemkvcon may ignore SIGTERM, and since the child has
    // exited these orphaned processes can't be found by kill_makemkvcon_children().
    unsafe { libc::kill(-child_pid, libc::SIGKILL) };

    let (lines_str, status, error_msg) = parse_child_output(&child_result);

    let lines: Vec<String> = lines_str
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();

    let scan_error = if status != 0 {
        Some(error_msg.unwrap_or_else(|| "AACS authentication failed".into()))
    } else {
        None
    };

    Ok((lines, scan_error))
}

#[cfg(target_os = "linux")]
fn read_pipe_to_string(fd: i32) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len()) };
        if n <= 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n as usize]);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(target_os = "linux")]
fn parse_child_output(data: &str) -> (&str, u8, Option<String>) {
    // Format: log lines (newline-separated), then status byte (0=ok, 1=err),
    // then optional error message.
    // Find whichever of \0 or \x01 appears last — the status byte is always
    // appended after all log content.
    let pos_zero = data.rfind('\0');
    let pos_one = data.rfind('\x01');

    let (pos, status) = match (pos_zero, pos_one) {
        (Some(z), Some(o)) => {
            if z > o {
                (z, 0u8)
            } else {
                (o, 1u8)
            }
        }
        (Some(z), None) => (z, 0),
        (None, Some(o)) => (o, 1),
        (None, None) => {
            return (
                data,
                1,
                Some("Child process terminated unexpectedly".into()),
            );
        }
    };

    let error_msg = if status != 0 && pos + 1 < data.len() {
        Some(data[pos + 1..].to_string())
    } else {
        None
    };

    (&data[..pos], status, error_msg)
}

// Thread-local log capture buffer. When `Some`, the log callback stores
// formatted lines here. When `None`, log output is silently discarded.
// This enables parallel disc scans — each thread captures its own log output
// independently since FFmpeg's `av_log()` runs synchronously on the calling thread.
thread_local! {
    static THREAD_LOG_BUFFER: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

/// Shared implementation for the log capture callback.
///
/// Formats the log message via `av_log_format_line2` and stores it in
/// the global `LOG_CAPTURE_LINES` buffer. The `vl` parameter is passed
/// through opaquely — its concrete type differs by architecture.
///
/// SAFETY: `vl` must be valid for `av_log_format_line2`. Called from the
/// arch-specific `log_capture_callback` trampolines below.
macro_rules! log_capture_body {
    ($avcl:expr, $level:expr, $fmt:expr, $vl:expr) => {{
        if $level > ffi::AV_LOG_INFO {
            return;
        }

        let mut buf = [0u8; 1024];
        let mut print_prefix: std::ffi::c_int = 1;

        let len = ffi::av_log_format_line2(
            $avcl,
            $level,
            $fmt,
            $vl,
            buf.as_mut_ptr() as *mut std::ffi::c_char,
            buf.len() as std::ffi::c_int,
            &mut print_prefix,
        );

        if len > 0 {
            let len = (len as usize).min(buf.len().saturating_sub(1));
            if let Ok(s) = std::str::from_utf8(&buf[..len]) {
                THREAD_LOG_BUFFER.with(|buf| {
                    if let Ok(mut borrow) = buf.try_borrow_mut() {
                        if let Some(ref mut lines) = *borrow {
                            lines.push(s.to_string());
                        }
                    }
                });
                log::debug!(target: "ffmpeg", "{}", s.trim());
            }
        }
    }};
}

/// C-compatible log callback — x86_64 variant.
///
/// On x86_64, va_list is `[__va_list_tag; 1]` which decays to `*mut __va_list_tag`
/// in function parameters.
#[cfg(target_arch = "x86_64")]
unsafe extern "C" fn log_capture_callback(
    avcl: *mut std::ffi::c_void,
    level: std::ffi::c_int,
    fmt: *const std::ffi::c_char,
    vl: *mut ffi::__va_list_tag,
) {
    log_capture_body!(avcl, level, fmt, vl);
}

/// C-compatible log callback — non-x86_64 variant.
///
/// On aarch64 and other architectures, va_list is an opaque type passed by value
/// (e.g., `__BindgenOpaqueArray<u64, 4>` on aarch64).
#[cfg(not(target_arch = "x86_64"))]
unsafe extern "C" fn log_capture_callback(
    avcl: *mut std::ffi::c_void,
    level: std::ffi::c_int,
    fmt: *const std::ffi::c_char,
    vl: ffi::va_list,
) {
    log_capture_body!(avcl, level, fmt, vl);
}

/// Parse a single log line looking for playlist info from libbluray.
fn parse_playlist_log_line(re: &Regex, line: &str) -> Option<Playlist> {
    let caps = re.captures(line)?;
    let num = caps[1].to_string();
    let duration = caps[2].to_string();
    let seconds = duration_to_seconds(&duration);
    Some(Playlist {
        num,
        duration,
        seconds,
        video_streams: 0,
        audio_streams: 0,
        subtitle_streams: 0,
    })
}

/// Probe full media info for a specific playlist on a Blu-ray device.
///
/// Extracts video codec, resolution, HDR status, frame rate, bit depth, profile,
/// and first audio stream info.
#[allow(dead_code)] // Retained for potential future use (e.g., GUI frontend)
pub fn probe_media_info(device: &str, playlist_num: &str) -> Result<MediaInfo, MediaError> {
    let ctx = open_bluray(device, Some(playlist_num))?;

    let mut info = MediaInfo::default();

    // Find video stream
    for stream in ctx.streams() {
        let params = stream.parameters();
        if params.medium() == MediaType::Video {
            let codec_id = params.id();
            info.codec = codec_id.name().to_string();
            info.width = params.width();
            info.height = params.height();
            info.resolution = if info.height > 0 {
                format!("{}p", info.height)
            } else {
                String::new()
            };

            // Aspect ratio from dimensions
            info.aspect_ratio = format_aspect_ratio(info.width, info.height);

            // Frame rate from stream r_frame_rate
            let rate = stream.rate();
            info.framerate = format_framerate((rate.numerator(), rate.denominator()));

            // Bit depth
            let bits_raw = params.bits_per_raw_sample();
            info.bit_depth = if bits_raw > 0 {
                bits_raw.to_string()
            } else {
                let bits_coded = params.bits_per_coded_sample();
                if bits_coded > 0 {
                    bits_coded.to_string()
                } else {
                    String::new()
                }
            };

            // Profile
            let profile_raw = params.profile();
            let profile = Profile::from((codec_id, profile_raw));
            info.profile = format_video_profile(profile);

            // HDR detection from color transfer characteristic
            let color_trc = params.color_transfer_characteristic();
            let color_transfer_str = color_trc.name().unwrap_or("").to_string();

            // Side data for Dolby Vision / HDR10+ detection
            // FFmpeg 8.0 moved side data off streams, so we check via codec params
            // For now, use the color transfer to classify HDR type.
            // Dolby Vision and HDR10+ detection from stream side data requires
            // unsafe access to the raw AVStream pointer.
            let side_data_types = extract_side_data_types(&stream);
            let side_data_refs: Vec<&str> = side_data_types.iter().map(|s| s.as_str()).collect();
            info.hdr = classify_hdr(&color_transfer_str, &side_data_refs);

            break;
        }
    }

    // Find first audio stream
    for stream in ctx.streams() {
        let params = stream.parameters();
        if params.medium() == MediaType::Audio {
            let codec_id = params.id();
            let codec_name = codec_id.name().to_string();

            // For DTS, use the profile as the display name if available
            let profile_raw = params.profile();
            let profile = Profile::from((codec_id, profile_raw));
            info.audio = match &profile {
                Profile::DTS(dts) => format_dts_profile(dts).to_string(),
                _ => codec_name,
            };

            let ch_layout = params.ch_layout();
            let channels = ch_layout.channels() as u16;
            let layout_desc = ch_layout.description();
            info.channels = format_channel_layout(channels, &layout_desc);

            info.audio_lang = stream
                .metadata()
                .get("language")
                .map(|s| s.to_string())
                .unwrap_or_default();

            break;
        }
    }

    // Bitrate from format context
    let bitrate = ctx.bit_rate();
    info.bitrate_bps = if bitrate > 0 { bitrate as u64 } else { 0 };

    Ok(info)
}

/// Probe both media info and detailed stream info from a single context open.
pub fn probe_playlist(
    device: &str,
    playlist_num: &str,
) -> Result<(MediaInfo, StreamInfo), MediaError> {
    let mut guard = crate::aacs::MakemkvconGuard::new();
    let ctx = guard.track_open(|| open_bluray(device, Some(playlist_num)))?;

    let mut media_info = MediaInfo::default();
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

                // First video populates MediaInfo
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

                // First audio populates MediaInfo
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

                // TODO: extract AV_DISPOSITION_FORCED when API available
                // ffmpeg-the-third may not expose disposition directly
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

    // Bitrate
    let bitrate = ctx.bit_rate();
    media_info.bitrate_bps = if bitrate > 0 { bitrate as u64 } else { 0 };

    let stream_info = StreamInfo {
        video_streams,
        audio_streams,
        subtitle_streams,
    };

    Ok((media_info, stream_info))
}

/// Extract side data type names from a stream via raw FFI.
///
/// Checks both the AVStream side_data (FFmpeg < 8.0) and
/// AVCodecParameters.coded_side_data (FFmpeg >= 8.0) for maximum
/// compatibility. Whichever fields exist in the current build will
/// be non-null; the other will be null/zero.
///
/// SAFETY: Accesses raw AVStream and AVCodecParameters pointers. These are
/// valid for the lifetime of the format context that owns the stream.
fn extract_side_data_types(stream: &ffmpeg_the_third::Stream) -> Vec<String> {
    let mut types = Vec::new();

    unsafe {
        let stream_ptr = stream.as_ptr();

        // AVStream.nb_side_data + AVStream.side_data (FFmpeg < 7.0)
        #[cfg(feature = "ff_api_avstream_side_data")]
        {
            let nb_stream_sd = (*stream_ptr).nb_side_data;
            let stream_sd = (*stream_ptr).side_data;
            if nb_stream_sd > 0 && !stream_sd.is_null() {
                for i in 0..nb_stream_sd as usize {
                    let sd = &*stream_sd.add(i);
                    let name_ptr = ffi::av_packet_side_data_name(sd.type_);
                    if !name_ptr.is_null() {
                        if let Ok(name) = std::ffi::CStr::from_ptr(name_ptr).to_str() {
                            types.push(name.to_string());
                        }
                    }
                }
            }
        }

        // AVCodecParameters.coded_side_data (8.0+)
        let params = (*stream_ptr).codecpar;
        if !params.is_null() {
            let nb_coded = (*params).nb_coded_side_data;
            let coded_sd = (*params).coded_side_data;
            if nb_coded > 0 && !coded_sd.is_null() {
                for i in 0..nb_coded as usize {
                    let sd = &*coded_sd.add(i);
                    let name_ptr = ffi::av_packet_side_data_name(sd.type_);
                    if !name_ptr.is_null() {
                        if let Ok(name) = std::ffi::CStr::from_ptr(name_ptr).to_str() {
                            types.push(name.to_string());
                        }
                    }
                }
            }
        }
    }

    types
}

// --- Pure helper functions (testable without hardware) ---

/// Classify HDR type from color transfer characteristic name and side data types.
///
/// Matches the same logic as the existing `disc.rs` `parse_media_info_json`, but
/// works from the API values rather than JSON strings.
pub fn classify_hdr(color_transfer: &str, side_data_types: &[&str]) -> String {
    let has_dovi = side_data_types
        .iter()
        .any(|s| s.contains("DOVI") || s.contains("Dolby Vision"));

    let has_hdr10plus = side_data_types
        .iter()
        .any(|s| s.contains("HDR Dynamic Metadata SMPTE2094-40") || s.contains("SMPTE2094"));

    if color_transfer == "smpte2084" {
        if has_dovi {
            "DV".to_string()
        } else if has_hdr10plus {
            "HDR10+".to_string()
        } else {
            "HDR10".to_string()
        }
    } else if color_transfer == "arib-std-b67" {
        "HLG".to_string()
    } else {
        "SDR".to_string()
    }
}

/// Format a channel layout description into a user-friendly string.
///
/// Converts FFmpeg's layout descriptions like "5.1(side)" into "5.1",
/// and handles named layouts like "mono" and "stereo" with numeric equivalents
/// for display consistency.
pub fn format_channel_layout(channels: u16, layout: &str) -> String {
    if layout.is_empty() || layout == "unknown" {
        return match channels {
            1 => "mono".to_string(),
            2 => "stereo".to_string(),
            6 => "5.1".to_string(),
            8 => "7.1".to_string(),
            n if n > 0 => format!("{} channels", n),
            _ => String::new(),
        };
    }

    // Strip the parenthesized suffix (e.g., "5.1(side)" -> "5.1")
    let base = layout.split('(').next().unwrap_or(layout).trim();

    base.to_string()
}

/// Format a rational frame rate as a decimal string (e.g., "23.976").
///
/// Common Blu-ray frame rates:
/// - 24000/1001 = 23.976
/// - 24/1 = 24.000
/// - 30000/1001 = 29.970
/// - 50/1 = 50.000
/// - 60000/1001 = 59.940
pub fn format_framerate(rate: (i32, i32)) -> String {
    let (num, den) = rate;
    if den == 0 || num == 0 {
        return String::new();
    }
    format!("{:.3}", num as f64 / den as f64)
}

/// Compute display aspect ratio from width and height.
///
/// Returns common ratios like "16:9", "4:3", "2.40:1" as strings.
/// Falls back to "W:H" simplified by GCD for uncommon ratios.
pub fn format_aspect_ratio(width: u32, height: u32) -> String {
    if width == 0 || height == 0 {
        return String::new();
    }

    let g = gcd(width, height);
    let w = width / g;
    let h = height / g;

    format!("{}:{}", w, h)
}

/// Greatest common divisor (Euclidean algorithm).
fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Format a codec Profile enum into a human-readable string for video codecs.
fn format_video_profile(profile: Profile) -> String {
    match profile {
        Profile::Unknown | Profile::Reserved => String::new(),
        Profile::H264(p) => format!("{:?}", p),
        Profile::HEVC(p) => match p {
            ffmpeg_the_third::codec::profile::HEVC::Main => "Main".to_string(),
            ffmpeg_the_third::codec::profile::HEVC::Main10 => "Main 10".to_string(),
            ffmpeg_the_third::codec::profile::HEVC::MainStillPicture => {
                "Main Still Picture".to_string()
            }
            ffmpeg_the_third::codec::profile::HEVC::Rext => "Rext".to_string(),
        },
        Profile::AV1(p) => format!("{:?}", p),
        Profile::VP9(p) => format!("Profile {:?}", p),
        // For other codec profiles, use Debug formatting
        other => format!("{:?}", other),
    }
}

/// Format a codec Profile enum for audio codec display.
fn format_codec_profile(profile: Profile) -> Option<String> {
    match profile {
        Profile::Unknown | Profile::Reserved => None,
        Profile::DTS(dts) => Some(format_dts_profile(&dts).to_string()),
        Profile::TrueHD_Atmos => Some("Atmos".to_string()),
        Profile::EAC3_DDP_Atmos => Some("Atmos".to_string()),
        other => Some(format!("{:?}", other)),
    }
}

/// Map DTS profile variants to the display names used in media info.
fn format_dts_profile(dts: &ffmpeg_the_third::codec::profile::DTS) -> &'static str {
    use ffmpeg_the_third::codec::profile::DTS;
    match dts {
        DTS::Default => "dts",
        DTS::ES => "dts-es",
        DTS::_96_24 => "dts 96/24",
        DTS::HD_HRA => "dts-hd hra",
        DTS::HD_MA => "dts-hd ma",
        DTS::Express => "dts express",
        DTS::HD_MA_X => "dts-hd ma x",
        DTS::HD_MA_X_IMAX => "dts-hd ma x imax",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- classify_hdr tests ---

    #[test]
    fn test_classify_hdr_sdr() {
        assert_eq!(classify_hdr("bt709", &[]), "SDR");
        assert_eq!(classify_hdr("", &[]), "SDR");
        assert_eq!(classify_hdr("bt2020_10", &[]), "SDR");
    }

    #[test]
    fn test_classify_hdr_hdr10() {
        assert_eq!(classify_hdr("smpte2084", &[]), "HDR10");
    }

    #[test]
    fn test_classify_hdr_dolby_vision() {
        assert_eq!(
            classify_hdr("smpte2084", &["DOVI configuration record"]),
            "DV"
        );
        assert_eq!(classify_hdr("smpte2084", &["Dolby Vision metadata"]), "DV");
    }

    #[test]
    fn test_classify_hdr_hdr10plus() {
        assert_eq!(
            classify_hdr("smpte2084", &["HDR Dynamic Metadata SMPTE2094-40"]),
            "HDR10+"
        );
    }

    #[test]
    fn test_classify_hdr_dv_takes_priority_over_hdr10plus() {
        // If both DV and HDR10+ side data are present, DV wins
        assert_eq!(
            classify_hdr(
                "smpte2084",
                &[
                    "DOVI configuration record",
                    "HDR Dynamic Metadata SMPTE2094-40"
                ]
            ),
            "DV"
        );
    }

    #[test]
    fn test_classify_hdr_hlg() {
        assert_eq!(classify_hdr("arib-std-b67", &[]), "HLG");
    }

    // --- format_channel_layout tests ---

    #[test]
    fn test_format_channel_layout_51_side() {
        assert_eq!(format_channel_layout(6, "5.1(side)"), "5.1");
    }

    #[test]
    fn test_format_channel_layout_71() {
        assert_eq!(format_channel_layout(8, "7.1"), "7.1");
    }

    #[test]
    fn test_format_channel_layout_stereo() {
        assert_eq!(format_channel_layout(2, "stereo"), "stereo");
    }

    #[test]
    fn test_format_channel_layout_mono() {
        assert_eq!(format_channel_layout(1, "mono"), "mono");
    }

    #[test]
    fn test_format_channel_layout_empty_fallback() {
        assert_eq!(format_channel_layout(6, ""), "5.1");
        assert_eq!(format_channel_layout(8, ""), "7.1");
        assert_eq!(format_channel_layout(2, ""), "stereo");
        assert_eq!(format_channel_layout(1, ""), "mono");
        assert_eq!(format_channel_layout(4, ""), "4 channels");
    }

    #[test]
    fn test_format_channel_layout_unknown() {
        assert_eq!(format_channel_layout(0, ""), "");
        assert_eq!(format_channel_layout(0, "unknown"), "");
    }

    // --- format_framerate tests ---

    #[test]
    fn test_format_framerate_23976() {
        assert_eq!(format_framerate((24000, 1001)), "23.976");
    }

    #[test]
    fn test_format_framerate_24() {
        assert_eq!(format_framerate((24, 1)), "24.000");
    }

    #[test]
    fn test_format_framerate_50() {
        assert_eq!(format_framerate((50, 1)), "50.000");
    }

    #[test]
    fn test_format_framerate_29970() {
        assert_eq!(format_framerate((30000, 1001)), "29.970");
    }

    #[test]
    fn test_format_framerate_zero_den() {
        assert_eq!(format_framerate((24, 0)), "");
    }

    #[test]
    fn test_format_framerate_zero_num() {
        assert_eq!(format_framerate((0, 1)), "");
    }

    // --- format_aspect_ratio tests ---

    #[test]
    fn test_format_aspect_ratio_16_9() {
        assert_eq!(format_aspect_ratio(1920, 1080), "16:9");
    }

    #[test]
    fn test_format_aspect_ratio_4_3() {
        assert_eq!(format_aspect_ratio(1440, 1080), "4:3");
    }

    #[test]
    fn test_format_aspect_ratio_uhd() {
        assert_eq!(format_aspect_ratio(3840, 2160), "16:9");
    }

    #[test]
    fn test_format_aspect_ratio_zero() {
        assert_eq!(format_aspect_ratio(0, 1080), "");
        assert_eq!(format_aspect_ratio(1920, 0), "");
        assert_eq!(format_aspect_ratio(0, 0), "");
    }

    // --- parse_playlist_log_line tests ---

    #[test]
    fn test_parse_playlist_log_line_valid() {
        let re = Regex::new(r"playlist (\d+)\.mpls \((\d+:\d+:\d+)\)").unwrap();
        let line = "[bluray @ 0x5f3c] playlist 00001.mpls (0:43:42)";
        let pl = parse_playlist_log_line(&re, line).unwrap();
        assert_eq!(pl.num, "00001");
        assert_eq!(pl.duration, "0:43:42");
        assert_eq!(pl.seconds, 2622);
    }

    #[test]
    fn test_parse_playlist_log_line_no_match() {
        let re = Regex::new(r"playlist (\d+)\.mpls \((\d+:\d+:\d+)\)").unwrap();
        let line = "[bluray @ 0x5f3c] Opening disc...";
        assert!(parse_playlist_log_line(&re, line).is_none());
    }

    #[test]
    fn test_parse_playlist_log_line_long_duration() {
        let re = Regex::new(r"playlist (\d+)\.mpls \((\d+:\d+:\d+)\)").unwrap();
        let line = "playlist 00042.mpls (2:15:30)";
        let pl = parse_playlist_log_line(&re, line).unwrap();
        assert_eq!(pl.num, "00042");
        assert_eq!(pl.seconds, 8130);
    }

    // --- gcd tests ---

    #[test]
    fn test_gcd_basic() {
        assert_eq!(gcd(1920, 1080), 120);
        assert_eq!(gcd(3840, 2160), 240);
        assert_eq!(gcd(1440, 1080), 360);
    }

    #[test]
    fn test_gcd_coprime() {
        assert_eq!(gcd(17, 13), 1);
    }

    #[test]
    fn test_gcd_same() {
        assert_eq!(gcd(100, 100), 100);
    }

    // --- DTS profile formatting ---

    #[test]
    fn test_format_dts_profile_hd_ma() {
        use ffmpeg_the_third::codec::profile::DTS;
        assert_eq!(format_dts_profile(&DTS::HD_MA), "dts-hd ma");
    }

    #[test]
    fn test_format_dts_profile_default() {
        use ffmpeg_the_third::codec::profile::DTS;
        assert_eq!(format_dts_profile(&DTS::Default), "dts");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_child_output_success() {
        let data = "log line 1\nlog line 2\n\0";
        let (lines, status, error) = parse_child_output(data);
        assert_eq!(status, 0);
        assert!(error.is_none());
        assert_eq!(lines, "log line 1\nlog line 2\n");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_child_output_error() {
        let data = "log line\n\x01AACS failed";
        let (lines, status, error) = parse_child_output(data);
        assert_eq!(status, 1);
        assert_eq!(error, Some("AACS failed".to_string()));
        assert_eq!(lines, "log line\n");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_child_output_embedded_nul_with_error() {
        let data = "log with \0 embedded\n\x01AACS failed";
        let (_lines, status, error) = parse_child_output(data);
        assert_eq!(status, 1, "should detect error status, not embedded NUL");
        assert_eq!(error, Some("AACS failed".to_string()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_child_output_embedded_nul_with_success() {
        let data = "log with \0 embedded\n\0";
        let (_lines, status, error) = parse_child_output(data);
        assert_eq!(status, 0);
        assert!(error.is_none());
    }
}
