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
}

// ============================================================================
// Helpers
// ============================================================================

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
}
