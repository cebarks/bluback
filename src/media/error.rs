use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
#[allow(dead_code)] // Public API — variants will be constructed when media module is consumed directly
pub enum MediaError {
    /// AACS host certificate revoked (MKBv72+), need per-disc VUK in KEYDB.cfg
    AacsRevoked,
    /// AACS authentication failed (general — USB bridge issues, missing keys, etc.)
    AacsAuthFailed(String),
    /// libbluray/libaacs hung during AACS init (60s timeout exceeded)
    AacsTimeout,
    /// Device path doesn't exist or isn't an optical drive
    DeviceNotFound(String),
    /// Drive present but no disc inserted
    NoDisc,
    /// Requested playlist doesn't exist on disc
    PlaylistNotFound(String),
    /// Playlist has no usable streams
    NoStreams,
    /// Error during packet read/write in remux
    RemuxFailed(String),
    /// Output file already exists
    OutputExists(PathBuf),
    /// User-initiated cancellation via AtomicBool
    Cancelled,
    /// FFmpeg library error (passthrough)
    Ffmpeg(ffmpeg_the_third::Error),
    /// Standard I/O error
    Io(std::io::Error),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AacsRevoked => write!(
                f,
                "AACS host certificate revoked. This disc requires a per-disc VUK in KEYDB.cfg. \
                 If you have MakeMKV with LibreDrive, set aacs_backend = \"libmmbd\"."
            ),
            Self::AacsAuthFailed(msg) => write!(f, "AACS authentication failed: {}", msg),
            Self::AacsTimeout => write!(
                f,
                "AACS initialization timed out (60s). If libmmbd is installed, verify makemkvcon \
                 is available, or set aacs_backend = \"libaacs\" to use plain libaacs."
            ),
            Self::DeviceNotFound(dev) => write!(f, "Device not found: {}", dev),
            Self::NoDisc => write!(f, "No disc in drive"),
            Self::PlaylistNotFound(num) => write!(f, "Playlist {} not found on disc", num),
            Self::NoStreams => write!(f, "No usable streams in playlist"),
            Self::RemuxFailed(msg) => write!(f, "Remux failed: {}", msg),
            Self::OutputExists(path) => {
                write!(f, "Output file already exists: {}", path.display())
            }
            Self::Cancelled => write!(f, "Operation cancelled"),
            Self::Ffmpeg(e) => write!(f, "FFmpeg error: {}", e),
            Self::Io(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for MediaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ffmpeg_the_third::Error> for MediaError {
    fn from(e: ffmpeg_the_third::Error) -> Self {
        Self::Ffmpeg(e)
    }
}

impl From<std::io::Error> for MediaError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Inspect an FFmpeg error to classify AACS-related failures.
/// FFmpeg wraps libbluray/libaacs errors — we match on known substrings.
pub fn classify_aacs_error(err: &ffmpeg_the_third::Error) -> Option<MediaError> {
    let msg = err.to_string().to_lowercase();
    if msg.contains("no valid processing key")
        || msg.contains("processing key")
        || msg.contains("your host key/certificate has been revoked")
    {
        Some(MediaError::AacsRevoked)
    } else if msg.contains("aacs") || msg.contains("libaacs") || msg.contains("bdplus") {
        Some(MediaError::AacsAuthFailed(err.to_string()))
    } else {
        None
    }
}
