//! SQLite-backed rip history database.

#![allow(dead_code)] // Types and methods used in future tasks

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA_VERSION: i64 = 1;
const STALE_SESSION_HOURS: i64 = 4;

// ============================================================================
// Enums
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Scanned,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl SessionStatus {
    pub fn as_str(&self) -> &str {
        match self {
            SessionStatus::Scanned => "scanned",
            SessionStatus::InProgress => "in_progress",
            SessionStatus::Completed => "completed",
            SessionStatus::Failed => "failed",
            SessionStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "scanned" => Some(SessionStatus::Scanned),
            "in_progress" => Some(SessionStatus::InProgress),
            "completed" => Some(SessionStatus::Completed),
            "failed" => Some(SessionStatus::Failed),
            "cancelled" => Some(SessionStatus::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    InProgress,
    Completed,
    Failed,
    Skipped,
}

impl FileStatus {
    pub fn as_str(&self) -> &str {
        match self {
            FileStatus::InProgress => "in_progress",
            FileStatus::Completed => "completed",
            FileStatus::Failed => "failed",
            FileStatus::Skipped => "skipped",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "in_progress" => Some(FileStatus::InProgress),
            "completed" => Some(FileStatus::Completed),
            "failed" => Some(FileStatus::Failed),
            "skipped" => Some(FileStatus::Skipped),
            _ => None,
        }
    }
}

// ============================================================================
// Input Types
// ============================================================================

pub struct SessionInfo {
    pub volume_label: String,
    pub device: Option<String>,
    pub tmdb_id: Option<i64>,
    pub tmdb_type: Option<String>,
    pub title: String,
    pub season: Option<i32>,
    pub disc_number: Option<i32>,
    pub batch_id: Option<String>,
    pub config_snapshot: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscPlaylistInfo {
    pub playlist: String,
    pub duration_ms: Option<i64>,
    pub video_streams: Option<i32>,
    pub audio_streams: Option<i32>,
    pub subtitle_streams: Option<i32>,
    pub chapters: Option<i32>,
    pub is_filtered: bool,
}

pub struct RippedFileInfo {
    pub playlist: String,
    pub episodes: Option<String>,
    pub output_path: String,
    pub file_size: Option<i64>,
    pub duration_ms: Option<i64>,
    pub streams: Option<String>,
    pub chapters: Option<i32>,
}

// ============================================================================
// Output Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: i64,
    pub title: String,
    pub volume_label: String,
    pub season: Option<i32>,
    pub disc_number: Option<i32>,
    pub started_at: String,
    pub files_completed: i64,
    pub files_total: i64,
    pub total_size: i64,
    pub status: SessionStatus,
    pub batch_id: Option<String>,
}

impl SessionSummary {
    pub fn display_status(&self) -> &str {
        if self.status == SessionStatus::Completed && self.files_completed < self.files_total {
            "partial"
        } else {
            self.status.as_str()
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RippedFileDetail {
    pub id: i64,
    pub playlist: String,
    pub episodes: Option<String>,
    pub output_path: String,
    pub file_size: Option<i64>,
    pub duration_ms: Option<i64>,
    pub chapters: Option<i32>,
    pub status: FileStatus,
    pub error: Option<String>,
    pub verified: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub device: Option<String>,
    pub tmdb_id: Option<i64>,
    pub tmdb_type: Option<String>,
    pub finished_at: Option<String>,
    pub config_snapshot: Option<String>,
    pub playlists: Vec<DiscPlaylistInfo>,
    pub files: Vec<RippedFileDetail>,
}

#[derive(Default)]
pub struct SessionFilter {
    pub limit: Option<i64>,
    pub status: Option<SessionStatus>,
    pub title: Option<String>,
    pub since: Option<String>,
    pub season: Option<i32>,
    pub batch_id: Option<String>,
}

#[derive(Default)]
pub struct HistoryStats {
    pub total_sessions: i64,
    pub completed_sessions: i64,
    pub partial_sessions: i64,
    pub failed_sessions: i64,
    pub scanned_sessions: i64,
    pub total_files: i64,
    pub failed_files: i64,
    pub skipped_files: i64,
    pub total_size: i64,
    pub first_session: Option<String>,
    pub last_session: Option<String>,
    pub batch_count: i64,
}

// ============================================================================
// Path Resolution
// ============================================================================

/// Resolve the history DB path: BLUBACK_HISTORY_PATH env → config → XDG default.
pub fn resolve_db_path(config: Option<&crate::config::Config>) -> std::path::PathBuf {
    if let Ok(path) = std::env::var("BLUBACK_HISTORY_PATH") {
        return std::path::PathBuf::from(path);
    }
    if let Some(path) = config.and_then(|c| c.history_path()) {
        return std::path::PathBuf::from(path);
    }
    let data_dir = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home).join(".local/share")
        });
    data_dir.join("bluback").join("history.db")
}

// ============================================================================
// Config Snapshot
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    pub output_dir: String,
    pub tv_format: String,
    pub movie_format: String,
    pub special_format: String,
    pub preset: String,
    pub audio_languages: Vec<String>,
    pub subtitle_languages: Vec<String>,
    pub prefer_surround: bool,
    pub stream_selection: String,
    pub aacs_backend: String,
    pub reserve_index_space: u32,
    pub verify: bool,
    pub verify_level: String,
}

impl ConfigSnapshot {
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self {
            output_dir: config.output_dir.clone().unwrap_or_else(|| ".".to_string()),
            tv_format: config.tv_format.clone().unwrap_or_default(),
            movie_format: config.movie_format.clone().unwrap_or_default(),
            special_format: config.special_format.clone().unwrap_or_default(),
            preset: config
                .preset
                .clone()
                .unwrap_or_else(|| "default".to_string()),
            audio_languages: config
                .streams
                .as_ref()
                .and_then(|s| s.audio_languages.clone())
                .unwrap_or_default(),
            subtitle_languages: config
                .streams
                .as_ref()
                .and_then(|s| s.subtitle_languages.clone())
                .unwrap_or_default(),
            prefer_surround: config
                .streams
                .as_ref()
                .and_then(|s| s.prefer_surround)
                .unwrap_or(false),
            stream_selection: config
                .stream_selection
                .clone()
                .unwrap_or_else(|| "all".to_string()),
            aacs_backend: config
                .aacs_backend
                .clone()
                .unwrap_or_else(|| "auto".to_string()),
            reserve_index_space: config.reserve_index_space.unwrap_or(500),
            verify: config.verify.unwrap_or(false),
            verify_level: config
                .verify_level
                .clone()
                .unwrap_or_else(|| "quick".to_string()),
        }
    }
}

// ============================================================================
// Database
// ============================================================================

pub struct HistoryDb {
    conn: Connection,
}

impl HistoryDb {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create history database directory")?;
        }

        let conn = Connection::open(path).context("Failed to open history database")?;
        let mut db = Self { conn };
        db.init()?;
        Ok(db)
    }

    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("Failed to open in-memory database")?;
        let mut db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&mut self) -> Result<()> {
        // Set pragmas
        self.conn
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .context("Failed to set database pragmas")?;

        // Run migrations
        self.run_migrations()?;

        // Clean up stale in_progress sessions
        self.cleanup_stale_sessions()?;

        Ok(())
    }

    fn run_migrations(&mut self) -> Result<()> {
        let current_version = self.get_schema_version()?;

        if current_version == 0 {
            self.apply_migration_1()?;
        }

        Ok(())
    }

    fn get_schema_version(&self) -> Result<i64> {
        let version: Result<i64, rusqlite::Error> =
            self.conn
                .query_row("SELECT version FROM schema_version", [], |row| row.get(0));

        match version {
            Ok(v) => Ok(v),
            Err(rusqlite::Error::SqliteFailure(_, _)) => Ok(0),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(e.into()),
        }
    }

    fn apply_migration_1(&mut self) -> Result<()> {
        self.conn
            .execute_batch(
                r#"
                CREATE TABLE schema_version (version INTEGER NOT NULL);

                CREATE TABLE sessions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    volume_label TEXT NOT NULL,
                    device TEXT,
                    tmdb_id INTEGER,
                    tmdb_type TEXT CHECK(tmdb_type IN ('tv', 'movie')),
                    title TEXT NOT NULL,
                    season INTEGER,
                    disc_number INTEGER,
                    started_at TEXT NOT NULL,
                    finished_at TEXT,
                    status TEXT NOT NULL DEFAULT 'scanned' CHECK(status IN ('scanned', 'in_progress', 'completed', 'failed', 'cancelled')),
                    batch_id TEXT,
                    config_snapshot TEXT
                );

                CREATE TABLE disc_playlists (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    playlist TEXT NOT NULL,
                    duration_ms INTEGER,
                    video_streams INTEGER,
                    audio_streams INTEGER,
                    subtitle_streams INTEGER,
                    chapters INTEGER,
                    is_filtered BOOLEAN DEFAULT 0,
                    UNIQUE(session_id, playlist)
                );

                CREATE TABLE ripped_files (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    playlist TEXT NOT NULL,
                    episodes TEXT,
                    output_path TEXT NOT NULL,
                    file_size INTEGER,
                    duration_ms INTEGER,
                    streams TEXT,
                    chapters INTEGER,
                    status TEXT NOT NULL DEFAULT 'in_progress' CHECK(status IN ('in_progress', 'completed', 'failed', 'skipped')),
                    error TEXT,
                    verified TEXT CHECK(verified IN ('passed', 'failed', 'skipped')),
                    started_at TEXT NOT NULL,
                    finished_at TEXT,
                    UNIQUE(session_id, playlist)
                );

                CREATE INDEX idx_sessions_tmdb ON sessions(tmdb_id, tmdb_type, season);
                CREATE INDEX idx_sessions_label ON sessions(volume_label);
                CREATE INDEX idx_sessions_status ON sessions(status);
                CREATE INDEX idx_sessions_started ON sessions(started_at);
                CREATE INDEX idx_ripped_files_session ON ripped_files(session_id);
                "#,
            )
            .context("Failed to apply migration 1")?;

        self.conn
            .execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )
            .context("Failed to set schema version")?;

        Ok(())
    }

    fn cleanup_stale_sessions(&mut self) -> Result<()> {
        let cutoff = now_iso8601_minus_hours(STALE_SESSION_HOURS);
        let now = now_iso8601();

        // First, mark stale in-progress files as failed
        self.conn
            .execute(
                "UPDATE ripped_files SET status = 'failed', error = 'session interrupted (stale cleanup)', finished_at = ?1 WHERE session_id IN (SELECT id FROM sessions WHERE status = 'in_progress' AND started_at < ?2) AND status = 'in_progress'",
                params![now, cutoff],
            )
            .context("Failed to clean up stale ripped_files")?;

        // Then mark stale sessions as failed
        self.conn
            .execute(
                "UPDATE sessions SET status = 'failed', finished_at = ?1 WHERE status = 'in_progress' AND started_at < ?2",
                params![now, cutoff],
            )
            .context("Failed to clean up stale sessions")?;

        Ok(())
    }

    // ========================================================================
    // Recording Methods
    // ========================================================================

    pub fn start_session(&self, info: &SessionInfo) -> Result<i64> {
        self.conn
            .execute(
                r#"INSERT INTO sessions (volume_label, device, tmdb_id, tmdb_type, title, season, disc_number, started_at, status, batch_id, config_snapshot)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"#,
                params![
                    info.volume_label,
                    info.device,
                    info.tmdb_id,
                    info.tmdb_type,
                    info.title,
                    info.season,
                    info.disc_number,
                    now_iso8601(),
                    SessionStatus::Scanned.as_str(),
                    info.batch_id,
                    info.config_snapshot,
                ],
            )
            .context("Failed to insert session")?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_session(&self, id: i64, status: SessionStatus) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET status = ?1, finished_at = ?2 WHERE id = ?3",
                params![status.as_str(), now_iso8601(), id],
            )
            .context("Failed to update session status")?;

        Ok(())
    }

    pub fn record_disc_playlists(
        &self,
        session_id: i64,
        playlists: &[DiscPlaylistInfo],
    ) -> Result<()> {
        let tx = self
            .conn
            .unchecked_transaction()
            .context("Failed to begin transaction")?;

        for playlist in playlists {
            tx.execute(
                r#"INSERT INTO disc_playlists (session_id, playlist, duration_ms, video_streams, audio_streams, subtitle_streams, chapters, is_filtered)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
                params![
                    session_id,
                    playlist.playlist,
                    playlist.duration_ms,
                    playlist.video_streams,
                    playlist.audio_streams,
                    playlist.subtitle_streams,
                    playlist.chapters,
                    playlist.is_filtered,
                ],
            )
            .context("Failed to insert playlist")?;
        }

        tx.commit()
            .context("Failed to commit playlist transaction")?;
        Ok(())
    }

    pub fn record_file(&self, session_id: i64, file: &RippedFileInfo) -> Result<i64> {
        self.conn
            .execute(
                r#"INSERT INTO ripped_files (session_id, playlist, episodes, output_path, file_size, duration_ms, streams, chapters, started_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                   ON CONFLICT(session_id, playlist) DO UPDATE SET
                       episodes = excluded.episodes,
                       output_path = excluded.output_path,
                       file_size = COALESCE(excluded.file_size, ripped_files.file_size),
                       duration_ms = COALESCE(excluded.duration_ms, ripped_files.duration_ms),
                       streams = COALESCE(excluded.streams, ripped_files.streams),
                       chapters = COALESCE(excluded.chapters, ripped_files.chapters)"#,
                params![
                    session_id,
                    file.playlist,
                    file.episodes,
                    file.output_path,
                    file.file_size,
                    file.duration_ms,
                    file.streams,
                    file.chapters,
                    now_iso8601(),
                ],
            )
            .context("Failed to upsert ripped file")?;

        // Query the stable ID after upsert
        let id: i64 = self
            .conn
            .query_row(
                "SELECT id FROM ripped_files WHERE session_id = ?1 AND playlist = ?2",
                params![session_id, file.playlist],
                |row| row.get(0),
            )
            .context("Failed to retrieve file ID after upsert")?;

        Ok(id)
    }

    pub fn update_file_status(
        &self,
        file_id: i64,
        status: FileStatus,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE ripped_files SET status = ?1, finished_at = ?2, error = ?3 WHERE id = ?4",
                params![status.as_str(), now_iso8601(), error, file_id],
            )
            .context("Failed to update file status")?;

        Ok(())
    }

    // ========================================================================
    // Query Methods
    // ========================================================================

    pub fn last_episode(&self, tmdb_id: i64, season: i32) -> Result<Option<i32>> {
        let result = self
            .conn
            .query_row(
                r#"SELECT MAX(CAST(je.value AS INTEGER))
                   FROM sessions s
                   JOIN ripped_files rf ON rf.session_id = s.id, json_each(rf.episodes) je
                   WHERE s.tmdb_id = ?1 AND s.season = ?2 AND rf.status = 'completed'
                     AND typeof(je.value) = 'integer'"#,
                params![tmdb_id, season],
                |row| row.get::<_, Option<i32>>(0),
            )
            .context("Failed to query last episode")?;

        Ok(result)
    }

    pub fn last_episode_by_label(&self, label_pattern: &str, season: i32) -> Result<Option<i32>> {
        let result = self
            .conn
            .query_row(
                r#"SELECT MAX(CAST(je.value AS INTEGER))
                   FROM sessions s
                   JOIN ripped_files rf ON rf.session_id = s.id, json_each(rf.episodes) je
                   WHERE s.volume_label LIKE ?1 AND s.season = ?2 AND rf.status = 'completed'
                     AND typeof(je.value) = 'integer'"#,
                params![label_pattern, season],
                |row| row.get::<_, Option<i32>>(0),
            )
            .context("Failed to query last episode by label")?;

        Ok(result)
    }

    pub fn last_special(&self, tmdb_id: i64, season: i32) -> Result<Option<i32>> {
        let result = self
            .conn
            .query_row(
                r#"SELECT MAX(CAST(SUBSTR(je.value, 3) AS INTEGER))
                   FROM sessions s
                   JOIN ripped_files rf ON rf.session_id = s.id, json_each(rf.episodes) je
                   WHERE s.tmdb_id = ?1 AND s.season = ?2 AND rf.status = 'completed'
                     AND typeof(je.value) = 'text' AND je.value LIKE 'SP%'"#,
                params![tmdb_id, season],
                |row| row.get::<_, Option<i32>>(0),
            )
            .context("Failed to query last special")?;

        Ok(result)
    }

    pub fn last_special_by_label(&self, label_pattern: &str, season: i32) -> Result<Option<i32>> {
        let result = self
            .conn
            .query_row(
                r#"SELECT MAX(CAST(SUBSTR(je.value, 3) AS INTEGER))
                   FROM sessions s
                   JOIN ripped_files rf ON rf.session_id = s.id, json_each(rf.episodes) je
                   WHERE s.volume_label LIKE ?1 AND s.season = ?2 AND rf.status = 'completed'
                     AND typeof(je.value) = 'text' AND je.value LIKE 'SP%'"#,
                params![label_pattern, season],
                |row| row.get::<_, Option<i32>>(0),
            )
            .context("Failed to query last special by label")?;

        Ok(result)
    }

    // ========================================================================
    // Duplicate Detection
    // ========================================================================

    pub fn find_session_by_label(&self, label: &str) -> Result<Vec<SessionSummary>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT
                s.id, s.title, s.volume_label, s.season, s.disc_number,
                s.started_at, s.status, s.batch_id,
                COALESCE((SELECT COUNT(*) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status = 'completed'), 0) as files_completed,
                COALESCE((SELECT COUNT(*) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status IN ('completed', 'failed')), 0) as files_total,
                COALESCE((SELECT SUM(rf.file_size) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status = 'completed'), 0) as total_size
            FROM sessions s
            WHERE s.volume_label = ?1
            ORDER BY s.started_at DESC"#,
        )
        .context("Failed to prepare find_session_by_label statement")?;

        let sessions = stmt
            .query_map(params![label], |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    volume_label: row.get(2)?,
                    season: row.get(3)?,
                    disc_number: row.get(4)?,
                    started_at: row.get(5)?,
                    status: SessionStatus::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or(SessionStatus::Failed),
                    batch_id: row.get(7)?,
                    files_completed: row.get(8)?,
                    files_total: row.get(9)?,
                    total_size: row.get(10)?,
                })
            })
            .context("Failed to query sessions by label")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to collect session results")?;

        Ok(sessions)
    }

    pub fn find_session_by_tmdb(
        &self,
        tmdb_id: i64,
        tmdb_type: &str,
        season: Option<i32>,
    ) -> Result<Vec<SessionSummary>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT
                s.id, s.title, s.volume_label, s.season, s.disc_number,
                s.started_at, s.status, s.batch_id,
                COALESCE((SELECT COUNT(*) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status = 'completed'), 0) as files_completed,
                COALESCE((SELECT COUNT(*) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status IN ('completed', 'failed')), 0) as files_total,
                COALESCE((SELECT SUM(rf.file_size) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status = 'completed'), 0) as total_size
            FROM sessions s
            WHERE s.tmdb_id = ?1 AND s.tmdb_type = ?2 AND (s.season = ?3 OR ?3 IS NULL)
            ORDER BY s.started_at DESC"#,
        )
        .context("Failed to prepare find_session_by_tmdb statement")?;

        let sessions = stmt
            .query_map(params![tmdb_id, tmdb_type, season], |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    volume_label: row.get(2)?,
                    season: row.get(3)?,
                    disc_number: row.get(4)?,
                    started_at: row.get(5)?,
                    status: SessionStatus::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or(SessionStatus::Failed),
                    batch_id: row.get(7)?,
                    files_completed: row.get(8)?,
                    files_total: row.get(9)?,
                    total_size: row.get(10)?,
                })
            })
            .context("Failed to query sessions by tmdb")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to collect session results")?;

        Ok(sessions)
    }

    // ========================================================================
    // Listing
    // ========================================================================

    pub fn list_sessions(&self, filter: &SessionFilter) -> Result<Vec<SessionSummary>> {
        let mut where_clauses = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref status) = filter.status {
            where_clauses.push("s.status = ?");
            params_vec.push(Box::new(status.as_str().to_string()));
        }

        if let Some(ref title) = filter.title {
            where_clauses.push("s.title LIKE '%' || ? || '%'");
            params_vec.push(Box::new(title.clone()));
        }

        if let Some(ref since) = filter.since {
            where_clauses.push("s.started_at >= ?");
            params_vec.push(Box::new(since.clone()));
        }

        if let Some(season) = filter.season {
            where_clauses.push("s.season = ?");
            params_vec.push(Box::new(season));
        }

        if let Some(ref batch_id) = filter.batch_id {
            where_clauses.push("s.batch_id = ?");
            params_vec.push(Box::new(batch_id.clone()));
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        let sql = if let Some(limit) = filter.limit {
            params_vec.push(Box::new(limit));
            format!(
                r#"SELECT
                    s.id, s.title, s.volume_label, s.season, s.disc_number,
                    s.started_at, s.status, s.batch_id,
                    COALESCE((SELECT COUNT(*) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status = 'completed'), 0) as files_completed,
                    COALESCE((SELECT COUNT(*) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status IN ('completed', 'failed')), 0) as files_total,
                    COALESCE((SELECT SUM(rf.file_size) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status = 'completed'), 0) as total_size
                FROM sessions s
                {}
                ORDER BY s.started_at DESC
                LIMIT ?"#,
                where_sql
            )
        } else {
            format!(
                r#"SELECT
                    s.id, s.title, s.volume_label, s.season, s.disc_number,
                    s.started_at, s.status, s.batch_id,
                    COALESCE((SELECT COUNT(*) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status = 'completed'), 0) as files_completed,
                    COALESCE((SELECT COUNT(*) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status IN ('completed', 'failed')), 0) as files_total,
                    COALESCE((SELECT SUM(rf.file_size) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status = 'completed'), 0) as total_size
                FROM sessions s
                {}
                ORDER BY s.started_at DESC"#,
                where_sql
            )
        };

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self
            .conn
            .prepare(&sql)
            .context("Failed to prepare list_sessions statement")?;

        let sessions = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    volume_label: row.get(2)?,
                    season: row.get(3)?,
                    disc_number: row.get(4)?,
                    started_at: row.get(5)?,
                    status: SessionStatus::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or(SessionStatus::Failed),
                    batch_id: row.get(7)?,
                    files_completed: row.get(8)?,
                    files_total: row.get(9)?,
                    total_size: row.get(10)?,
                })
            })
            .context("Failed to query sessions")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to collect session results")?;

        Ok(sessions)
    }

    pub fn get_session(&self, id: i64) -> Result<Option<SessionDetail>> {
        let summary: Option<SessionSummary> = {
            let mut stmt = self.conn.prepare(
                r#"SELECT
                    s.id, s.title, s.volume_label, s.season, s.disc_number,
                    s.started_at, s.status, s.batch_id,
                    COALESCE((SELECT COUNT(*) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status = 'completed'), 0) as files_completed,
                    COALESCE((SELECT COUNT(*) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status IN ('completed', 'failed')), 0) as files_total,
                    COALESCE((SELECT SUM(rf.file_size) FROM ripped_files rf WHERE rf.session_id = s.id AND rf.status = 'completed'), 0) as total_size
                FROM sessions s
                WHERE s.id = ?1"#,
            )
            .context("Failed to prepare get_session statement")?;

            stmt.query_row(params![id], |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    volume_label: row.get(2)?,
                    season: row.get(3)?,
                    disc_number: row.get(4)?,
                    started_at: row.get(5)?,
                    status: SessionStatus::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or(SessionStatus::Failed),
                    batch_id: row.get(7)?,
                    files_completed: row.get(8)?,
                    files_total: row.get(9)?,
                    total_size: row.get(10)?,
                })
            })
            .optional()
            .context("Failed to query session")?
        };

        let summary = match summary {
            Some(s) => s,
            None => return Ok(None),
        };

        #[allow(clippy::type_complexity)]
        let (device, tmdb_id, tmdb_type, finished_at, config_snapshot): (
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = self
            .conn
            .query_row(
                "SELECT device, tmdb_id, tmdb_type, finished_at, config_snapshot FROM sessions WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .context("Failed to query session details")?;

        let mut playlist_stmt = self
            .conn
            .prepare(
                "SELECT playlist, duration_ms, video_streams, audio_streams, subtitle_streams, chapters, is_filtered FROM disc_playlists WHERE session_id = ?1",
            )
            .context("Failed to prepare playlist query")?;

        let playlists = playlist_stmt
            .query_map(params![id], |row| {
                Ok(DiscPlaylistInfo {
                    playlist: row.get(0)?,
                    duration_ms: row.get(1)?,
                    video_streams: row.get(2)?,
                    audio_streams: row.get(3)?,
                    subtitle_streams: row.get(4)?,
                    chapters: row.get(5)?,
                    is_filtered: row.get(6)?,
                })
            })
            .context("Failed to query playlists")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to collect playlists")?;

        let mut file_stmt = self
            .conn
            .prepare(
                "SELECT id, playlist, episodes, output_path, file_size, duration_ms, chapters, status, error, verified FROM ripped_files WHERE session_id = ?1",
            )
            .context("Failed to prepare file query")?;

        let files = file_stmt
            .query_map(params![id], |row| {
                Ok(RippedFileDetail {
                    id: row.get(0)?,
                    playlist: row.get(1)?,
                    episodes: row.get(2)?,
                    output_path: row.get(3)?,
                    file_size: row.get(4)?,
                    duration_ms: row.get(5)?,
                    chapters: row.get(6)?,
                    status: FileStatus::from_str(&row.get::<_, String>(7)?)
                        .unwrap_or(FileStatus::Failed),
                    error: row.get(8)?,
                    verified: row.get(9)?,
                })
            })
            .context("Failed to query files")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to collect files")?;

        Ok(Some(SessionDetail {
            summary,
            device,
            tmdb_id,
            tmdb_type,
            finished_at,
            config_snapshot,
            playlists,
            files,
        }))
    }

    pub fn stats(&self) -> Result<HistoryStats> {
        #[allow(clippy::type_complexity)]
        let (
            total_sessions,
            completed_sessions,
            failed_sessions,
            scanned_sessions,
            first_session,
            last_session,
        ): (
            i64,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
            Option<String>,
        ) = self
            .conn
            .query_row(
                r#"SELECT
                    COUNT(*),
                    SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'scanned' THEN 1 ELSE 0 END),
                    MIN(started_at),
                    MAX(started_at)
                FROM sessions"#,
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .context("Failed to query session stats")?;

        let (total_files, failed_files, skipped_files, total_size): (
            i64,
            Option<i64>,
            Option<i64>,
            i64,
        ) = self
            .conn
            .query_row(
                r#"SELECT
                    COUNT(*),
                    SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'skipped' THEN 1 ELSE 0 END),
                    COALESCE(SUM(CASE WHEN status = 'completed' THEN file_size ELSE 0 END), 0)
                FROM ripped_files"#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .context("Failed to query file stats")?;

        let batch_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(DISTINCT batch_id) FROM sessions WHERE batch_id IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .context("Failed to query batch count")?;

        let partial_sessions: i64 = self
            .conn
            .query_row(
                r#"SELECT COUNT(DISTINCT s.id)
                FROM sessions s
                WHERE s.status = 'completed'
                AND EXISTS (
                    SELECT 1 FROM ripped_files rf
                    WHERE rf.session_id = s.id AND rf.status = 'failed'
                )
                AND EXISTS (
                    SELECT 1 FROM ripped_files rf
                    WHERE rf.session_id = s.id AND rf.status = 'completed'
                )"#,
                [],
                |row| row.get(0),
            )
            .context("Failed to query partial sessions")?;

        Ok(HistoryStats {
            total_sessions,
            completed_sessions: completed_sessions.unwrap_or(0),
            partial_sessions,
            failed_sessions: failed_sessions.unwrap_or(0),
            scanned_sessions: scanned_sessions.unwrap_or(0),
            total_files,
            failed_files: failed_files.unwrap_or(0),
            skipped_files: skipped_files.unwrap_or(0),
            total_size,
            first_session,
            last_session,
            batch_count,
        })
    }

    // ========================================================================
    // Management
    // ========================================================================

    pub fn delete_session(&self, id: i64) -> Result<bool> {
        let count = self
            .conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])
            .context("Failed to delete session")?;

        Ok(count > 0)
    }

    pub fn clear_all(&self) -> Result<u64> {
        let count = self
            .conn
            .execute("DELETE FROM sessions", [])
            .context("Failed to clear all sessions")?;

        Ok(count as u64)
    }

    pub fn prune(&self, cutoff: &str, statuses: Option<&[SessionStatus]>) -> Result<u64> {
        let count = if let Some(statuses) = statuses {
            let placeholders = statuses.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "DELETE FROM sessions WHERE started_at < ?1 AND status IN ({})",
                placeholders
            );

            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(cutoff.to_string())];
            for status in statuses {
                params_vec.push(Box::new(status.as_str().to_string()));
            }
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();

            self.conn
                .execute(&sql, params_refs.as_slice())
                .context("Failed to prune sessions")?
        } else {
            self.conn
                .execute(
                    "DELETE FROM sessions WHERE started_at < ?1",
                    params![cutoff],
                )
                .context("Failed to prune sessions")?
        };

        Ok(count as u64)
    }

    // ========================================================================
    // Export
    // ========================================================================

    pub fn export_json(&self, writer: &mut dyn std::io::Write) -> Result<()> {
        let sessions = self.list_sessions(&SessionFilter {
            limit: None,
            ..Default::default()
        })?;

        let mut details = Vec::new();
        for summary in sessions {
            if let Some(detail) = self.get_session(summary.id)? {
                details.push(detail);
            }
        }

        serde_json::to_writer_pretty(writer, &details)
            .context("Failed to serialize sessions to JSON")?;

        Ok(())
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn now_iso8601() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn now_iso8601_minus_hours(hours: i64) -> String {
    let now = chrono::Utc::now();
    let cutoff = now - chrono::Duration::hours(hours);
    cutoff.format("%Y-%m-%dT%H:%M:%S").to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_memory_creates_schema() {
        let db = HistoryDb::open_memory().unwrap();
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM disc_playlists", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM ripped_files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_schema_version_set() {
        let db = HistoryDb::open_memory().unwrap();
        let version: i64 = db
            .conn
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn test_wal_mode_enabled() {
        let dir = std::env::temp_dir().join("bluback_test_wal");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_wal.db");
        let _ = std::fs::remove_file(&path);
        let db = HistoryDb::open(&path).unwrap();
        let mode: String = db
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        drop(db);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
        let _ = std::fs::remove_dir(dir);
    }

    #[test]
    fn test_check_constraints_reject_invalid_status() {
        let db = HistoryDb::open_memory().unwrap();
        let result = db.conn.execute(
            "INSERT INTO sessions (volume_label, title, started_at, status) VALUES (?1, ?2, ?3, ?4)",
            params!["DISC", "Test", "2026-04-13T00:00:00", "bogus"],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_check_constraints_accept_valid_status() {
        let db = HistoryDb::open_memory().unwrap();
        for status in &["scanned", "in_progress", "completed", "failed", "cancelled"] {
            let result = db.conn.execute(
                "INSERT INTO sessions (volume_label, title, started_at, status) VALUES (?1, ?2, ?3, ?4)",
                params![format!("DISC_{}", status), "Test", "2026-04-13T00:00:00", status],
            );
            assert!(result.is_ok(), "status '{}' should be valid", status);
        }
    }

    #[test]
    fn test_unique_constraint_disc_playlists() {
        let db = HistoryDb::open_memory().unwrap();
        db.conn
            .execute(
                "INSERT INTO sessions (volume_label, title, started_at, status) VALUES ('D', 'T', '2026-04-13T00:00:00', 'scanned')",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO disc_playlists (session_id, playlist) VALUES (1, '00800')",
                [],
            )
            .unwrap();
        let dup = db.conn.execute(
            "INSERT INTO disc_playlists (session_id, playlist) VALUES (1, '00800')",
            [],
        );
        assert!(dup.is_err());
    }

    #[test]
    fn test_cascade_delete() {
        let db = HistoryDb::open_memory().unwrap();
        db.conn.execute("INSERT INTO sessions (volume_label, title, started_at, status) VALUES ('D', 'T', '2026-04-13T00:00:00', 'scanned')", []).unwrap();
        db.conn
            .execute(
                "INSERT INTO disc_playlists (session_id, playlist) VALUES (1, '00800')",
                [],
            )
            .unwrap();
        db.conn.execute("INSERT INTO ripped_files (session_id, playlist, output_path, started_at) VALUES (1, '00800', '/tmp/test.mkv', '2026-04-13T00:00:00')", []).unwrap();
        db.conn
            .execute("DELETE FROM sessions WHERE id = 1", [])
            .unwrap();
        let pl_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM disc_playlists", [], |r| r.get(0))
            .unwrap();
        let rf_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM ripped_files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(pl_count, 0);
        assert_eq!(rf_count, 0);
    }

    // Helper to reduce boilerplate in tests
    fn make_session_info(label: &str, title: &str) -> SessionInfo {
        SessionInfo {
            volume_label: label.to_string(),
            device: None,
            tmdb_id: None,
            tmdb_type: None,
            title: title.to_string(),
            season: None,
            disc_number: None,
            batch_id: None,
            config_snapshot: None,
        }
    }

    #[test]
    fn test_start_session() {
        let db = HistoryDb::open_memory().unwrap();
        let info = SessionInfo {
            volume_label: "BREAKING_BAD_S1_D1".to_string(),
            device: Some("/dev/sr0".to_string()),
            tmdb_id: Some(1396),
            tmdb_type: Some("tv".to_string()),
            title: "Breaking Bad".to_string(),
            season: Some(1),
            disc_number: Some(1),
            batch_id: None,
            config_snapshot: None,
        };
        let id = db.start_session(&info).unwrap();
        assert_eq!(id, 1);

        let status: String = db
            .conn
            .query_row("SELECT status FROM sessions WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "scanned");
    }

    #[test]
    fn test_finish_session() {
        let db = HistoryDb::open_memory().unwrap();
        let id = db
            .start_session(&make_session_info("DISC", "Test"))
            .unwrap();
        db.finish_session(id, SessionStatus::Completed).unwrap();

        let status: String = db
            .conn
            .query_row("SELECT status FROM sessions WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "completed");

        let finished: Option<String> = db
            .conn
            .query_row(
                "SELECT finished_at FROM sessions WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(finished.is_some());
    }

    #[test]
    fn test_record_disc_playlists() {
        let db = HistoryDb::open_memory().unwrap();
        let id = db
            .start_session(&make_session_info("DISC", "Test"))
            .unwrap();
        let playlists = vec![
            DiscPlaylistInfo {
                playlist: "00800".to_string(),
                duration_ms: Some(2700000),
                video_streams: Some(1),
                audio_streams: Some(2),
                subtitle_streams: Some(3),
                chapters: Some(12),
                is_filtered: false,
            },
            DiscPlaylistInfo {
                playlist: "00801".to_string(),
                duration_ms: Some(30000),
                video_streams: Some(1),
                audio_streams: Some(1),
                subtitle_streams: Some(0),
                chapters: Some(1),
                is_filtered: true,
            },
        ];
        db.record_disc_playlists(id, &playlists).unwrap();

        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM disc_playlists WHERE session_id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_record_file_and_update_status() {
        let db = HistoryDb::open_memory().unwrap();
        let session_id = db
            .start_session(&make_session_info("DISC", "Test"))
            .unwrap();
        let file_info = RippedFileInfo {
            playlist: "00800".to_string(),
            episodes: Some("[1,2]".to_string()),
            output_path: "/tmp/S01E01.mkv".to_string(),
            file_size: None,
            duration_ms: None,
            streams: None,
            chapters: Some(12),
        };
        let file_id = db.record_file(session_id, &file_info).unwrap();
        assert_eq!(file_id, 1);

        db.update_file_status(file_id, FileStatus::Completed, None)
            .unwrap();
        let status: String = db
            .conn
            .query_row(
                "SELECT status FROM ripped_files WHERE id = ?1",
                [file_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "completed");
    }

    #[test]
    fn test_upsert_preserves_file_id() {
        let db = HistoryDb::open_memory().unwrap();
        let session_id = db
            .start_session(&make_session_info("DISC", "Test"))
            .unwrap();
        let file_info = RippedFileInfo {
            playlist: "00800".to_string(),
            episodes: Some("[1]".to_string()),
            output_path: "/tmp/S01E01.mkv".to_string(),
            file_size: None,
            duration_ms: None,
            streams: None,
            chapters: None,
        };
        let file_id_1 = db.record_file(session_id, &file_info).unwrap();

        // Same playlist, same session — UPSERT should preserve row ID
        let file_info_retry = RippedFileInfo {
            playlist: "00800".to_string(),
            episodes: Some("[1]".to_string()),
            output_path: "/tmp/S01E01_retry.mkv".to_string(),
            file_size: None,
            duration_ms: None,
            streams: None,
            chapters: None,
        };
        let file_id_2 = db.record_file(session_id, &file_info_retry).unwrap();
        assert_eq!(file_id_1, file_id_2);
    }

    #[test]
    fn test_last_episode_by_tmdb() {
        let db = HistoryDb::open_memory().unwrap();
        let mut info = make_session_info("BB_S1_D1", "Breaking Bad");
        info.tmdb_id = Some(1396);
        info.tmdb_type = Some("tv".to_string());
        info.season = Some(1);
        let sid = db.start_session(&info).unwrap();

        for ep in 1..=4 {
            let f = RippedFileInfo {
                playlist: format!("008{:02}", ep),
                episodes: Some(format!("[{}]", ep)),
                output_path: format!("/tmp/S01E{:02}.mkv", ep),
                file_size: Some(5_000_000_000),
                duration_ms: Some(2700000),
                streams: None,
                chapters: None,
            };
            let fid = db.record_file(sid, &f).unwrap();
            db.update_file_status(fid, FileStatus::Completed, None)
                .unwrap();
        }
        db.finish_session(sid, SessionStatus::Completed).unwrap();

        let last = db.last_episode(1396, 1).unwrap();
        assert_eq!(last, Some(4));
    }

    #[test]
    fn test_last_episode_only_counts_completed() {
        let db = HistoryDb::open_memory().unwrap();
        let mut info = make_session_info("BB_S1_D1", "Breaking Bad");
        info.tmdb_id = Some(1396);
        info.tmdb_type = Some("tv".to_string());
        info.season = Some(1);
        let sid = db.start_session(&info).unwrap();

        let f1 = RippedFileInfo {
            playlist: "00800".to_string(),
            episodes: Some("[1]".to_string()),
            output_path: "/tmp/S01E01.mkv".to_string(),
            file_size: None,
            duration_ms: None,
            streams: None,
            chapters: None,
        };
        let fid1 = db.record_file(sid, &f1).unwrap();
        db.update_file_status(fid1, FileStatus::Completed, None)
            .unwrap();

        let f2 = RippedFileInfo {
            playlist: "00801".to_string(),
            episodes: Some("[2]".to_string()),
            output_path: "/tmp/S01E02.mkv".to_string(),
            file_size: None,
            duration_ms: None,
            streams: None,
            chapters: None,
        };
        let fid2 = db.record_file(sid, &f2).unwrap();
        db.update_file_status(fid2, FileStatus::Failed, Some("AACS error"))
            .unwrap();

        let last = db.last_episode(1396, 1).unwrap();
        assert_eq!(last, Some(1));
    }

    #[test]
    fn test_last_episode_multi_episode_playlist() {
        let db = HistoryDb::open_memory().unwrap();
        let mut info = make_session_info("BB_S1_D1", "Breaking Bad");
        info.tmdb_id = Some(1396);
        info.tmdb_type = Some("tv".to_string());
        info.season = Some(1);
        let sid = db.start_session(&info).unwrap();

        let f = RippedFileInfo {
            playlist: "00800".to_string(),
            episodes: Some("[3,4]".to_string()),
            output_path: "/tmp/S01E03-E04.mkv".to_string(),
            file_size: None,
            duration_ms: None,
            streams: None,
            chapters: None,
        };
        let fid = db.record_file(sid, &f).unwrap();
        db.update_file_status(fid, FileStatus::Completed, None)
            .unwrap();

        let last = db.last_episode(1396, 1).unwrap();
        assert_eq!(last, Some(4));
    }

    #[test]
    fn test_last_episode_no_match() {
        let db = HistoryDb::open_memory().unwrap();
        let last = db.last_episode(9999, 1).unwrap();
        assert_eq!(last, None);
    }

    #[test]
    fn test_last_special_by_tmdb() {
        let db = HistoryDb::open_memory().unwrap();
        let mut info = make_session_info("BB_S1_D1", "Breaking Bad");
        info.tmdb_id = Some(1396);
        info.tmdb_type = Some("tv".to_string());
        info.season = Some(1);
        let sid = db.start_session(&info).unwrap();

        let f = RippedFileInfo {
            playlist: "00810".to_string(),
            episodes: Some(r#"["SP1","SP2"]"#.to_string()),
            output_path: "/tmp/S01SP01.mkv".to_string(),
            file_size: None,
            duration_ms: None,
            streams: None,
            chapters: None,
        };
        let fid = db.record_file(sid, &f).unwrap();
        db.update_file_status(fid, FileStatus::Completed, None)
            .unwrap();

        let last = db.last_special(1396, 1).unwrap();
        assert_eq!(last, Some(2));
    }

    #[test]
    fn test_last_episode_by_label() {
        let db = HistoryDb::open_memory().unwrap();
        let mut info = make_session_info("BREAKING_BAD_S1_D1", "Breaking Bad");
        info.season = Some(1);
        let sid = db.start_session(&info).unwrap();

        let f = RippedFileInfo {
            playlist: "00800".to_string(),
            episodes: Some("[7]".to_string()),
            output_path: "/tmp/S01E07.mkv".to_string(),
            file_size: None,
            duration_ms: None,
            streams: None,
            chapters: None,
        };
        let fid = db.record_file(sid, &f).unwrap();
        db.update_file_status(fid, FileStatus::Completed, None)
            .unwrap();

        let last = db.last_episode_by_label("BREAKING_BAD%", 1).unwrap();
        assert_eq!(last, Some(7));
    }

    #[test]
    fn test_last_episode_by_label_no_cross_season() {
        let db = HistoryDb::open_memory().unwrap();
        let mut info = make_session_info("BB_S1_D1", "Breaking Bad");
        info.season = Some(1);
        let sid = db.start_session(&info).unwrap();
        let f = RippedFileInfo {
            playlist: "00800".to_string(),
            episodes: Some("[7]".to_string()),
            output_path: "/tmp/S01E07.mkv".to_string(),
            file_size: None,
            duration_ms: None,
            streams: None,
            chapters: None,
        };
        let fid = db.record_file(sid, &f).unwrap();
        db.update_file_status(fid, FileStatus::Completed, None)
            .unwrap();

        let last = db.last_episode_by_label("BB%", 2).unwrap();
        assert_eq!(last, None);
    }

    #[test]
    fn test_find_session_by_label() {
        let db = HistoryDb::open_memory().unwrap();
        let sid = db
            .start_session(&make_session_info("MY_DISC", "Test"))
            .unwrap();
        db.finish_session(sid, SessionStatus::Completed).unwrap();
        let results = db.find_session_by_label("MY_DISC").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].volume_label, "MY_DISC");
    }

    #[test]
    fn test_find_session_by_label_multiple_matches() {
        let db = HistoryDb::open_memory().unwrap();
        let sid1 = db
            .start_session(&make_session_info("DISC1", "Test"))
            .unwrap();
        db.finish_session(sid1, SessionStatus::Failed).unwrap();
        let sid2 = db
            .start_session(&make_session_info("DISC1", "Test"))
            .unwrap();
        db.finish_session(sid2, SessionStatus::Completed).unwrap();
        let results = db.find_session_by_label("DISC1").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_find_session_by_tmdb() {
        let db = HistoryDb::open_memory().unwrap();
        let mut info = make_session_info("DISC", "Breaking Bad");
        info.tmdb_id = Some(1396);
        info.tmdb_type = Some("tv".to_string());
        info.season = Some(1);
        let sid = db.start_session(&info).unwrap();
        db.finish_session(sid, SessionStatus::Completed).unwrap();
        let results = db.find_session_by_tmdb(1396, "tv", Some(1)).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_list_sessions_with_filter() {
        let db = HistoryDb::open_memory().unwrap();
        let s1 = db
            .start_session(&make_session_info("D1", "Show A"))
            .unwrap();
        db.finish_session(s1, SessionStatus::Completed).unwrap();
        let s2 = db
            .start_session(&make_session_info("D2", "Show B"))
            .unwrap();
        db.finish_session(s2, SessionStatus::Failed).unwrap();
        let filter = SessionFilter {
            status: Some(SessionStatus::Completed),
            ..Default::default()
        };
        let results = db.list_sessions(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Show A");
    }

    #[test]
    fn test_list_sessions_title_search() {
        let db = HistoryDb::open_memory().unwrap();
        db.start_session(&make_session_info("D1", "Breaking Bad"))
            .unwrap();
        db.start_session(&make_session_info("D2", "Better Call Saul"))
            .unwrap();
        let filter = SessionFilter {
            title: Some("breaking".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions(&filter).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_get_session_detail() {
        let db = HistoryDb::open_memory().unwrap();
        let sid = db
            .start_session(&make_session_info("DISC", "Test"))
            .unwrap();
        db.record_disc_playlists(
            sid,
            &[DiscPlaylistInfo {
                playlist: "00800".to_string(),
                duration_ms: Some(2700000),
                video_streams: Some(1),
                audio_streams: Some(2),
                subtitle_streams: Some(1),
                chapters: Some(12),
                is_filtered: false,
            }],
        )
        .unwrap();
        let fid = db
            .record_file(
                sid,
                &RippedFileInfo {
                    playlist: "00800".to_string(),
                    episodes: Some("[1]".to_string()),
                    output_path: "/tmp/test.mkv".to_string(),
                    file_size: Some(5_000_000_000),
                    duration_ms: Some(2700000),
                    streams: None,
                    chapters: Some(12),
                },
            )
            .unwrap();
        db.update_file_status(fid, FileStatus::Completed, None)
            .unwrap();
        db.finish_session(sid, SessionStatus::Completed).unwrap();
        let detail = db.get_session(sid).unwrap().unwrap();
        assert_eq!(detail.summary.title, "Test");
        assert_eq!(detail.playlists.len(), 1);
        assert_eq!(detail.files.len(), 1);
        assert_eq!(detail.files[0].status, FileStatus::Completed);
    }

    #[test]
    fn test_stats() {
        let db = HistoryDb::open_memory().unwrap();
        let s1 = db.start_session(&make_session_info("D1", "Show")).unwrap();
        let fid = db
            .record_file(
                s1,
                &RippedFileInfo {
                    playlist: "00800".to_string(),
                    episodes: None,
                    output_path: "/tmp/t.mkv".to_string(),
                    file_size: Some(1_000_000),
                    duration_ms: None,
                    streams: None,
                    chapters: None,
                },
            )
            .unwrap();
        db.update_file_status(fid, FileStatus::Completed, None)
            .unwrap();
        db.finish_session(s1, SessionStatus::Completed).unwrap();
        let stats = db.stats().unwrap();
        assert_eq!(stats.total_sessions, 1);
        assert_eq!(stats.completed_sessions, 1);
        assert_eq!(stats.total_files, 1);
        assert_eq!(stats.total_size, 1_000_000);
    }

    #[test]
    fn test_delete_session() {
        let db = HistoryDb::open_memory().unwrap();
        let sid = db
            .start_session(&make_session_info("DISC", "Test"))
            .unwrap();
        assert!(db.delete_session(sid).unwrap());
        assert!(db.get_session(sid).unwrap().is_none());
    }

    #[test]
    fn test_clear_all() {
        let db = HistoryDb::open_memory().unwrap();
        db.start_session(&make_session_info("D1", "A")).unwrap();
        db.start_session(&make_session_info("D2", "B")).unwrap();
        let deleted = db.clear_all().unwrap();
        assert_eq!(deleted, 2);
        let stats = db.stats().unwrap();
        assert_eq!(stats.total_sessions, 0);
    }

    #[test]
    fn test_prune_by_age() {
        let db = HistoryDb::open_memory().unwrap();
        db.conn.execute(
            "INSERT INTO sessions (volume_label, title, started_at, status) VALUES ('OLD', 'Old Show', '2025-01-01T00:00:00', 'completed')",
            [],
        ).unwrap();
        db.start_session(&make_session_info("NEW", "New Show"))
            .unwrap();
        let pruned = db.prune("2026-01-01T00:00:00", None).unwrap();
        assert_eq!(pruned, 1);
        let remaining = db.list_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].title, "New Show");
    }

    #[test]
    fn test_display_status_partial() {
        let db = HistoryDb::open_memory().unwrap();
        let sid = db.start_session(&make_session_info("D", "T")).unwrap();
        let f1 = db
            .record_file(
                sid,
                &RippedFileInfo {
                    playlist: "00800".to_string(),
                    episodes: Some("[1]".to_string()),
                    output_path: "/tmp/1.mkv".to_string(),
                    file_size: None,
                    duration_ms: None,
                    streams: None,
                    chapters: None,
                },
            )
            .unwrap();
        db.update_file_status(f1, FileStatus::Completed, None)
            .unwrap();
        let f2 = db
            .record_file(
                sid,
                &RippedFileInfo {
                    playlist: "00801".to_string(),
                    episodes: Some("[2]".to_string()),
                    output_path: "/tmp/2.mkv".to_string(),
                    file_size: None,
                    duration_ms: None,
                    streams: None,
                    chapters: None,
                },
            )
            .unwrap();
        db.update_file_status(f2, FileStatus::Failed, Some("error"))
            .unwrap();
        db.finish_session(sid, SessionStatus::Completed).unwrap();
        let sessions = db.list_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(sessions[0].display_status(), "partial");
        assert_eq!(sessions[0].files_completed, 1);
        assert_eq!(sessions[0].files_total, 2);
    }

    #[test]
    fn test_display_status_excludes_skipped() {
        let db = HistoryDb::open_memory().unwrap();
        let sid = db.start_session(&make_session_info("D", "T")).unwrap();
        let f1 = db
            .record_file(
                sid,
                &RippedFileInfo {
                    playlist: "00800".to_string(),
                    episodes: Some("[1]".to_string()),
                    output_path: "/tmp/1.mkv".to_string(),
                    file_size: None,
                    duration_ms: None,
                    streams: None,
                    chapters: None,
                },
            )
            .unwrap();
        db.update_file_status(f1, FileStatus::Completed, None)
            .unwrap();
        let f2 = db
            .record_file(
                sid,
                &RippedFileInfo {
                    playlist: "00801".to_string(),
                    episodes: Some("[2]".to_string()),
                    output_path: "/tmp/2.mkv".to_string(),
                    file_size: None,
                    duration_ms: None,
                    streams: None,
                    chapters: None,
                },
            )
            .unwrap();
        db.update_file_status(f2, FileStatus::Skipped, None)
            .unwrap();
        db.finish_session(sid, SessionStatus::Completed).unwrap();
        let sessions = db.list_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(sessions[0].files_completed, 1);
        assert_eq!(sessions[0].files_total, 1); // skipped excluded
        assert_eq!(sessions[0].display_status(), "completed");
    }
}
