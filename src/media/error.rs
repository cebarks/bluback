use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
#[allow(dead_code)] // Public API — variants will be constructed when media module is consumed directly
pub enum MediaError {
    /// AACS host certificate revoked (MKBv72+), need per-disc VUK in KEYDB.cfg
    AacsRevoked,
    /// AACS authentication failed (general — USB bridge issues, missing keys, etc.)
    AacsAuthFailed(String),
    /// libbluray/libaacs hung during AACS init (timeout exceeded).
    /// Contains the detected backend name for diagnostic messages.
    AacsTimeout(String),
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
            Self::AacsTimeout(ref backend) => {
                writeln!(f, "AACS initialization timed out (backend: {}).", backend)?;
                match backend.as_str() {
                    "libmmbd" => write!(
                        f,
                        "libmmbd + makemkvcon IPC hung during AACS negotiation.\n\
                         This can happen with USB Blu-ray drives (ASMedia bridge SCSI passthrough issue).\n\
                         Try:\n  \
                           - Power cycle the drive (unplug USB, wait 5s, replug)\n  \
                           - Ensure makemkvcon is registered (beta key or purchased)\n  \
                           - Try aacs_backend = \"libaacs\" if a per-disc VUK is in KEYDB.cfg"
                    ),
                    "libaacs" => write!(
                        f,
                        "libaacs hung during AACS key exchange.\n\
                         This can happen when the host certificate is revoked (MKBv72+ discs)\n\
                         or with USB drives that have SCSI passthrough issues.\n\
                         Try:\n  \
                           - aacs_backend = \"libmmbd\" (requires makemkvcon + registered MakeMKV)\n  \
                           - Add a per-disc VUK to KEYDB.cfg"
                    ),
                    _ => write!(
                        f,
                        "This can happen with USB Blu-ray drives (ASMedia bridge SCSI passthrough issue)\n\
                         or when the AACS host certificate is revoked (MKBv72+ discs).\n\
                         Try:\n  \
                           - aacs_backend = \"libmmbd\" (requires makemkvcon + registered MakeMKV)\n  \
                           - Add a per-disc VUK to KEYDB.cfg"
                    ),
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_aacs_revoked() {
        let msg = format!("{}", MediaError::AacsRevoked);
        assert!(msg.contains("AACS host certificate revoked"));
        assert!(msg.contains("VUK"));
    }

    #[test]
    fn test_display_aacs_auth_failed() {
        let msg = format!("{}", MediaError::AacsAuthFailed("test error".into()));
        assert!(msg.contains("AACS authentication failed"));
        assert!(msg.contains("test error"));
    }

    #[test]
    fn test_display_aacs_timeout_libmmbd() {
        let msg = format!("{}", MediaError::AacsTimeout("libmmbd".into()));
        assert!(msg.contains("timed out"));
        assert!(msg.contains("libmmbd"));
        assert!(msg.contains("makemkvcon"));
    }

    #[test]
    fn test_display_aacs_timeout_libaacs() {
        let msg = format!("{}", MediaError::AacsTimeout("libaacs".into()));
        assert!(msg.contains("timed out"));
        assert!(msg.contains("libaacs"));
    }

    #[test]
    fn test_display_aacs_timeout_unknown() {
        let msg = format!("{}", MediaError::AacsTimeout("auto".into()));
        assert!(msg.contains("timed out"));
        assert!(msg.contains("USB"));
    }

    #[test]
    fn test_display_device_not_found() {
        let msg = format!("{}", MediaError::DeviceNotFound("/dev/sr0".into()));
        assert_eq!(msg, "Device not found: /dev/sr0");
    }

    #[test]
    fn test_display_no_disc() {
        assert_eq!(format!("{}", MediaError::NoDisc), "No disc in drive");
    }

    #[test]
    fn test_display_cancelled() {
        assert_eq!(format!("{}", MediaError::Cancelled), "Operation cancelled");
    }

    #[test]
    fn test_display_no_streams() {
        assert_eq!(
            format!("{}", MediaError::NoStreams),
            "No usable streams in playlist"
        );
    }

    #[test]
    fn test_display_output_exists() {
        let msg = format!(
            "{}",
            MediaError::OutputExists(PathBuf::from("/tmp/test.mkv"))
        );
        assert!(msg.contains("already exists"));
        assert!(msg.contains("/tmp/test.mkv"));
    }

    #[test]
    fn test_error_source_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let err = MediaError::Io(io_err);
        assert!(std::error::Error::source(&err).is_some());
    }

    #[test]
    fn test_error_source_none_for_others() {
        assert!(std::error::Error::source(&MediaError::Cancelled).is_none());
        assert!(std::error::Error::source(&MediaError::NoDisc).is_none());
    }
}
