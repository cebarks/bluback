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

pub type EpisodeAssignments = HashMap<String, Episode>;
