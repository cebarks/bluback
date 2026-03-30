pub mod error;
pub mod probe;
pub mod remux;

pub use error::MediaError;
#[allow(unused_imports)]
pub use probe::{probe_media_info, probe_playlist, scan_playlists_with_progress};
pub use remux::{RemuxOptions, StreamSelection};

use std::sync::Once;

static FFMPEG_INIT: Once = Once::new();

/// Initialize FFmpeg libraries. Safe to call multiple times — only runs once.
pub fn ensure_init() {
    FFMPEG_INIT.call_once(|| {
        ffmpeg_the_third::init().expect("Failed to initialize FFmpeg");
        // Suppress FFmpeg log output by default — libbluray is noisy and
        // the default callback prints to stderr which corrupts TUI mode.
        // scan_playlists temporarily raises the level to capture playlist info.
        ffmpeg_the_third::log::set_level(ffmpeg_the_third::log::Level::Quiet);
    });
}
