use serde::Deserialize;
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct Playlist {
    pub num: String,
    pub duration: String,
    pub seconds: u32,
    pub video_streams: u32,
    pub audio_streams: u32,
    pub subtitle_streams: u32,
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
#[allow(dead_code)] // Public API — fields read when media module is consumed directly
pub struct AudioStream {
    pub index: usize,
    pub codec: String,
    pub channels: u16,
    pub channel_layout: String,
    pub language: Option<String>,
    pub profile: Option<String>,
}

impl AudioStream {
    pub fn is_surround(&self) -> bool {
        self.channels >= 6
    }

    pub fn display_line(&self) -> String {
        let lang = self.language.as_deref().unwrap_or("und");
        let codec_name = self.profile.as_deref().unwrap_or(&self.codec);
        format!("{} {} ({})", codec_name, self.channel_layout, lang)
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API — fields read when media module is consumed directly
pub struct VideoStream {
    pub index: usize,
    pub codec: String,
    pub resolution: String,
    pub hdr: String,
    pub framerate: String,
    pub bit_depth: String,
}

impl VideoStream {
    pub fn display_line(&self) -> String {
        let hdr_part = if self.hdr.is_empty() || self.hdr == "SDR" {
            String::new()
        } else {
            format!("  {}", self.hdr)
        };
        format!(
            "{} {}  {}fps{}",
            self.codec.to_uppercase(),
            self.resolution,
            self.framerate,
            hdr_part
        )
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API — fields read when media module is consumed directly
pub struct SubtitleStream {
    pub index: usize,
    pub codec: String,
    pub language: Option<String>,
    pub forced: bool,
}

impl SubtitleStream {
    pub fn display_line(&self) -> String {
        let lang = self.language.as_deref().unwrap_or("und");
        let forced_tag = if self.forced { " FORCED" } else { "" };
        format!("{} ({}){}", self.codec_display_name(), lang, forced_tag)
    }

    fn codec_display_name(&self) -> &str {
        match self.codec.as_str() {
            "hdmv_pgs_subtitle" => "PGS",
            "subrip" | "srt" => "SRT",
            "dvd_subtitle" => "VobSub",
            "ass" => "ASS",
            other => other,
        }
    }
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // Public API — fields read when media module is consumed directly
pub struct StreamInfo {
    pub video_streams: Vec<VideoStream>,
    pub audio_streams: Vec<AudioStream>,
    pub subtitle_streams: Vec<SubtitleStream>,
    #[deprecated(
        note = "Use subtitle_streams.len() instead — will be removed after probe/remux migration"
    )]
    pub subtitle_count: u32,
}

#[derive(Debug, Clone)]
pub struct ChapterMark {
    #[allow(dead_code)] // Used by media module's chapter injection logic
    pub index: u32,
    pub start_secs: f64,
}

/// Resolved MKV metadata tags ready to write to the output container.
#[derive(Debug, Clone, Default)]
pub struct MkvMetadata {
    pub tags: HashMap<String, String>,
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
    #[allow(dead_code)]
    // Matched in dashboard render; constructed by Task 5 (verify failure prompt)
    Verifying,
    Done(u64),
    Verified(u64, #[allow(dead_code)] crate::verify::VerifyResult),
    VerifyFailed(u64, crate::verify::VerifyResult),
    Skipped(u64),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct RipJob {
    pub playlist: Playlist,
    pub episode: Vec<Episode>,
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
    /// Overall bitrate in bits/s from ffprobe format section
    pub bitrate_bps: u64,
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

pub type EpisodeAssignments = HashMap<String, Vec<Episode>>;

pub struct TmdbLookupResult {
    pub episodes: Vec<Episode>,
    pub season: u32,
    pub show_name: String,
    pub first_air_date: Option<String>,
}

/// Result types for background operations in TUI mode
pub enum BackgroundResult {
    /// No disc detected on this device
    WaitingForDisc(String),
    /// Disc found, now scanning playlists
    DiscFound(String),
    /// Scan progress: (elapsed_secs, timeout_secs)
    ScanProgress(u64, u64),
    /// Disc scan completed: (device, label, playlists)
    DiscScan(anyhow::Result<(String, String, Vec<Playlist>)>),
    /// TMDb show search completed
    ShowSearch(anyhow::Result<Vec<TmdbShow>>),
    /// TMDb movie search completed
    MovieSearch(anyhow::Result<Vec<TmdbMovie>>),
    /// TMDb season fetch completed
    SeasonFetch(anyhow::Result<Vec<Episode>>),
    /// Single playlist probe result (for lazy probe of filtered playlists)
    #[allow(dead_code)] // Constructed by Task 12 (final wiring)
    MediaProbe(String, Box<Option<(MediaInfo, StreamInfo)>>),
    /// Bulk probe results for episode-length playlists
    #[allow(dead_code)] // Constructed by Task 12 (final wiring)
    BulkProbe(std::collections::HashMap<String, (MediaInfo, StreamInfo)>),
}

// =============================================================================
// Multi-drive types
// =============================================================================

/// Unique identifier for a drive session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// State shown in the tab bar for each session
#[derive(Debug, Clone, PartialEq)]
pub enum TabState {
    Idle,
    Scanning,
    Wizard,
    Ripping,
    Done,
    Error,
}

/// Compact summary for tab bar rendering
#[derive(Debug, Clone)]
pub struct TabSummary {
    #[allow(dead_code)] // Part of multi-drive API; used for overlap tracking
    pub session_id: SessionId,
    pub device_name: String,
    pub state: TabState,
    /// (current_job, total_jobs, overall_percent)
    pub rip_progress: Option<(usize, usize, u8)>,
    pub error: Option<String>,
}

/// Commands sent from main thread to a session thread
pub enum SessionCommand {
    /// Keyboard input routed to this session
    KeyEvent(crossterm::event::KeyEvent),
    /// Copy TMDb/season/episode context from another session
    LinkTo { context: SharedContext },
    /// Config was updated via settings panel
    ConfigChanged(Box<crate::config::Config>),
    /// Drive removed or app shutting down
    Shutdown,
}

/// Messages sent from a session thread to the main thread
pub enum SessionMessage {
    /// Full display state snapshot (on screen transitions, wizard changes)
    Snapshot(Box<RenderSnapshot>),
    /// Lightweight rip progress update (frequent during remux)
    #[allow(dead_code)] // Part of multi-drive API; not yet emitted but handled by coordinator
    Progress {
        session_id: SessionId,
        progress: RipProgress,
        job_index: usize,
    },
    /// One-shot event notification
    #[allow(dead_code)] // Part of multi-drive API; not yet emitted but handled by coordinator
    Notification(Notification),
}

/// One-shot notifications from session to main thread
#[derive(Debug, Clone)]
#[allow(dead_code)] // Part of multi-drive API; variants not yet constructed but handled by coordinator
pub enum Notification {
    /// Session's screen changed (for tab bar update)
    ScreenChanged {
        session_id: SessionId,
        tab_summary: TabSummary,
    },
    /// Episode assignments confirmed (for overlap validation)
    EpisodesAssigned {
        session_id: SessionId,
        show_name: String,
        season: u32,
        episodes: Vec<u32>,
    },
    /// Rip job completed
    RipComplete {
        session_id: SessionId,
        filename: String,
        size: u64,
    },
    /// Rip job failed
    RipFailed {
        session_id: SessionId,
        filename: String,
        error: String,
    },
    /// All rip jobs done
    AllDone { session_id: SessionId },
    /// Session crashed
    SessionCrashed {
        session_id: SessionId,
        error: String,
    },
    /// New disc detected (on Done screen)
    DiscDetected {
        session_id: SessionId,
        label: String,
    },
}

/// Context copied from one session to another for linked multi-disc workflows
#[derive(Debug, Clone)]
pub struct SharedContext {
    pub show_name: String,
    pub tmdb_show: Option<TmdbShow>,
    pub season_num: u32,
    pub next_episode: u32,
    pub movie_mode: bool,
    pub episodes: Vec<Episode>,
}

/// Events from the drive monitor thread
pub enum DriveEvent {
    /// New optical drive detected
    DriveAppeared(std::path::PathBuf),
    /// Optical drive removed
    DriveDisappeared(std::path::PathBuf),
    /// Disc inserted into a drive (device, volume_label)
    DiscInserted(std::path::PathBuf, #[allow(dead_code)] String),
    /// Disc ejected from a drive
    DiscEjected(std::path::PathBuf),
}

/// Full display-only state sent from session to main thread for rendering.
/// Only the view matching the current screen is populated.
#[derive(Debug, Clone)]
pub struct RenderSnapshot {
    pub session_id: SessionId,
    #[allow(dead_code)] // Part of multi-drive API; available for per-session device display
    pub device: std::path::PathBuf,
    pub screen: crate::tui::Screen,
    pub status_message: String,
    pub spinner_frame: usize,
    /// Available once TMDb lookup complete (for Ctrl+L link picker)
    pub linkable_context: Option<SharedContext>,
    pub scanning: Option<ScanningView>,
    pub tmdb: Option<TmdbView>,
    pub season: Option<SeasonView>,
    pub playlist_mgr: Option<PlaylistView>,
    pub confirm: Option<ConfirmView>,
    pub dashboard: Option<DashboardView>,
    pub done: Option<DoneView>,
}

#[derive(Debug, Clone)]
pub struct ScanningView {
    pub label: String,
    pub scan_log: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TmdbView {
    pub has_api_key: bool,
    pub movie_mode: bool,
    #[allow(dead_code)] // Part of multi-drive view API; populated by session snapshot
    pub search_query: String,
    pub input_buffer: String,
    pub input_focus: crate::tui::InputFocus,
    pub show_results: Vec<TmdbShow>,
    pub movie_results: Vec<TmdbMovie>,
    pub list_cursor: usize,
    #[allow(dead_code)] // Part of multi-drive view API; populated by session snapshot
    pub show_name: String,
    pub label: String,
    pub episodes_pl_count: usize,
}

#[derive(Debug, Clone)]
pub struct SeasonView {
    pub show_name: String,
    pub season_num: Option<u32>,
    pub input_buffer: String,
    pub input_focus: crate::tui::InputFocus,
    pub episodes: Vec<Episode>,
    pub list_cursor: usize,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct PlaylistView {
    pub movie_mode: bool,
    pub show_name: String,
    pub season_num: Option<u32>,
    pub playlists: Vec<Playlist>,
    pub episodes_pl: Vec<Playlist>,
    pub playlist_selected: Vec<bool>,
    pub episode_assignments: EpisodeAssignments,
    pub specials: HashSet<String>,
    pub show_filtered: bool,
    pub list_cursor: usize,
    pub input_focus: crate::tui::InputFocus,
    pub input_buffer: String,
    pub chapter_counts: HashMap<String, usize>,
    #[allow(dead_code)] // Part of multi-drive view API; available for episode name display
    pub episodes: Vec<Episode>,
    pub label: String,
    pub filenames: HashMap<String, String>,
    pub stream_infos: HashMap<String, StreamInfo>,
    pub track_selections: HashMap<String, Vec<usize>>,
    pub expanded_playlist: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ConfirmView {
    pub filenames: Vec<String>,
    pub playlists: Vec<Playlist>,
    #[allow(dead_code)] // Part of multi-drive view API; available for episode detail display
    pub episode_assignments: EpisodeAssignments,
    #[allow(dead_code)] // Part of multi-drive view API; available for scroll position
    pub list_cursor: usize,
    pub movie_mode: bool,
    pub label: String,
    pub output_dir: String,
    pub dry_run: bool,
    pub media_infos: HashMap<String, MediaInfo>,
}

#[derive(Debug, Clone)]
pub struct DashboardView {
    pub jobs: Vec<RipJob>,
    pub current_rip: usize,
    pub confirm_abort: bool,
    pub confirm_rescan: bool,
    pub label: String,
    pub verify_failed_idx: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DoneView {
    pub jobs: Vec<RipJob>,
    #[allow(dead_code)] // Part of multi-drive view API; available for done screen header
    pub label: String,
    pub disc_detected_label: Option<String>,
    #[allow(dead_code)] // Part of multi-drive view API; controls post-rip eject behavior
    pub eject: bool,
    pub status_message: String,
    pub filenames: Vec<String>,
}

pub enum Overlay {
    Settings(SettingsState),
}

pub struct SettingsState {
    pub cursor: usize,
    pub items: Vec<SettingItem>,
    pub editing: Option<usize>,
    pub input_buffer: String,
    pub cursor_pos: usize,
    pub dirty: bool,
    pub save_message: Option<String>,
    pub save_message_at: Option<std::time::Instant>,
    pub confirm_close: Option<bool>,
    pub scroll_offset: usize,
    pub standalone: bool,
    /// Env var overrides detected at open: (env_var_name, key, value)
    pub env_overrides: Vec<(String, String, String)>,
}

pub enum SettingItem {
    Toggle {
        label: String,
        key: String,
        value: bool,
    },
    Choice {
        label: String,
        key: String,
        options: Vec<String>,
        selected: usize,
        /// For "Custom..." option: stores the user-entered value
        custom_value: Option<String>,
    },
    Text {
        label: String,
        key: String,
        value: String,
    },
    Number {
        label: String,
        key: String,
        value: u32,
    },
    Separator {
        label: Option<String>,
    },
    Action {
        label: String,
    },
}

impl SettingsState {
    #[allow(dead_code)] // Used extensively in tests
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self::from_config_with_drives(config, &[])
    }

    pub fn from_config_with_drives(
        config: &crate::config::Config,
        detected_drives: &[String],
    ) -> Self {
        use crate::config::*;

        // Build device options: auto-detect, detected drives, Custom...
        let mut device_options = vec![DEFAULT_DEVICE.to_string()];
        for drive in detected_drives {
            let s = drive.to_string();
            if s != DEFAULT_DEVICE && !device_options.contains(&s) {
                device_options.push(s);
            }
        }
        device_options.push("Custom...".to_string());

        let configured_device = config
            .device
            .clone()
            .unwrap_or_else(|| DEFAULT_DEVICE.into());
        let (device_selected, device_custom) = if configured_device == DEFAULT_DEVICE {
            (0, None)
        } else if let Some(pos) = device_options.iter().position(|o| o == &configured_device) {
            (pos, None)
        } else {
            // Custom value not in detected drives
            (device_options.len() - 1, Some(configured_device))
        };

        let items = vec![
            SettingItem::Separator {
                label: Some("General".into()),
            },
            SettingItem::Text {
                label: "Output Directory".into(),
                key: "output_dir".into(),
                value: config
                    .output_dir
                    .clone()
                    .unwrap_or_else(|| DEFAULT_OUTPUT_DIR.into()),
            },
            SettingItem::Choice {
                label: "Device".into(),
                key: "device".into(),
                options: device_options,
                selected: device_selected,
                custom_value: device_custom,
            },
            SettingItem::Toggle {
                label: "Auto-Eject After Rip".into(),
                key: "eject".into(),
                value: config.eject.unwrap_or(false),
            },
            SettingItem::Toggle {
                label: "Max Read Speed".into(),
                key: "max_speed".into(),
                value: config.max_speed.unwrap_or(true),
            },
            SettingItem::Number {
                label: "Min Duration (secs)".into(),
                key: "min_duration".into(),
                value: config.min_duration.unwrap_or(DEFAULT_MIN_DURATION),
            },
            SettingItem::Toggle {
                label: "Verbose libbluray".into(),
                key: "verbose_libbluray".into(),
                value: config.verbose_libbluray.unwrap_or(false),
            },
            SettingItem::Toggle {
                label: "Overwrite Existing Files".into(),
                key: "overwrite".into(),
                value: config.overwrite.unwrap_or(false),
            },
            SettingItem::Toggle {
                label: "Verify Rips".into(),
                key: "verify".into(),
                value: config.verify.unwrap_or(false),
            },
            SettingItem::Choice {
                label: "Verify Level".into(),
                key: "verify_level".into(),
                options: vec!["quick".into(), "full".into()],
                selected: match config.verify_level.as_deref() {
                    Some("full") => 1,
                    _ => 0,
                },
                custom_value: None,
            },
            SettingItem::Choice {
                key: "aacs_backend".into(),
                label: "AACS Backend".into(),
                options: vec!["auto".into(), "libaacs".into(), "libmmbd".into()],
                selected: match config.aacs_backend.as_deref() {
                    Some("libaacs") => 1,
                    Some("libmmbd") => 2,
                    _ => 0,
                },
                custom_value: None,
            },
            SettingItem::Number {
                label: "Index Reserve Space (KB)".into(),
                key: "reserve_index_space".into(),
                value: config
                    .reserve_index_space
                    .unwrap_or(DEFAULT_RESERVE_INDEX_SPACE),
            },
            SettingItem::Separator {
                label: Some("Naming".into()),
            },
            SettingItem::Choice {
                label: "Preset".into(),
                key: "preset".into(),
                options: vec![
                    "(none)".into(),
                    "default".into(),
                    "plex".into(),
                    "jellyfin".into(),
                ],
                selected: match config.preset.as_deref() {
                    Some("default") => 1,
                    Some("plex") => 2,
                    Some("jellyfin") => 3,
                    _ => 0,
                },
                custom_value: None,
            },
            SettingItem::Text {
                label: "TV Format".into(),
                key: "tv_format".into(),
                value: config
                    .tv_format
                    .clone()
                    .unwrap_or_else(|| DEFAULT_TV_FORMAT.into()),
            },
            SettingItem::Text {
                label: "Movie Format".into(),
                key: "movie_format".into(),
                value: config
                    .movie_format
                    .clone()
                    .unwrap_or_else(|| DEFAULT_MOVIE_FORMAT.into()),
            },
            SettingItem::Text {
                label: "Special Format".into(),
                key: "special_format".into(),
                value: config
                    .special_format
                    .clone()
                    .unwrap_or_else(|| DEFAULT_SPECIAL_FORMAT.into()),
            },
            SettingItem::Toggle {
                label: "Show Filtered".into(),
                key: "show_filtered".into(),
                value: config.show_filtered.unwrap_or(false),
            },
            SettingItem::Separator {
                label: Some("Logging".into()),
            },
            SettingItem::Toggle {
                label: "Log to File".into(),
                key: "log_file".into(),
                value: config.log_file.unwrap_or(true),
            },
            SettingItem::Choice {
                label: "Stderr Log Level".into(),
                key: "log_level".into(),
                options: vec![
                    "error".into(),
                    "warn".into(),
                    "info".into(),
                    "debug".into(),
                    "trace".into(),
                ],
                selected: match config.log_level.as_deref() {
                    Some("error") => 0,
                    Some("info") => 2,
                    Some("debug") => 3,
                    Some("trace") => 4,
                    _ => 1, // warn is default
                },
                custom_value: None,
            },
            SettingItem::Separator {
                label: Some("TMDb".into()),
            },
            SettingItem::Text {
                label: "API Key".into(),
                key: "tmdb_api_key".into(),
                value: config.tmdb_api_key.clone().unwrap_or_default(),
            },
            SettingItem::Separator {
                label: Some("Metadata".into()),
            },
            SettingItem::Toggle {
                label: "Embed Metadata Tags".into(),
                key: "metadata.enabled".into(),
                value: config.metadata_enabled(),
            },
            SettingItem::Separator {
                label: Some("Streams".into()),
            },
            SettingItem::Text {
                label: "Audio Languages".into(),
                key: "audio_languages".into(),
                value: config
                    .streams
                    .as_ref()
                    .and_then(|s| s.audio_languages.as_ref())
                    .map(|v| v.join(","))
                    .unwrap_or_default(),
            },
            SettingItem::Text {
                label: "Subtitle Languages".into(),
                key: "subtitle_languages".into(),
                value: config
                    .streams
                    .as_ref()
                    .and_then(|s| s.subtitle_languages.as_ref())
                    .map(|v| v.join(","))
                    .unwrap_or_default(),
            },
            SettingItem::Toggle {
                label: "Prefer Surround".into(),
                key: "prefer_surround".into(),
                value: config
                    .streams
                    .as_ref()
                    .and_then(|s| s.prefer_surround)
                    .unwrap_or(false),
            },
            SettingItem::Separator {
                label: Some("Hooks".into()),
            },
            SettingItem::Text {
                label: "Post-Rip Command".into(),
                key: "post_rip.command".into(),
                value: config
                    .post_rip
                    .as_ref()
                    .and_then(|h| h.command.clone())
                    .unwrap_or_default(),
            },
            SettingItem::Toggle {
                label: "  Run on Failure".into(),
                key: "post_rip.on_failure".into(),
                value: config
                    .post_rip
                    .as_ref()
                    .map(|h| h.on_failure())
                    .unwrap_or(false),
            },
            SettingItem::Toggle {
                label: "  Blocking".into(),
                key: "post_rip.blocking".into(),
                value: config
                    .post_rip
                    .as_ref()
                    .map(|h| h.blocking())
                    .unwrap_or(true),
            },
            SettingItem::Toggle {
                label: "  Log Output".into(),
                key: "post_rip.log_output".into(),
                value: config
                    .post_rip
                    .as_ref()
                    .map(|h| h.log_output())
                    .unwrap_or(true),
            },
            SettingItem::Text {
                label: "Post-Session Command".into(),
                key: "post_session.command".into(),
                value: config
                    .post_session
                    .as_ref()
                    .and_then(|h| h.command.clone())
                    .unwrap_or_default(),
            },
            SettingItem::Toggle {
                label: "  Run on Failure".into(),
                key: "post_session.on_failure".into(),
                value: config
                    .post_session
                    .as_ref()
                    .map(|h| h.on_failure())
                    .unwrap_or(false),
            },
            SettingItem::Toggle {
                label: "  Blocking".into(),
                key: "post_session.blocking".into(),
                value: config
                    .post_session
                    .as_ref()
                    .map(|h| h.blocking())
                    .unwrap_or(true),
            },
            SettingItem::Toggle {
                label: "  Log Output".into(),
                key: "post_session.log_output".into(),
                value: config
                    .post_session
                    .as_ref()
                    .map(|h| h.log_output())
                    .unwrap_or(true),
            },
            SettingItem::Separator { label: None },
            SettingItem::Action {
                label: "Save to Config (Ctrl+S)".into(),
            },
        ];

        let cursor = items
            .iter()
            .position(|i| !matches!(i, SettingItem::Separator { .. }))
            .unwrap_or(0);

        SettingsState {
            cursor,
            items,
            editing: None,
            input_buffer: String::new(),
            cursor_pos: 0,
            dirty: false,
            save_message: None,
            save_message_at: None,
            confirm_close: None,
            scroll_offset: 0,
            standalone: false,
            env_overrides: Vec::new(),
        }
    }

    /// Check for BLUBACK_* env vars and apply their values to settings items.
    /// Returns the list of overrides that were applied.
    pub fn apply_env_overrides(&mut self) {
        const ENV_MAPPINGS: &[(&str, &str)] = &[
            ("BLUBACK_OUTPUT_DIR", "output_dir"),
            ("BLUBACK_DEVICE", "device"),
            ("BLUBACK_EJECT", "eject"),
            ("BLUBACK_MAX_SPEED", "max_speed"),
            ("BLUBACK_MIN_DURATION", "min_duration"),
            ("BLUBACK_PRESET", "preset"),
            ("BLUBACK_TV_FORMAT", "tv_format"),
            ("BLUBACK_MOVIE_FORMAT", "movie_format"),
            ("BLUBACK_SPECIAL_FORMAT", "special_format"),
            ("BLUBACK_SHOW_FILTERED", "show_filtered"),
            ("BLUBACK_VERBOSE_LIBBLURAY", "verbose_libbluray"),
            ("BLUBACK_RESERVE_INDEX_SPACE", "reserve_index_space"),
            ("BLUBACK_OVERWRITE", "overwrite"),
            ("BLUBACK_VERIFY", "verify"),
            ("BLUBACK_VERIFY_LEVEL", "verify_level"),
            ("BLUBACK_AACS_BACKEND", "aacs_backend"),
            ("BLUBACK_METADATA", "metadata.enabled"),
            ("BLUBACK_AUDIO_LANGUAGES", "audio_languages"),
            ("BLUBACK_SUBTITLE_LANGUAGES", "subtitle_languages"),
            ("BLUBACK_PREFER_SURROUND", "prefer_surround"),
            ("TMDB_API_KEY", "tmdb_api_key"),
        ];

        let mut overrides = Vec::new();

        for &(env_var, config_key) in ENV_MAPPINGS {
            if let Ok(val) = std::env::var(env_var) {
                if val.is_empty() {
                    continue;
                }
                let applied = self.apply_env_value(config_key, &val);
                if applied {
                    overrides.push((env_var.to_string(), config_key.to_string(), val));
                }
            }
        }

        if !overrides.is_empty() {
            self.dirty = true;
            let names: Vec<&str> = overrides.iter().map(|(env, _, _)| env.as_str()).collect();
            self.save_message = Some(format!("Imported from env: {}", names.join(", ")));
            // Don't set save_message_at — message persists until next user input
            // (the 2-second auto-clear only triggers when save_message_at is Some)
        }
        self.env_overrides = overrides;
    }

    fn apply_env_value(&mut self, key: &str, val: &str) -> bool {
        for item in &mut self.items {
            match item {
                SettingItem::Text { key: k, value, .. } if k == key => {
                    *value = val.to_string();
                    return true;
                }
                SettingItem::Toggle { key: k, value, .. } if k == key => {
                    match val.to_lowercase().as_str() {
                        "true" | "1" | "yes" => {
                            *value = true;
                            return true;
                        }
                        "false" | "0" | "no" => {
                            *value = false;
                            return true;
                        }
                        _ => return false,
                    }
                }
                SettingItem::Number { key: k, value, .. } if k == key => {
                    if let Ok(n) = val.parse::<u32>() {
                        if n > 0 {
                            *value = n;
                            return true;
                        }
                    }
                    return false;
                }
                SettingItem::Choice {
                    key: k,
                    options,
                    selected,
                    custom_value,
                    ..
                } if k == key => {
                    if key == "device" {
                        // Check if the value matches a known option
                        if let Some(pos) = options.iter().position(|o| o == val) {
                            *selected = pos;
                        } else {
                            // Set as custom
                            *selected = options.len() - 1; // "Custom..."
                            *custom_value = Some(val.to_string());
                        }
                        return true;
                    } else if key == "preset" || key == "aacs_backend" {
                        if let Some(pos) = options.iter().position(|o| o == val) {
                            *selected = pos;
                            return true;
                        }
                    }
                    return false;
                }
                _ => {}
            }
        }
        false
    }

    /// Check which env vars are currently set that would override saved config.
    pub fn active_env_var_warnings(&self) -> Vec<String> {
        const ENV_MAPPINGS: &[(&str, &str)] = &[
            ("BLUBACK_OUTPUT_DIR", "output_dir"),
            ("BLUBACK_DEVICE", "device"),
            ("BLUBACK_EJECT", "eject"),
            ("BLUBACK_MAX_SPEED", "max_speed"),
            ("BLUBACK_MIN_DURATION", "min_duration"),
            ("BLUBACK_PRESET", "preset"),
            ("BLUBACK_TV_FORMAT", "tv_format"),
            ("BLUBACK_MOVIE_FORMAT", "movie_format"),
            ("BLUBACK_SPECIAL_FORMAT", "special_format"),
            ("BLUBACK_SHOW_FILTERED", "show_filtered"),
            ("BLUBACK_VERBOSE_LIBBLURAY", "verbose_libbluray"),
            ("BLUBACK_RESERVE_INDEX_SPACE", "reserve_index_space"),
            ("BLUBACK_OVERWRITE", "overwrite"),
            ("BLUBACK_VERIFY", "verify"),
            ("BLUBACK_VERIFY_LEVEL", "verify_level"),
            ("BLUBACK_AACS_BACKEND", "aacs_backend"),
            ("BLUBACK_METADATA", "metadata.enabled"),
            ("BLUBACK_AUDIO_LANGUAGES", "audio_languages"),
            ("BLUBACK_SUBTITLE_LANGUAGES", "subtitle_languages"),
            ("BLUBACK_PREFER_SURROUND", "prefer_surround"),
            ("TMDB_API_KEY", "tmdb_api_key"),
        ];

        let mut warnings = Vec::new();
        for &(env_var, _) in ENV_MAPPINGS {
            if let Ok(val) = std::env::var(env_var) {
                if !val.is_empty() {
                    warnings.push(env_var.to_string());
                }
            }
        }
        warnings
    }

    pub fn to_config(&self) -> crate::config::Config {
        use crate::config::*;
        let mut config = crate::config::Config::default();

        for item in &self.items {
            match item {
                SettingItem::Text { key, value, .. } => match key.as_str() {
                    "output_dir" if value != DEFAULT_OUTPUT_DIR => {
                        config.output_dir = Some(value.clone())
                    }
                    "tv_format" if value != DEFAULT_TV_FORMAT => {
                        config.tv_format = Some(value.clone())
                    }
                    "movie_format" if value != DEFAULT_MOVIE_FORMAT => {
                        config.movie_format = Some(value.clone())
                    }
                    "special_format" if value != DEFAULT_SPECIAL_FORMAT => {
                        config.special_format = Some(value.clone())
                    }
                    "tmdb_api_key" if !value.is_empty() => {
                        config.tmdb_api_key = Some(value.clone())
                    }
                    "post_rip.command" if !value.is_empty() => {
                        let hook = config.post_rip.get_or_insert_with(Default::default);
                        hook.command = Some(value.clone());
                    }
                    "post_session.command" if !value.is_empty() => {
                        let hook = config.post_session.get_or_insert_with(Default::default);
                        hook.command = Some(value.clone());
                    }
                    "audio_languages" if !value.is_empty() => {
                        let langs: Vec<String> = value
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if !langs.is_empty() {
                            let streams = config.streams.get_or_insert_with(Default::default);
                            streams.audio_languages = Some(langs);
                        }
                    }
                    "subtitle_languages" if !value.is_empty() => {
                        let langs: Vec<String> = value
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if !langs.is_empty() {
                            let streams = config.streams.get_or_insert_with(Default::default);
                            streams.subtitle_languages = Some(langs);
                        }
                    }
                    _ => {}
                },
                SettingItem::Toggle { key, value, .. } => match key.as_str() {
                    "eject" if *value => config.eject = Some(true),
                    "max_speed" if !*value => config.max_speed = Some(false),
                    "show_filtered" if *value => config.show_filtered = Some(true),
                    "verbose_libbluray" if *value => config.verbose_libbluray = Some(true),
                    "overwrite" if *value => config.overwrite = Some(true),
                    "verify" if *value => config.verify = Some(true),
                    "log_file" if !*value => config.log_file = Some(false),
                    "metadata.enabled" if !*value => {
                        let meta = config.metadata.get_or_insert_with(Default::default);
                        meta.enabled = Some(false);
                    }
                    "post_rip.on_failure" if *value => {
                        let hook = config.post_rip.get_or_insert_with(Default::default);
                        hook.on_failure = Some(true);
                    }
                    "post_rip.blocking" if !*value => {
                        let hook = config.post_rip.get_or_insert_with(Default::default);
                        hook.blocking = Some(false);
                    }
                    "post_rip.log_output" if !*value => {
                        let hook = config.post_rip.get_or_insert_with(Default::default);
                        hook.log_output = Some(false);
                    }
                    "post_session.on_failure" if *value => {
                        let hook = config.post_session.get_or_insert_with(Default::default);
                        hook.on_failure = Some(true);
                    }
                    "post_session.blocking" if !*value => {
                        let hook = config.post_session.get_or_insert_with(Default::default);
                        hook.blocking = Some(false);
                    }
                    "post_session.log_output" if !*value => {
                        let hook = config.post_session.get_or_insert_with(Default::default);
                        hook.log_output = Some(false);
                    }
                    "prefer_surround" if *value => {
                        let streams = config.streams.get_or_insert_with(Default::default);
                        streams.prefer_surround = Some(true);
                    }
                    _ => {}
                },
                SettingItem::Number { key, value, .. } => match key.as_str() {
                    "min_duration" if *value != DEFAULT_MIN_DURATION => {
                        config.min_duration = Some(*value)
                    }
                    "reserve_index_space" if *value != DEFAULT_RESERVE_INDEX_SPACE => {
                        config.reserve_index_space = Some(*value)
                    }
                    _ => {}
                },
                SettingItem::Choice {
                    key,
                    options,
                    selected,
                    custom_value,
                    ..
                } => match key.as_str() {
                    "preset" => {
                        let val = &options[*selected];
                        if val != "(none)" {
                            config.preset = Some(val.clone());
                        }
                    }
                    "device" => {
                        let val = &options[*selected];
                        if val == "Custom..." {
                            if let Some(ref cv) = custom_value {
                                if !cv.is_empty() && cv != DEFAULT_DEVICE {
                                    config.device = Some(cv.clone());
                                }
                            }
                        } else if val != DEFAULT_DEVICE {
                            config.device = Some(val.clone());
                        }
                    }
                    "aacs_backend" => {
                        let val = &options[*selected];
                        if val != "auto" {
                            config.aacs_backend = Some(val.clone());
                        }
                    }
                    "verify_level" => {
                        let val = &options[*selected];
                        if val != "quick" {
                            config.verify_level = Some(val.clone());
                        }
                    }
                    "log_level" => {
                        let val = &options[*selected];
                        if val != "warn" {
                            config.log_level = Some(val.clone());
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        config
    }

    pub fn is_separator(&self, idx: usize) -> bool {
        matches!(self.items.get(idx), Some(SettingItem::Separator { .. }))
    }

    pub fn move_cursor_down(&mut self) {
        let mut next = self.cursor + 1;
        while next < self.items.len() && self.is_separator(next) {
            next += 1;
        }
        if next < self.items.len() {
            self.cursor = next;
        }
    }

    pub fn move_cursor_up(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut prev = self.cursor - 1;
        while prev > 0 && self.is_separator(prev) {
            prev -= 1;
        }
        if !self.is_separator(prev) {
            self.cursor = prev;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_stream_is_surround() {
        let stream = AudioStream {
            index: 0,
            codec: "truehd".into(),
            channels: 8,
            channel_layout: "7.1".into(),
            language: Some("eng".into()),
            profile: None,
        };
        assert!(stream.is_surround());

        let stereo = AudioStream {
            index: 1,
            codec: "aac".into(),
            channels: 2,
            channel_layout: "stereo".into(),
            language: Some("eng".into()),
            profile: None,
        };
        assert!(!stereo.is_surround());
    }

    #[test]
    fn test_video_stream_display() {
        let v = VideoStream {
            index: 0,
            codec: "hevc".into(),
            resolution: "1920x1080".into(),
            hdr: "HDR10".into(),
            framerate: "23.976".into(),
            bit_depth: "10".into(),
        };
        assert_eq!(v.display_line(), "HEVC 1920x1080  23.976fps  HDR10");

        let sdr = VideoStream {
            index: 0,
            codec: "h264".into(),
            resolution: "1920x1080".into(),
            hdr: "SDR".into(),
            framerate: "24".into(),
            bit_depth: "8".into(),
        };
        assert_eq!(sdr.display_line(), "H264 1920x1080  24fps");
    }

    #[test]
    fn test_subtitle_stream_display() {
        let s = SubtitleStream {
            index: 3,
            codec: "hdmv_pgs_subtitle".into(),
            language: Some("eng".into()),
            forced: false,
        };
        assert_eq!(s.display_line(), "PGS (eng)");

        let forced = SubtitleStream {
            index: 4,
            codec: "hdmv_pgs_subtitle".into(),
            language: Some("eng".into()),
            forced: true,
        };
        assert_eq!(forced.display_line(), "PGS (eng) FORCED");

        let unknown_codec = SubtitleStream {
            index: 5,
            codec: "dvb_teletext".into(),
            language: None,
            forced: false,
        };
        assert_eq!(unknown_codec.display_line(), "dvb_teletext (und)");
    }

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
            bitrate_bps: 22587000,
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

    #[test]
    fn test_settings_state_from_config_item_count() {
        let config = crate::config::Config::default();
        let state = SettingsState::from_config(&config);
        // 7 separators + 31 settings + 1 action = 39 items
        // (28 base + 1 verify + 2 verify settings from upstream + 3 streams from this branch)
        let non_separator_count = state
            .items
            .iter()
            .filter(|i| !matches!(i, SettingItem::Separator { .. }))
            .count();
        assert_eq!(non_separator_count, 32); // 31 settings + 1 action
    }

    #[test]
    fn test_settings_state_from_config_values() {
        let config = crate::config::Config {
            eject: Some(true),
            min_duration: Some(600),
            ..Default::default()
        };
        let state = SettingsState::from_config(&config);
        let eject = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Toggle { key, .. } if key == "eject"));
        assert!(matches!(
            eject,
            Some(SettingItem::Toggle { value: true, .. })
        ));
        let min_dur = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Number { key, .. } if key == "min_duration"));
        assert!(matches!(
            min_dur,
            Some(SettingItem::Number { value: 600, .. })
        ));
    }

    #[test]
    fn test_settings_device_with_detected_drives() {
        let config = crate::config::Config::default();
        let drives = vec!["/dev/sr0".to_string(), "/dev/sr1".to_string()];
        let state = SettingsState::from_config_with_drives(&config, &drives);
        let device = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Choice { key, .. } if key == "device"))
            .unwrap();
        if let SettingItem::Choice {
            options, selected, ..
        } = device
        {
            assert_eq!(options[0], "auto-detect");
            assert!(options.contains(&"/dev/sr0".to_string()));
            assert!(options.contains(&"/dev/sr1".to_string()));
            assert_eq!(options.last().unwrap(), "Custom...");
            assert_eq!(*selected, 0); // default is auto-detect
        }
    }

    #[test]
    fn test_settings_device_known_drive_selected() {
        let config = crate::config::Config {
            device: Some("/dev/sr1".into()),
            ..Default::default()
        };
        let drives = vec!["/dev/sr0".to_string(), "/dev/sr1".to_string()];
        let state = SettingsState::from_config_with_drives(&config, &drives);
        let device = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Choice { key, .. } if key == "device"))
            .unwrap();
        if let SettingItem::Choice {
            options, selected, ..
        } = device
        {
            assert_eq!(options[*selected], "/dev/sr1");
        }
    }

    #[test]
    fn test_settings_device_custom_value() {
        let config = crate::config::Config {
            device: Some("/dev/custom0".into()),
            ..Default::default()
        };
        let drives = vec!["/dev/sr0".to_string()];
        let state = SettingsState::from_config_with_drives(&config, &drives);
        let device = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Choice { key, .. } if key == "device"))
            .unwrap();
        if let SettingItem::Choice {
            options,
            selected,
            custom_value,
            ..
        } = device
        {
            assert_eq!(options[*selected], "Custom...");
            assert_eq!(custom_value.as_deref(), Some("/dev/custom0"));
        }
    }

    #[test]
    fn test_settings_device_to_config_detected() {
        let config = crate::config::Config::default();
        let drives = vec!["/dev/sr0".to_string()];
        let mut state = SettingsState::from_config_with_drives(&config, &drives);
        // Select /dev/sr0
        let device_idx = state
            .items
            .iter()
            .position(|i| matches!(i, SettingItem::Choice { key, .. } if key == "device"))
            .unwrap();
        if let SettingItem::Choice { selected, .. } = &mut state.items[device_idx] {
            *selected = 1; // /dev/sr0
        }
        let restored = state.to_config();
        assert_eq!(restored.device.as_deref(), Some("/dev/sr0"));
    }

    #[test]
    fn test_settings_device_to_config_custom() {
        let config = crate::config::Config {
            device: Some("/dev/custom0".into()),
            ..Default::default()
        };
        let drives = vec!["/dev/sr0".to_string()];
        let state = SettingsState::from_config_with_drives(&config, &drives);
        let restored = state.to_config();
        assert_eq!(restored.device.as_deref(), Some("/dev/custom0"));
    }

    #[test]
    fn test_env_override_toggle() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        // Simulate BLUBACK_EJECT=true
        assert!(state.apply_env_value("eject", "true"));
        let eject = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Toggle { key, .. } if key == "eject"))
            .unwrap();
        assert!(matches!(eject, SettingItem::Toggle { value: true, .. }));
    }

    #[test]
    fn test_env_override_toggle_false() {
        let config = crate::config::Config {
            max_speed: Some(true),
            ..Default::default()
        };
        let mut state = SettingsState::from_config(&config);
        assert!(state.apply_env_value("max_speed", "false"));
        let ms = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Toggle { key, .. } if key == "max_speed"))
            .unwrap();
        assert!(matches!(ms, SettingItem::Toggle { value: false, .. }));
    }

    #[test]
    fn test_env_override_number() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        assert!(state.apply_env_value("min_duration", "600"));
        let md = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Number { key, .. } if key == "min_duration"))
            .unwrap();
        assert!(matches!(md, SettingItem::Number { value: 600, .. }));
    }

    #[test]
    fn test_env_override_number_invalid() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        assert!(!state.apply_env_value("min_duration", "abc"));
        assert!(!state.apply_env_value("min_duration", "0"));
    }

    #[test]
    fn test_env_override_text() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        assert!(state.apply_env_value("output_dir", "/tmp/rips"));
        let od = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Text { key, .. } if key == "output_dir"))
            .unwrap();
        assert!(matches!(od, SettingItem::Text { value, .. } if value == "/tmp/rips"));
    }

    #[test]
    fn test_env_override_preset() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        assert!(state.apply_env_value("preset", "plex"));
        let preset = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Choice { key, .. } if key == "preset"))
            .unwrap();
        assert!(matches!(preset, SettingItem::Choice { selected: 2, .. })); // plex is index 2
    }

    #[test]
    fn test_env_override_preset_invalid() {
        let config = crate::config::Config::default();
        let mut state = SettingsState::from_config(&config);
        assert!(!state.apply_env_value("preset", "nonexistent"));
    }

    #[test]
    fn test_settings_cursor_skips_separators_down() {
        let state = SettingsState::from_config(&crate::config::Config::default());
        // First item is a separator (General), cursor should start at first non-separator
        assert!(!state.is_separator(state.cursor));
    }

    #[test]
    fn test_settings_cursor_move_down_skips_separator() {
        let mut state = SettingsState::from_config(&crate::config::Config::default());
        // Move to the last item before a separator, then down should skip it
        // Find "Index Reserve Space" (last in General group), next is Separator(Naming)
        let reserve_idx = state
            .items
            .iter()
            .position(
                |i| matches!(i, SettingItem::Number { key, .. } if key == "reserve_index_space"),
            )
            .unwrap();
        state.cursor = reserve_idx;
        state.move_cursor_down();
        // Should have skipped the Naming separator
        assert!(!state.is_separator(state.cursor));
        assert!(state.cursor > reserve_idx + 1);
    }

    #[test]
    fn test_settings_cursor_move_up_skips_separator() {
        let mut state = SettingsState::from_config(&crate::config::Config::default());
        // Find "Preset" (first in Naming group), going up should skip the Separator
        let preset_idx = state
            .items
            .iter()
            .position(|i| matches!(i, SettingItem::Choice { key, .. } if key == "preset"))
            .unwrap();
        state.cursor = preset_idx;
        state.move_cursor_up();
        assert!(!state.is_separator(state.cursor));
        assert!(state.cursor < preset_idx - 1);
    }

    #[test]
    fn test_settings_cursor_stays_at_bounds() {
        let mut state = SettingsState::from_config(&crate::config::Config::default());
        // Move to first non-separator
        let first = state
            .items
            .iter()
            .position(|i| !matches!(i, SettingItem::Separator { .. }))
            .unwrap();
        state.cursor = first;
        state.move_cursor_up();
        // Should not go past the first non-separator
        assert_eq!(state.cursor, first);

        // Move to last item
        let last = state.items.len() - 1;
        state.cursor = last;
        state.move_cursor_down();
        assert_eq!(state.cursor, last);
    }

    #[test]
    fn test_session_id_equality() {
        assert_eq!(SessionId(1), SessionId(1));
        assert_ne!(SessionId(1), SessionId(2));
    }

    #[test]
    fn test_tab_summary_from_screen() {
        let summary = TabSummary {
            session_id: SessionId(1),
            device_name: "sr0".into(),
            state: TabState::Ripping,
            rip_progress: Some((2, 5, 40)),
            error: None,
        };
        assert_eq!(summary.state, TabState::Ripping);
        assert_eq!(summary.rip_progress, Some((2, 5, 40)));
    }

    #[test]
    fn test_shared_context_clone() {
        let ctx = SharedContext {
            show_name: "Test Show".into(),
            tmdb_show: None,
            season_num: 1,
            next_episode: 5,
            movie_mode: false,
            episodes: vec![],
        };
        let cloned = ctx.clone();
        assert_eq!(cloned.show_name, "Test Show");
        assert_eq!(cloned.next_episode, 5);
    }

    #[test]
    fn test_settings_state_to_config_roundtrip() {
        let config = crate::config::Config {
            eject: Some(true),
            preset: Some("plex".into()),
            min_duration: Some(600),
            output_dir: Some("/tmp/rips".into()),
            ..Default::default()
        };
        let state = SettingsState::from_config(&config);
        let restored = state.to_config();
        assert_eq!(restored.eject, Some(true));
        assert_eq!(restored.preset.as_deref(), Some("plex"));
        assert_eq!(restored.min_duration, Some(600));
        assert_eq!(restored.output_dir.as_deref(), Some("/tmp/rips"));
    }

    #[test]
    fn test_settings_has_hooks_section() {
        let config = crate::config::Config::default();
        let state = SettingsState::from_config(&config);
        let has_hooks_separator = state
            .items
            .iter()
            .any(|i| matches!(i, SettingItem::Separator { label: Some(l) } if l == "Hooks"));
        assert!(has_hooks_separator);
    }

    #[test]
    fn test_settings_hook_items_from_config() {
        let config = crate::config::Config {
            post_rip: Some(crate::config::HookConfig {
                command: Some("echo test".into()),
                on_failure: Some(true),
                blocking: Some(false),
                log_output: Some(false),
            }),
            ..Default::default()
        };
        let state = SettingsState::from_config(&config);
        let cmd = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Text { key, .. } if key == "post_rip.command"));
        assert!(matches!(cmd, Some(SettingItem::Text { value, .. }) if value == "echo test"));
        let on_fail = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Toggle { key, .. } if key == "post_rip.on_failure"));
        assert!(matches!(
            on_fail,
            Some(SettingItem::Toggle { value: true, .. })
        ));
    }

    #[test]
    fn test_settings_hook_to_config_roundtrip() {
        let config = crate::config::Config {
            post_rip: Some(crate::config::HookConfig {
                command: Some("echo test".into()),
                on_failure: Some(true),
                blocking: Some(false),
                log_output: None,
            }),
            ..Default::default()
        };
        let state = SettingsState::from_config(&config);
        let restored = state.to_config();
        let hook = restored.post_rip.unwrap();
        assert_eq!(hook.command.as_deref(), Some("echo test"));
        assert_eq!(hook.on_failure, Some(true));
        assert_eq!(hook.blocking, Some(false));
    }

    #[test]
    fn test_settings_verify_toggle_roundtrip() {
        let config = crate::config::Config {
            verify: Some(true),
            ..Default::default()
        };
        let state = SettingsState::from_config(&config);
        let verify = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Toggle { key, .. } if key == "verify"));
        assert!(matches!(
            verify,
            Some(SettingItem::Toggle { value: true, .. })
        ));
        let restored = state.to_config();
        assert_eq!(restored.verify, Some(true));
    }

    #[test]
    fn test_settings_verify_toggle_default_false() {
        let config = crate::config::Config::default();
        let state = SettingsState::from_config(&config);
        let verify = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Toggle { key, .. } if key == "verify"));
        assert!(matches!(
            verify,
            Some(SettingItem::Toggle { value: false, .. })
        ));
        let restored = state.to_config();
        assert!(restored.verify.is_none()); // false is default, so not serialized
    }

    #[test]
    fn test_settings_verify_level_roundtrip_full() {
        let config = crate::config::Config {
            verify_level: Some("full".into()),
            ..Default::default()
        };
        let state = SettingsState::from_config(&config);
        let level = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Choice { key, .. } if key == "verify_level"));
        assert!(matches!(
            level,
            Some(SettingItem::Choice { selected: 1, .. })
        )); // full is index 1
        let restored = state.to_config();
        assert_eq!(restored.verify_level.as_deref(), Some("full"));
    }

    #[test]
    fn test_settings_verify_level_default_quick() {
        let config = crate::config::Config::default();
        let state = SettingsState::from_config(&config);
        let level = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Choice { key, .. } if key == "verify_level"));
        assert!(matches!(
            level,
            Some(SettingItem::Choice { selected: 0, .. })
        )); // quick is index 0
        let restored = state.to_config();
        assert!(restored.verify_level.is_none()); // quick is default, so not serialized
    }

    #[test]
    fn test_playlist_default_stream_counts() {
        let pl = Playlist {
            num: "00001".into(),
            duration: "1:00:00".into(),
            seconds: 3600,
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
        };
        assert_eq!(pl.video_streams, 0);
        assert_eq!(pl.audio_streams, 0);
        assert_eq!(pl.subtitle_streams, 0);
    }

    // --- Settings roundtrip tests for stream fields ---

    #[test]
    fn test_settings_streams_roundtrip() {
        let config = crate::config::Config {
            streams: Some(crate::config::StreamsConfig {
                audio_languages: Some(vec!["eng".into(), "jpn".into()]),
                subtitle_languages: Some(vec!["eng".into()]),
                prefer_surround: Some(true),
            }),
            ..Default::default()
        };
        let state = SettingsState::from_config(&config);

        let audio_lang = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Text { key, .. } if key == "audio_languages"));
        assert!(
            matches!(audio_lang, Some(SettingItem::Text { value, .. }) if value == "eng,jpn"),
            "audio_languages should be 'eng,jpn'"
        );

        let sub_lang = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Text { key, .. } if key == "subtitle_languages"));
        assert!(
            matches!(sub_lang, Some(SettingItem::Text { value, .. }) if value == "eng"),
            "subtitle_languages should be 'eng'"
        );

        let surround = state
            .items
            .iter()
            .find(|i| matches!(i, SettingItem::Toggle { key, .. } if key == "prefer_surround"));
        assert!(
            matches!(surround, Some(SettingItem::Toggle { value: true, .. })),
            "prefer_surround should be true"
        );

        let restored = state.to_config();
        let streams = restored.streams.unwrap();
        assert_eq!(
            streams.audio_languages,
            Some(vec!["eng".into(), "jpn".into()])
        );
        assert_eq!(streams.subtitle_languages, Some(vec!["eng".into()]));
        assert_eq!(streams.prefer_surround, Some(true));
    }

    #[test]
    fn test_settings_streams_empty_roundtrip() {
        let config = crate::config::Config::default();
        let state = SettingsState::from_config(&config);
        let restored = state.to_config();
        assert!(
            restored.streams.is_none(),
            "empty streams config should not create [streams] section"
        );
    }
}
