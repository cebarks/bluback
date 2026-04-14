# Rip History — Design Spec

**Date:** 2026-04-13
**Version target:** v0.11
**Depends on:** Batch mode (v0.10, implemented)

## Overview

SQLite-backed rip history that tracks every disc scan and rip session. Serves three purposes: standalone rip log ("what have I ripped?"), duplicate detection ("don't re-rip this disc"), and episode/special continuation across sessions and batch runs ("pick up where I left off").

## Architecture

### Integration Model

`Option<HistoryDb>` lives on `App` (TUI) and is passed into CLI functions. Follows the same pattern as `Config` — loaded at startup, threaded through via existing state structs. `None` if the DB fails to open (read-only filesystem, corrupt DB, etc.) — log a warning, continue without history. All history calls guarded by `if let Some(db) = &self.history { ... }`.

Note: `HistoryDb` methods use `&self` (not `&mut self`) for all operations including writes, because `rusqlite::Connection` uses interior mutability for the statement cache and write path.

### Threading Model

`rusqlite::Connection` is `!Send` and `!Sync` — it cannot be shared across threads. This matters for multi-drive TUI mode where each `DriveSession` runs on its own thread.

**Approach:** Each thread opens its own `HistoryDb` connection to the same file. WAL mode makes concurrent writers safe (SQLite serializes writes internally). In practice:

- **Main thread (TUI):** Owns the `App`-level `HistoryDb` for overlay queries, contextual hints, and settings.
- **Session threads:** Each `DriveSession` opens its own `HistoryDb` on spawn (passed the DB path, not the connection). Used for recording sessions/files, episode continuation queries, and duplicate checks.
- **CLI mode:** Single-threaded, one `HistoryDb` instance — no threading concern.

The `HistoryDb::open()` path is passed to session threads via the existing `SessionConfig` / spawn mechanism, same as other config values. Each thread calls `HistoryDb::open()` independently.

### Finish Session Call Sites

- **TUI mode:** `finish_session()` is called when all rip jobs complete and the dashboard transitions to the Done screen (in `tick_rip()` when all jobs are terminal). If the user rescans (`Enter` on Done / `Ctrl+R`), a new session is created for the next disc. The old session's status is final at that point.
- **CLI mode:** `finish_session()` is called at the end of `cli::run()`, after all playlists are processed.
- **Signal handling:** Best-effort `finish_session(Cancelled)` on first Ctrl+C (see Signal Handling Integration section).

### Dependencies

- **rusqlite** — SQLite bindings (with `bundled` feature for portability). Blocking, no async, matches the rest of the codebase.
- **uuid** — for `batch_id` generation. Use `v4` (random) feature only.
- No other new crates required.

## Data Model

### SQLite Schema

```sql
PRAGMA journal_mode=WAL;  -- set on connection open for safe concurrent reads

CREATE TABLE schema_version (
    version INTEGER NOT NULL
);

CREATE TABLE sessions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    volume_label    TEXT NOT NULL,
    device          TEXT,
    tmdb_id         INTEGER,
    tmdb_type       TEXT CHECK(tmdb_type IN ('tv', 'movie')),
    title           TEXT NOT NULL,
    season          INTEGER,           -- NULL for movies
    disc_number     INTEGER,           -- parsed from volume label if available
    started_at      TEXT NOT NULL,     -- ISO 8601
    finished_at     TEXT,
    status          TEXT NOT NULL DEFAULT 'scanned'
                    CHECK(status IN ('scanned', 'in_progress', 'completed', 'failed', 'cancelled')),
    batch_id        TEXT,              -- UUID grouping discs in one batch run
    config_snapshot TEXT               -- JSON (see Config Snapshot section)
);

CREATE TABLE disc_playlists (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id       INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    playlist         TEXT NOT NULL,
    duration_ms      INTEGER,
    video_streams    INTEGER,
    audio_streams    INTEGER,
    subtitle_streams INTEGER,
    chapters         INTEGER,
    is_filtered      BOOLEAN DEFAULT 0,
    UNIQUE(session_id, playlist)
);

CREATE TABLE ripped_files (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    playlist    TEXT NOT NULL,
    episodes    TEXT,              -- JSON array: [3,4] or ["SP1","SP2"] or null
    output_path TEXT NOT NULL,
    file_size   INTEGER,
    duration_ms INTEGER,
    streams     TEXT,              -- JSON: {"video":1,"audio":2,"subtitle":3}
    chapters    INTEGER,
    status      TEXT NOT NULL DEFAULT 'in_progress'
                CHECK(status IN ('in_progress', 'completed', 'failed', 'skipped')),
    error       TEXT,
    verified    TEXT CHECK(verified IN ('passed', 'failed', 'skipped')),
    started_at  TEXT NOT NULL,
    finished_at TEXT,
    UNIQUE(session_id, playlist)   -- one entry per playlist per session
);

CREATE INDEX idx_sessions_tmdb ON sessions(tmdb_id, tmdb_type, season);
CREATE INDEX idx_sessions_label ON sessions(volume_label);
CREATE INDEX idx_sessions_status ON sessions(status);
CREATE INDEX idx_sessions_started ON sessions(started_at);
CREATE INDEX idx_ripped_files_session ON ripped_files(session_id);
```

### Session Lifecycle

Sessions transition through these statuses:

- `scanned` — disc scanned, `disc_playlists` recorded, no ripping yet. Created even if the user backs out.
- `in_progress` — ripping has started (at least one file being remuxed).
- `completed` — all selected playlists finished (some individual files may have failed).
- `failed` — session-level failure (e.g., AACS error before any rip started).
- `cancelled` — user cancelled (Ctrl+C, `q`).

**Display status:** There is no `partial` database status. For display purposes (CLI output, TUI overlay), a `completed` session where some `ripped_files` have `status = 'failed'` is rendered as `"partial (4/6)"`. This derivation happens at the `SessionSummary` level — `SessionSummary` includes `files_completed` and `files_total` counts, and a `display_status()` method returns `"partial"` when `files_completed < files_total` and the session status is `completed`. Counts exclude `skipped` files — `files_total` counts only files with status `completed` or `failed` (i.e., files that were attempted). Intentionally skipped files (overwrite protection, deselected playlists) don't make a session look like a failure.

### Stale Session Cleanup

On `HistoryDb::open()`, after running migrations:
1. Find all sessions with `status = 'in_progress'` where `started_at` is older than 4 hours.
2. Mark them as `failed` with `error = "session interrupted (stale cleanup)"`.
3. Mark their in-progress `ripped_files` as `failed`.

This handles crashes, force-exits (`std::process::exit(130)` from double Ctrl+C), and panics — none of which run Rust `Drop` impls. The 4-hour threshold is generous enough to never false-positive on a long rip (longest Blu-ray is ~3 hours) but catches genuinely abandoned sessions.

### Signal Handling Integration

- **First Ctrl+C:** The existing `CANCELLED` flag is set. When the rip orchestrator's event loop observes the flag (not in the signal handler thread itself), it calls `finish_session(id, Cancelled)` and marks in-progress files as `failed` before deleting partial MKV files (existing cleanup behavior). This is best-effort — if it fails, stale cleanup on next open handles it.
- **Second Ctrl+C / force exit:** `std::process::exit(130)` runs. Session stays as `in_progress`. Cleaned up on next `open()` via stale session cleanup (see above).

### Key Data Decisions

- `episodes` is a JSON array to handle single (`[3]`), multi-episode (`[3,4]`), and specials (`["SP1","SP2"]`).
- `batch_id` (UUID v4 via the `uuid` crate) groups all discs from a single `run_batch()` invocation. Queryable via `SessionFilter` and the `--batch-id` flag on `history list`.
- `ON DELETE CASCADE` on both `disc_playlists` and `ripped_files` — deleting a session cleans up everything.
- `disc_playlists` records ALL playlists found on disc, including filtered ones (`is_filtered`), enabling "10/12 playlists ripped" reporting.
- `UNIQUE(session_id, playlist)` on both `disc_playlists` and `ripped_files` prevents duplicate entries. If a playlist is retried within the same session, use `INSERT ... ON CONFLICT(session_id, playlist) DO UPDATE SET ...` (UPSERT) to preserve the row's primary key. This is important because `record_file()` returns a `file_id` that callers may hold for later `update_file_status()` calls — `INSERT OR REPLACE` would invalidate that ID.
- CHECK constraints on `status`, `tmdb_type`, and `verified` columns catch bugs at the DB layer.
- WAL journal mode set on connection open for safe concurrent reads (TUI rendering while writes happen).

### Config Snapshot

The `config_snapshot` column stores a JSON object with these specific fields:

```json
{
  "output_dir": "/path/to/output",
  "tv_format": "{show}/S{season}/{show} - S{season}E{episode}[ - {title}]",
  "movie_format": "{title}[ ({year})]",
  "special_format": "{show}/S{season}/{show} - S{season}SP{episode}[ - {title}]",
  "preset": "default",
  "stream_selection": "all",
  "audio_languages": ["eng"],
  "subtitle_languages": ["eng"],
  "prefer_surround": false,
  "aacs_backend": "auto",
  "reserve_index_space": 500,
  "verify": false,
  "verify_level": "quick"
}
```

Serialized from a dedicated `ConfigSnapshot` struct — a flat struct that cherry-picks fields from both `Config` and nested `StreamsConfig`. Not a direct subset of `Config`; it flattens nested sections for readability. Both `Serialize` and `Deserialize` derived. Note: `Config` currently only derives `Deserialize` — `ConfigSnapshot` is a separate struct, so no changes to `Config` are needed.

### Episode Query Strategy

Episode and special continuation queries require extracting values from the `episodes` JSON array in `ripped_files`. SQLite's `json_each()` function handles this:

```sql
-- last_episode: find max episode number for a show+season
SELECT MAX(CAST(je.value AS INTEGER))
FROM sessions s
JOIN ripped_files rf ON rf.session_id = s.id, json_each(rf.episodes) je
WHERE s.tmdb_id = ? AND s.season = ? AND rf.status = 'completed'
  AND typeof(je.value) = 'integer';

-- last_special: find max special number (stored as "SP1", "SP2", etc.)
SELECT MAX(CAST(SUBSTR(je.value, 3) AS INTEGER))
FROM sessions s
JOIN ripped_files rf ON rf.session_id = s.id, json_each(rf.episodes) je
WHERE s.tmdb_id = ? AND s.season = ? AND rf.status = 'completed'
  AND typeof(je.value) = 'text' AND je.value LIKE 'SP%';
```

Performance: `json_each()` scans the JSON array per row. For a personal rip log (hundreds of sessions, not millions), this is sub-millisecond. No denormalization needed.

## Module: `src/history.rs`

### HistoryDb API

```rust
pub struct HistoryDb {
    conn: rusqlite::Connection,
}

impl HistoryDb {
    // Lifecycle
    pub fn open(path: &Path) -> Result<Self>    // open/create, migrations, stale cleanup, WAL mode
    pub fn open_memory() -> Result<Self>         // for tests

    // Recording
    pub fn start_session(&self, info: &SessionInfo) -> Result<i64>
    pub fn finish_session(&self, id: i64, status: SessionStatus) -> Result<()>
    pub fn record_disc_playlists(&self, session_id: i64, playlists: &[DiscPlaylistInfo]) -> Result<()>
    pub fn record_file(&self, session_id: i64, file: &RippedFileInfo) -> Result<i64>
    pub fn update_file_status(&self, file_id: i64, status: FileStatus, error: Option<&str>) -> Result<()>

    // Episode continuation
    pub fn last_episode(&self, tmdb_id: i64, season: i32) -> Result<Option<i32>>
    pub fn last_episode_by_label(&self, label_pattern: &str, season: i32) -> Result<Option<i32>>
    pub fn last_special(&self, tmdb_id: i64, season: i32) -> Result<Option<i32>>
    pub fn last_special_by_label(&self, label_pattern: &str, season: i32) -> Result<Option<i32>>

    // Duplicate detection
    pub fn find_session_by_label(&self, label: &str) -> Result<Vec<SessionSummary>>
    pub fn find_session_by_tmdb(&self, tmdb_id: i64, tmdb_type: &str, season: Option<i32>) -> Result<Vec<SessionSummary>>

    // Querying
    pub fn list_sessions(&self, filter: &SessionFilter) -> Result<Vec<SessionSummary>>
    pub fn get_session(&self, id: i64) -> Result<Option<SessionDetail>>
    pub fn stats(&self) -> Result<HistoryStats>

    // Management
    pub fn delete_session(&self, id: i64) -> Result<bool>
    pub fn clear_all(&self) -> Result<u64>
    pub fn prune(&self, older_than: Duration, statuses: Option<&[SessionStatus]>) -> Result<u64>

    // Export
    pub fn export_json(&self, writer: &mut dyn Write) -> Result<()>
}
```

### Supporting Types

- `SessionInfo` — input for `start_session` (label, device, tmdb info, title, season, disc_number).
- `SessionSummary` — lightweight: id, title, label, season, disc_number, date, file count (completed/total), total size, status. Has `display_status()` method that returns `"partial"` when `files_completed < files_total` and session status is `completed`.
- `SessionDetail` — full session with `Vec<RippedFileDetail>` and `Vec<DiscPlaylistInfo>`.
- `SessionFilter` — optional: date range, status, title search, batch_id, limit.
- `HistoryStats` — aggregates: total sessions, files, size, by-status breakdown.
- `SessionStatus` — `Scanned`, `InProgress`, `Completed`, `Failed`, `Cancelled`.
- `FileStatus` — `InProgress`, `Completed`, `Failed`, `Skipped`.
- `ConfigSnapshot` — subset of `Config` for serialization into `config_snapshot` column.

### DB Location

Default: `$XDG_DATA_HOME/bluback/history.db` (falls back to `~/.local/share/bluback/history.db` when `$XDG_DATA_HOME` is unset). Overridable via `history.path` config or `BLUBACK_HISTORY_PATH` env var.

### Migration Strategy

`schema_version` table tracks current version. On `open()`, compare against an embedded migrations array and run any pending ones in a transaction. No external tooling.

## Episode Continuation

### Resolution Order

1. **Explicit CLI flag** — `--start-episode N` always wins, no history lookup.
2. **TMDb ID match** — query `last_episode(tmdb_id, season)` for the highest successfully-ripped episode.
3. **Volume label heuristic** — parse label for a show pattern, query `last_episode_by_label(pattern, season)`. Uses `disc::parse_volume_label()` to extract the show portion, then SQL `LIKE` with `%` suffix, filtered by `sessions.season`.
4. **Fallback** — no history match, use existing logic (disc number parse or default to episode 1).

Specials follow the same resolution order using `last_special()` / `last_special_by_label()`, independent from regular episode numbering.

Episode continuation is a no-op in movie mode (movies have no episode numbers).

### Label Pattern Matching

The `last_episode_by_label` function normalizes labels using the existing `disc::parse_volume_label()` logic:

1. Parse label to extract show name, season, disc number.
2. Reconstruct a pattern from just the show name portion (e.g., `BREAKING_BAD_S1_D3` -> show=`BREAKING_BAD`, pattern=`BREAKING_BAD%`).
3. Query with `sessions.volume_label LIKE ?` AND `sessions.season = ?`.

The `season` column filter prevents cross-season bleed even when the label pattern is broad. Labels that don't parse (no recognizable pattern) skip label-based lookup entirely.

### How Suggestions Surface

| Context | Behavior |
|---|---|
| TUI Season screen | Starting episode pre-filled with `last_episode + 1`. Hint: `"Continuing from E08 (ripped 2026-04-10)"`. Freely editable. |
| CLI interactive | Episode prompt shows default: `"Starting episode [9]: "` |
| Headless / `--yes` | Auto-applies silently. Logged to stderr. |
| Batch mode | Auto-applies between discs. Preferred over the in-memory `next_start_episode` counter in `run_batch()` (survives crashes, counts only successful rips). Falls back to the in-memory counter when history is unavailable (`--no-history`, `--ignore-history`, or DB open failure). |

### Edge Cases

- Multiple seasons: matched by season number, no cross-season bleed.
- Specials don't affect regular episode numbering.
- `--ignore-history`: disables both duplicate detection AND episode continuation. The session is still recorded to history. Without this, a re-rip would get wrong episode suggestions (continuation would suggest starting *after* the episodes being re-ripped).
- `--no-history`: disables all history DB access — no reads, no writes. Episode continuation falls through to the existing fallback logic (disc number parse or default to episode 1).
- Movies: episode continuation is skipped entirely.

## Duplicate Detection

### When It Triggers

After disc scan completes (volume label known), before the TMDb search screen. The check runs in one phase using data available at scan time:

1. **Volume label match** — exact match against `sessions.volume_label` in the DB.
2. **TMDb ID match** — queries `sessions.tmdb_id` from *previous* sessions stored in the DB (not the current session's TMDb lookup, which hasn't happened yet). This catches re-rips where the disc has a different label but the same show/season was ripped before.

Both checks run against historical data already in the database. The current session's TMDb lookup is not needed for duplicate detection.

### Behavior by Mode

| Mode | Match type | Action |
|---|---|---|
| Batch | Exact label, previous `completed` | Auto-skip, log, eject, next disc |
| Batch | Label match, previous `failed`/`cancelled` | Treat as new (retry) |
| TUI interactive | Any match | Warning banner with rip details + playlist coverage, Y/N |
| CLI interactive | Any match | Prompt with details, `[y/N]` |
| Headless (`--yes`) | Any match | Skip unless `--ignore-history` |

### Duplicate Reporting

Cross-references `disc_playlists` against `ripped_files` for the matched session:

- All ripped: `"Fully ripped on 2026-04-10 (12/12 playlists, 45.2 GB)"`
- Partial: `"Partially ripped on 2026-04-10 (10/12 playlists). Missing: 00801, 00803"`
- Scan only: `"Previously scanned on 2026-04-10 but not ripped"`

### Movie Duplicate Detection

Movies are detected the same way (label match or TMDb ID match). A movie session is considered "fully ripped" when all selected playlists completed. Since movies don't have episodes, there is no "partial coverage" concept beyond playlist counts — a re-rip with different playlist selections (e.g., main feature vs. director's cut) is flagged as a duplicate of the disc, and the user can choose to proceed. The duplicate warning shows which playlists were ripped previously.

**TMDb ID for movies:** The current `TmdbMovie` struct does not have an `id` field — only `title` and `release_date`. An `id: u64` field must be added to `TmdbMovie` (populated from the TMDb API response, which already includes it) to support `find_session_by_tmdb` for movies. If TMDb was skipped, `tmdb_id` is NULL and duplicate detection falls back to volume label matching only.

**TMDb ID type note:** `TmdbShow.id` is `u64` in the current code, and `TmdbMovie.id` should also be `u64` to match. The `sessions.tmdb_id` SQLite column is `INTEGER` (signed 64-bit) and the `HistoryDb` API uses `i64`. TMDb IDs are positive integers that fit in both types. Cast `u64 as i64` at the boundary when writing to/reading from the DB.

### `--ignore-history` Flag

Overrides all duplicate detection. Named `--ignore-history` to avoid confusion with `--overwrite` (file-level conflicts) and `--force` (confirmation skip in `history clear`). These are independent — you might `--ignore-history` a re-rip but let it skip files that already exist on disk (`--overwrite` off), or vice versa.

## CLI Subcommand Interface

Hybrid approach: existing flat flags remain the default (implicit) subcommand. `bluback history` is a new explicit subcommand.

### Clap Structure

The existing `Args` struct remains unchanged. Before clap parsing, `main()` checks `std::env::args()` for a leading `"history"` token (first non-program argument). If found, parse with a separate `HistoryArgs` clap struct. Otherwise, parse with the existing `Args` struct as before.

```rust
fn main() {
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.get(1).map(|s| s.as_str()) == Some("history") {
        // Parse remaining args with HistoryArgs
        let history_args = HistoryArgs::parse_from(&raw_args[1..]);
        return run_history(history_args);
    }
    // Existing path — unchanged
    let args = Args::parse();
    // ...
}
```

This approach avoids clap's `flatten` + `subcommand` interaction issues entirely. The existing `Args` struct has `--title` which would collide with `history list --title` under a flatten-based approach. Two separate parsers with an argv pre-check is simpler and guaranteed to work. `HistoryArgs` is its own `#[derive(Parser)]` with nested subcommands (`list`, `show`, `stats`, `delete`, `clear`, `export`).

**`--yes` on `history clear`:** Unlike the root-level `--yes` which auto-enables when stdin is not a TTY, `history clear --yes` must be explicitly passed — it does NOT auto-enable on non-TTY. This prevents accidental data deletion in scripted/cron contexts.

### Commands

```
bluback history                            # alias for 'list'
bluback history list [OPTIONS]
    --limit <N>                            # default 20
    --status <STATUS>                      # completed, failed, cancelled, scanned
    --title <SEARCH>                       # fuzzy match
    --since <DURATION>                     # "2026-04-01", "7d", "1month"
    --season <N>
    --batch-id <UUID>                      # filter by batch run
    --json                                 # machine-readable

bluback history show <ID>                  # full session detail + files

bluback history stats                      # aggregate summary

bluback history delete <ID> [<ID>...]      # delete sessions (with confirmation)

bluback history clear                      # delete all (with confirmation)
    --older-than <DURATION>                # prune by age
    --status <STATUS>                      # prune by status
    --yes                                  # skip confirmation (consistent with existing --yes pattern)

bluback history export                     # JSON dump to stdout (SessionDetail array format)
```

Note: `history clear` uses `--yes` to skip confirmation, consistent with the existing `--yes` flag semantics elsewhere in bluback. This avoids the `--force` naming collision.

### Duration Parsing

`--since`, `--older-than`, and the `retention` config option use the same duration parser, which returns an enum: `ParsedDuration::Relative(days)` or `ParsedDuration::Absolute(NaiveDate)`. Accepted formats: `"30d"`, `"90d"`, `"6months"` / `"6month"`, `"1year"` / `"1years"` (relative), and `"2026-04-01"` (absolute date). Both singular and plural forms accepted for relative durations. Callers that don't support absolute dates (e.g., `--older-than`, `retention`) reject `Absolute` variants with a clear error message.

### Output Format

**`bluback history list`:**
```
 ID  Date        Title                    Season  Disc  Status     Files  Size
  1  2026-04-10  Breaking Bad             S01     D1    completed  6/6    28.4 GB
  2  2026-04-10  Breaking Bad             S01     D2    completed  7/7    31.1 GB
  3  2026-04-11  The Matrix               —       —     completed  1/1    35.2 GB
  4  2026-04-12  Severance                S02     D2    partial    4/6    18.9 GB
  5  2026-04-12  Severance                S02     D3    scanned    —      —
```

**`bluback history show 4`:**
```
Session #4 — Severance S02 D2
  Disc:     SEVERANCE_S2_D2
  Device:   /dev/sr0
  Date:     2026-04-12 14:32 -> 15:01
  TMDb:     tv/93740
  Status:   partial (4/6 playlists ripped)

  Files:
    + S02E05_Kill_the_Lies.mkv          4.8 GB  48:12  verified
    + S02E06_Attila.mkv                 4.7 GB  47:55  verified
    x S02E07_Chroma.mkv                 —        —     error: AACS key not found
    + S02E08_Sweet_Vibes.mkv            4.6 GB  46:30  verified
    . 00803.mpls                        —        —     skipped
    . 00805.mpls                        —        —     skipped (filtered)
```

**`bluback history stats`:**
```
History Summary
  Sessions:  42 (38 completed, 2 partial, 1 failed, 1 scanned)
  Files:     287 ripped, 3 failed, 12 skipped
  Total:     1.24 TB across 42 discs
  First:     2026-04-10
  Last:      2026-04-13
  Batches:   6 batch runs (avg 4.2 discs/batch)
```

**`bluback history export`:**

Outputs a JSON array of `SessionDetail` objects, each containing nested `files` and `playlists` arrays. Matches the `SessionDetail` struct structure. Suitable for backup, migration, or external tooling.

```json
[
  {
    "id": 1,
    "volume_label": "BREAKING_BAD_S1_D1",
    "title": "Breaking Bad",
    "season": 1,
    "disc_number": 1,
    "status": "completed",
    "started_at": "2026-04-10T14:32:00",
    "finished_at": "2026-04-10T15:01:00",
    "playlists": [ ... ],
    "files": [ ... ]
  }
]
```

## TUI Integration

### History Overlay (`Ctrl+H`)

`Overlay::History` variant in `App.overlay`. Same rendering pattern as settings overlay.

- Scrollable session table: Date, Title, Season, Disc, Status, Files, Size
- Filter input row at top — fuzzy search by title, `Tab` to cycle status filter
- `Enter` on a row -> detail view (files, verification, errors)
- `Esc` in detail -> back to list, `Esc` in list -> close overlay
- Management: `d` delete session (with confirmation), `p` prune (prompts for age), `D` clear all (with confirmation)
- Available from any screen, blocked during active text input

Note: `Ctrl+H` sends the same byte as Backspace (0x08) on some terminals. Since the overlay is blocked during active text input, this shouldn't cause issues — Backspace goes to the text input, `Ctrl+H` is only active when no text input has focus. Test on common terminals (alacritty, kitty, gnome-terminal) during implementation. CLAUDE.md's TUI Keybindings section must be updated when this is implemented.

### Contextual Hints

Passive, read-only hints on existing screens:

| Screen | Hint |
|---|---|
| Scanning | Duplicate banner after label read. Yellow bar: `"D2 of Breaking Bad S01 ripped on 2026-04-10 (6/6 playlists)"` or partial count |
| Season | Starting episode pre-filled from history. Hint line: `"Continuing from E08 (ripped 2026-04-10)"` |
| Playlist Manager | `+` marker in left gutter for playlists matching a completed rip. Informational only. |
| Confirm | Warning line if disc previously ripped: `"! This disc was previously ripped (2026-04-10)"` |
| Done | `"Session saved to history"` status line |

### TUI State

```rust
pub struct HistoryOverlayState {
    pub sessions: Vec<SessionSummary>,
    pub selected: usize,
    pub filter_text: String,
    pub status_filter: Option<SessionStatus>,
    pub detail_view: Option<SessionDetail>,
    pub confirm_action: Option<ConfirmAction>,
}
```

## Configuration

### Config Section

```toml
[history]
enabled = true
# path = "/custom/path/to/history.db"
# retention = "1year"
# retention_statuses = ["scanned", "failed"]
```

- `enabled` (default: `true`) — toggle history tracking entirely.
- `path` — DB location override. Default: `$XDG_DATA_HOME/bluback/history.db`.
- `retention` — auto-prune on startup. Accepts: `"30d"`, `"90d"`, `"6months"`, `"1year"`, or omitted for no auto-prune.
- `retention_statuses` — only prune these statuses. If omitted, all statuses pruned. Lets you keep completed rips forever while cleaning scan-only/failed entries. Accepts any valid status name.

All `[history]` sub-keys must be added to `KNOWN_KEYS` in `config.rs` for validation.

### CLI Flags

```
--no-history        # disable ALL history DB access for this run (no recording, no duplicate
                    # checks, no episode continuation queries — as if history doesn't exist)
--ignore-history    # override duplicate detection AND episode continuation for this run
                    # (still records the session to history, but doesn't read from it)
```

**Semantic distinction:**
- `--no-history` = pretend the DB doesn't exist. No reads, no writes. Use when you want a clean session uninfluenced by history and don't want this session recorded.
- `--ignore-history` = don't let history influence this session's behavior (no duplicate warnings, no episode pre-fill), but still record the session for future reference. Use when re-ripping a disc intentionally — you want the re-rip logged, but you don't want stale suggestions (e.g., episode continuation would suggest starting *after* the episodes you're about to re-rip, which is wrong).

### Environment Variables

- `BLUBACK_HISTORY` — `true`/`false` (maps to `enabled` config)
- `BLUBACK_HISTORY_PATH` — DB path override
- `BLUBACK_HISTORY_RETENTION` — retention period override
- `BLUBACK_IGNORE_HISTORY` — `true`/`false` (override duplicate detection)

All new env vars must be added to the settings panel env var import logic in `types.rs`.

### Settings Panel

New "History" separator with:
- `Enabled` — toggle
- `Retention` — text input (free-form duration string)
- `Retention Statuses` — text input (comma-separated status names). The settings panel accepts free-form input rather than a fixed choice cycle, since the valid combinations are numerous. Validated on save.

### Interaction with Other Features

- **`--check` mode:** Verify history DB can be opened and report version/session count. Non-destructive.
- **`--settings` mode:** History settings editable in settings panel. No DB operations beyond open/close.
- **Hooks:** New hook template variables: `{session_id}`, `{disc_number}`, `{history_status}`.
- **Verification:** `verified` column in `ripped_files` populated from verification results (existing `verify.rs` module).
- **Overwrite mode:** Independent from `--ignore-history`. `--overwrite` handles file-level conflicts. `--ignore-history` handles session-level duplicate detection. Both can be used together or independently.
- **Auto-detect:** No interaction — auto-detect runs after history checks.
- **Stream selection:** Captured in `config_snapshot` for reference, not used by history logic.

## Testing

### Unit Tests (`src/history.rs`)

- Schema creation and migration (memory DB)
- Migration upgrades (v1 -> v2, verify new columns)
- WAL mode activation
- Stale session cleanup on open
- Session lifecycle: start -> record playlists -> record files -> finish
- `display_status()` derivation: completed + all files ok = "completed", completed + some failed = "partial"
- `last_episode` and `last_special`: single disc, multi-disc, mixed (including json_each query correctness)
- `last_episode_by_label` / `last_special_by_label`: exact, normalized, no match
- `find_session_by_label`: completed vs failed vs scanned, multiple matches
- Duplicate detection: partial rips, re-rips, cancelled sessions
- `disc_playlists` vs `ripped_files` cross-reference (N/M reporting)
- UNIQUE constraint enforcement (INSERT OR REPLACE behavior)
- CHECK constraint enforcement (invalid status values rejected)
- Retention: `prune()` with cutoffs, status filters, cascade
- `delete_session`, `clear_all` with cascade
- `list_sessions` with filters
- `stats()` aggregates
- `export_json` format
- Empty DB edge cases
- Concurrent open (two instances, same file)
- `--no-history` flag: no DB access at all (no recording, no duplicate checks, no episode continuation)
- `--ignore-history` flag: no reading (no duplicate checks, no episode continuation) but still records session
- Movie mode: episode continuation is no-op, duplicate detection works by label/TMDb
- UPSERT behavior: verify primary key preserved on retry (not destroyed like INSERT OR REPLACE)

### Unit Tests (episode continuation)

- Resolution priority: explicit flag > TMDb > label heuristic > fallback
- Specials counter independent from regular episodes
- Batch auto-advance using history query vs in-memory counter
- Label pattern normalization via `parse_volume_label()`

### Integration Tests (`tests/`)

- `tests/history_integration.rs` — full workflow: create DB -> session -> playlists -> files -> query continuation -> duplicate check -> prune
- CLI subcommand smoke tests: `bluback history list`, `bluback history stats` on empty and seeded DBs
- Clap regression: verify `bluback --device /dev/sr0`, `bluback history list`, and `bluback --title "some title"` all parse correctly
- Clap edge case: `bluback history --title foo` parses `history` as subcommand, not as `--title` value

### No Mocking

All tests use `HistoryDb::open_memory()` with real SQLite.
