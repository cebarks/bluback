//! SQLite-backed rip history database.

use anyhow::Result;
use std::path::Path;

pub struct HistoryDb {
    conn: rusqlite::Connection,
}

impl HistoryDb {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        Ok(Self { conn })
    }

    pub fn open_memory() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        Ok(Self { conn })
    }
}
