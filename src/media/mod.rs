pub mod error;

pub use error::MediaError;

use std::sync::Once;

static FFMPEG_INIT: Once = Once::new();

/// Initialize FFmpeg libraries. Safe to call multiple times — only runs once.
pub fn ensure_init() {
    FFMPEG_INIT.call_once(|| {
        ffmpeg_the_third::init().expect("Failed to initialize FFmpeg");
    });
}
