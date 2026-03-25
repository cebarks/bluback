use std::sync::mpsc;
use std::time::Duration;

use ffmpeg_the_third::codec::profile::Profile;
use ffmpeg_the_third::ffi;
use ffmpeg_the_third::media::Type as MediaType;
use regex::Regex;

use super::{ensure_init, MediaError};
use crate::types::{AudioStream, MediaInfo, Playlist, StreamInfo};
use crate::util::duration_to_seconds;

/// Timeout for AACS/libbluray operations (seconds).
/// libmmbd + makemkvcon can take 90+ seconds for initial disc scan,
/// so this needs to be generous.
const SCAN_TIMEOUT_SECS: u64 = 120;

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
            ffmpeg_the_third::format::input_with_dictionary(&url, opts)
                .map_err(|e| match super::error::classify_aacs_error(&e) {
                    Some(me) => me,
                    None => MediaError::Ffmpeg(e),
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

/// Scan a Blu-ray device for available playlists.
///
/// Because the FFmpeg API doesn't expose playlist enumeration directly, libbluray
/// prints playlist info to stderr at AV_LOG_INFO level. We install a custom log
/// callback via `av_log_set_callback` to capture those lines, parse them with a
/// regex, and build `Playlist` structs.
///
/// The open is done in a spawned thread with a 60-second timeout to protect
/// against AACS hangs.
pub fn scan_playlists(device: &str) -> Result<Vec<Playlist>, MediaError> {
    ensure_init();

    let playlist_re =
        Regex::new(r"playlist (\d+)\.mpls \((\d+:\d+:\d+)\)").expect("valid regex");

    // Install a custom log callback that captures log lines into a global buffer.
    // We use a Mutex<Vec<String>> to collect lines from the callback thread-safely.
    // The callback uses av_log_format_line2 to render the format+va_list into a string.
    let captured_lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let callback_lines = std::sync::Arc::clone(&captured_lines);

    // Temporarily raise log level to INFO to capture libbluray playlist output
    ffmpeg_the_third::log::set_level(ffmpeg_the_third::log::Level::Info);

    // Install custom log callback via FFI.
    // The callback receives (avcl, level, fmt, vl) and uses av_log_format_line2
    // to format the message into a buffer, then pushes it to our Vec.
    //
    // SAFETY: av_log_set_callback is thread-safe per FFmpeg docs. The callback
    // itself must be thread-safe, which we achieve via a global Mutex.
    // We store the Arc in a global so the callback can access it.
    {
        let mut guard = LOG_CAPTURE_LINES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(callback_lines);
    }

    unsafe {
        ffmpeg_the_third::ffi::av_log_set_callback(Some(log_capture_callback));
    }

    // Spawn the format open in a thread with timeout
    let device_owned = device.to_string();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let result = open_bluray(&device_owned, None);
        // Drop the context immediately -- we only needed the open to trigger log output
        let _ = tx.send(result.map(|_ctx| ()));
    });

    let open_result = rx
        .recv_timeout(Duration::from_secs(SCAN_TIMEOUT_SECS))
        .map_err(|_| MediaError::AacsTimeout)?;

    // Set log level to quiet to suppress libbluray noise on subsequent calls.
    // The default callback prints to stderr which corrupts TUI mode.
    unsafe {
        ffmpeg_the_third::ffi::av_log_set_callback(Some(
            ffmpeg_the_third::ffi::av_log_default_callback,
        ));
    }
    ffmpeg_the_third::log::set_level(ffmpeg_the_third::log::Level::Quiet);

    // Grab captured lines
    let lines = {
        let mut guard = LOG_CAPTURE_LINES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let arc = guard.take();
        arc.map(|a| {
            let inner = a.lock().unwrap_or_else(|p| p.into_inner());
            inner.clone()
        })
        .unwrap_or_default()
    };

    // If the open itself failed, check whether we got playlists from the logs anyway.
    // libbluray logs playlist info before AACS errors sometimes.
    if let Err(ref e) = open_result {
        // Parse what we got before returning the error
        let playlists: Vec<Playlist> = lines
            .iter()
            .filter_map(|line| parse_playlist_log_line(&playlist_re, line))
            .collect();

        if !playlists.is_empty() {
            return Ok(playlists);
        }
        return Err(match e {
            MediaError::AacsRevoked => MediaError::AacsRevoked,
            MediaError::AacsAuthFailed(msg) => MediaError::AacsAuthFailed(msg.clone()),
            MediaError::AacsTimeout => MediaError::AacsTimeout,
            _ => MediaError::AacsAuthFailed(e.to_string()),
        });
    }

    let playlists: Vec<Playlist> = lines
        .iter()
        .filter_map(|line| parse_playlist_log_line(&playlist_re, line))
        .collect();

    Ok(playlists)
}

/// Global storage for the log capture callback's output buffer.
/// This is a Mutex<Option<Arc<Mutex<Vec<String>>>>> so the C callback can access it.
static LOG_CAPTURE_LINES: std::sync::Mutex<Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>> =
    std::sync::Mutex::new(None);

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
            let len = (len as usize).min(buf.len());
            if let Ok(s) = std::str::from_utf8(&buf[..len]) {
                if let Ok(guard) = LOG_CAPTURE_LINES.lock() {
                    if let Some(ref arc) = *guard {
                        if let Ok(mut lines) = arc.lock() {
                            lines.push(s.to_string());
                        }
                    }
                }
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
    })
}

/// Probe stream information for a specific playlist on a Blu-ray device.
///
/// Opens the device with the given playlist number and iterates streams to build
/// `AudioStream` entries and count subtitle streams.
#[allow(dead_code)] // Public API — used when media module is consumed directly
pub fn probe_streams(device: &str, playlist_num: &str) -> Result<StreamInfo, MediaError> {
    let ctx = open_bluray(device, Some(playlist_num))?;

    let mut audio_streams = Vec::new();
    let mut subtitle_count = 0u32;

    for stream in ctx.streams() {
        let params = stream.parameters();
        match params.medium() {
            MediaType::Audio => {
                let codec_id = params.id();
                let codec_name = codec_id.name().to_string();

                // Channel layout and count
                let ch_layout = params.ch_layout();
                let channels = ch_layout.channels() as u16;
                let layout_desc = ch_layout.description();
                let channel_layout = format_channel_layout(channels, &layout_desc);

                // Language from stream metadata
                let language = stream.metadata().get("language").map(|s| s.to_string());

                // Profile
                let profile_raw = params.profile();
                let profile = format_codec_profile(Profile::from((codec_id, profile_raw)));

                audio_streams.push(AudioStream {
                    index: stream.index(),
                    codec: codec_name,
                    channels,
                    channel_layout,
                    language,
                    profile,
                });
            }
            MediaType::Subtitle => {
                subtitle_count += 1;
            }
            _ => {}
        }
    }

    Ok(StreamInfo {
        audio_streams,
        subtitle_count,
    })
}

/// Probe full media info for a specific playlist on a Blu-ray device.
///
/// Extracts video codec, resolution, HDR status, frame rate, bit depth, profile,
/// and first audio stream info.
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
            info.framerate =
                format_framerate((rate.numerator(), rate.denominator()));

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
            let color_transfer_str = color_trc
                .name()
                .unwrap_or("")
                .to_string();

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

        // AVStream.nb_side_data + AVStream.side_data (pre-8.0)
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

    let has_hdr10plus = side_data_types.iter().any(|s| {
        s.contains("HDR Dynamic Metadata SMPTE2094-40") || s.contains("SMPTE2094")
    });

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
#[allow(dead_code)] // Called by probe_streams which is dead_code-allowed public API
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
        assert_eq!(
            classify_hdr("smpte2084", &["Dolby Vision metadata"]),
            "DV"
        );
    }

    #[test]
    fn test_classify_hdr_hdr10plus() {
        assert_eq!(
            classify_hdr(
                "smpte2084",
                &["HDR Dynamic Metadata SMPTE2094-40"]
            ),
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
}
