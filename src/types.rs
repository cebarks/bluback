use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Playlist {
    pub num: String,
    pub duration: String,
    pub seconds: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Episode {
    pub episode_number: u32,
    pub name: String,
    pub runtime: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TmdbShow {
    pub id: u64,
    pub name: String,
    pub first_air_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TmdbMovie {
    pub title: String,
    pub release_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LabelInfo {
    pub show: String,
    pub season: u32,
    pub disc: u32,
}

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub audio_streams: Vec<String>,
    pub sub_count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct RipProgress {
    pub frame: u64,
    pub fps: f64,
    pub total_size: u64,
    pub out_time_secs: u32,
    pub bitrate: String,
    pub speed: f64,
}

#[derive(Debug, Clone)]
pub enum PlaylistStatus {
    Pending,
    Ripping(RipProgress),
    Done(u64),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct RipJob {
    pub playlist: Playlist,
    pub episode: Option<Episode>,
    pub filename: String,
    pub status: PlaylistStatus,
}

#[derive(Debug, Clone, Default)]
pub struct MediaInfo {
    pub resolution: String,
    pub width: u32,
    pub height: u32,
    pub codec: String,
    pub hdr: String,
    pub aspect_ratio: String,
    pub framerate: String,
    pub bit_depth: String,
    pub profile: String,
    pub audio: String,
    pub channels: String,
    pub audio_lang: String,
}

impl MediaInfo {
    pub fn to_vars(&self) -> std::collections::HashMap<&str, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("resolution", self.resolution.clone());
        m.insert(
            "width",
            if self.width > 0 {
                self.width.to_string()
            } else {
                String::new()
            },
        );
        m.insert(
            "height",
            if self.height > 0 {
                self.height.to_string()
            } else {
                String::new()
            },
        );
        m.insert("codec", self.codec.clone());
        m.insert("hdr", self.hdr.clone());
        m.insert("aspect_ratio", self.aspect_ratio.clone());
        m.insert("framerate", self.framerate.clone());
        m.insert("bit_depth", self.bit_depth.clone());
        m.insert("profile", self.profile.clone());
        m.insert("audio", self.audio.clone());
        m.insert("channels", self.channels.clone());
        m.insert("audio_lang", self.audio_lang.clone());
        m
    }
}

pub type EpisodeAssignments = HashMap<String, Episode>;

pub struct TmdbLookupResult {
    pub episodes: Vec<Episode>,
    pub season: u32,
    pub show_name: String,
}

/// Result types for background operations in TUI mode
pub enum BackgroundResult {
    /// No disc detected on this device
    WaitingForDisc(String),
    /// Disc found, now scanning playlists
    DiscFound(String),
    /// Disc scan completed: (device, label, playlists)
    DiscScan(anyhow::Result<(String, String, Vec<Playlist>)>),
    /// TMDb show search completed
    ShowSearch(anyhow::Result<Vec<TmdbShow>>),
    /// TMDb movie search completed
    MovieSearch(anyhow::Result<Vec<TmdbMovie>>),
    /// TMDb season fetch completed
    SeasonFetch(anyhow::Result<Vec<Episode>>),
    /// Media info probes completed (one per selected playlist)
    MediaProbe(Vec<Option<MediaInfo>>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_info_to_vars_all_fields() {
        let info = MediaInfo {
            resolution: "1080p".into(),
            width: 1920,
            height: 1080,
            codec: "hevc".into(),
            hdr: "HDR10".into(),
            aspect_ratio: "16:9".into(),
            framerate: "23.976".into(),
            bit_depth: "10".into(),
            profile: "Main 10".into(),
            audio: "truehd".into(),
            channels: "7.1".into(),
            audio_lang: "eng".into(),
        };
        let vars = info.to_vars();
        assert_eq!(vars["resolution"], "1080p");
        assert_eq!(vars["width"], "1920");
        assert_eq!(vars["height"], "1080");
        assert_eq!(vars["codec"], "hevc");
        assert_eq!(vars["hdr"], "HDR10");
        assert_eq!(vars["aspect_ratio"], "16:9");
        assert_eq!(vars["framerate"], "23.976");
        assert_eq!(vars["bit_depth"], "10");
        assert_eq!(vars["profile"], "Main 10");
        assert_eq!(vars["audio"], "truehd");
        assert_eq!(vars["channels"], "7.1");
        assert_eq!(vars["audio_lang"], "eng");
    }

    #[test]
    fn test_media_info_default_is_empty() {
        let info = MediaInfo::default();
        let vars = info.to_vars();
        assert_eq!(vars["resolution"], "");
        assert_eq!(vars["codec"], "");
        assert_eq!(vars["hdr"], "");
    }
}
