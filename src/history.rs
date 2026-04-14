//! SQLite-backed rip history database.

#![allow(dead_code)] // Types and methods used in future tasks

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;

const SCHEMA_VERSION: i64 = 1;
const STALE_SESSION_HOURS: i64 = 4;

// ============================================================================
// Enums
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

        self.conn
            .execute(
                "UPDATE sessions SET status = 'failed' WHERE status = 'in_progress' AND started_at < ?1",
                params![cutoff],
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

    pub fn record_disc_playlists(&self, session_id: i64, playlists: &[DiscPlaylistInfo]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()
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

        tx.commit().context("Failed to commit playlist transaction")?;
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
                       file_size = excluded.file_size,
                       duration_ms = excluded.duration_ms,
                       streams = excluded.streams,
                       chapters = excluded.chapters,
                       started_at = excluded.started_at"#,
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
        let id: i64 = self.conn.query_row(
            "SELECT id FROM ripped_files WHERE session_id = ?1 AND playlist = ?2",
            params![session_id, file.playlist],
            |row| row.get(0),
        )
        .context("Failed to retrieve file ID after upsert")?;

        Ok(id)
    }

    pub fn update_file_status(&self, file_id: i64, status: FileStatus, error: Option<&str>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE ripped_files SET status = ?1, finished_at = ?2, error = ?3 WHERE id = ?4",
                params![status.as_str(), now_iso8601(), error, file_id],
            )
            .context("Failed to update file status")?;

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
        let id = db.start_session(&make_session_info("DISC", "Test")).unwrap();
        db.finish_session(id, SessionStatus::Completed).unwrap();

        let status: String = db
            .conn
            .query_row("SELECT status FROM sessions WHERE id = ?1", [id], |r| r.get(0))
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
        let id = db.start_session(&make_session_info("DISC", "Test")).unwrap();
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
        let session_id = db.start_session(&make_session_info("DISC", "Test")).unwrap();
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
        let session_id = db.start_session(&make_session_info("DISC", "Test")).unwrap();
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
}
