pub mod error;
pub mod probe;
pub mod remux;

pub use error::MediaError;
pub use probe::{
    classify_hdr, format_aspect_ratio, format_channel_layout, format_framerate, probe_media_info,
    probe_streams, scan_playlists,
};
pub use remux::{RemuxOptions, StreamSelection};

use std::sync::Once;

static FFMPEG_INIT: Once = Once::new();

/// Initialize FFmpeg libraries. Safe to call multiple times — only runs once.
pub fn ensure_init() {
    FFMPEG_INIT.call_once(|| {
        ffmpeg_the_third::init().expect("Failed to initialize FFmpeg");
    });
}
