# Multi-Drive Support Design Spec

**Date:** 2026-03-27
**Status:** Approved

## Overview

Add support for concurrent Blu-ray ripping from multiple drives simultaneously. Each drive gets its own independent session running in a dedicated thread, presented as tabs in the TUI. Drives are auto-detected with hot-plug support. Sessions can optionally share context (TMDb info, episode numbering) for multi-disc TV workflows.

## Goals

1. **Concurrent ripping** — Multiple drives rip simultaneously, each with its own session
2. **Multi-disc workflow** — Link sessions so disc 2 continues episode numbering where disc 1 left off
3. **Drive selection** — Interactive drive chooser when multiple drives are detected
4. **Resilience** — One session crashing or hanging doesn't affect others
5. **Hot-plug** — Drives added/removed at runtime are handled gracefully

## Non-Goals

- CLI concurrent multi-drive support (TODO for future — CLI stays single-drive for now)
- Re-encoding or transcoding across drives
- Shared thread pool or work-stealing between sessions

## Architecture

Three-layer actor architecture with channel-based communication:

```
┌─────────────────────────────────────────────────┐
│  Main Thread (Event Loop)                       │
│  - Terminal events (crossterm)                  │
│  - Input routing to active session              │
│  - Render loop (ratatui): tab bar + active view │
│  - Drive monitor polling                        │
│  - Settings overlay (global)                    │
└────────┬──────────────┬──────────────┬──────────┘
         │ channels     │ channels     │ channels
┌────────▼────┐  ┌──────▼──────┐  ┌───▼──────────┐
│ Session sr0 │  │ Session sr1 │  │ Session sr2  │
│ Own state   │  │ Own state   │  │ Own state    │
│ machine,    │  │ machine,    │  │ machine,     │
│ disc I/O,   │  │ disc I/O,   │  │ disc I/O,    │
│ remux       │  │ remux       │  │ remux        │
└─────────────┘  └─────────────┘  └──────────────┘
```

### Main Thread

A thin event loop that never blocks on I/O:

1. Poll terminal events (crossterm, ~50ms timeout for ~20fps render)
2. Handle global hotkeys: `Ctrl+C` (quit), `Ctrl+S` (settings overlay), `Tab`/`Shift+Tab` (switch active tab), `Ctrl+L` (link picker), `Ctrl+N` (new session in manual mode)
3. Forward all other input to the active session's input channel
4. Drain `DriveEvent` from the drive monitor
5. Drain `SessionMessage` from all session threads (snapshots, progress, notifications)
6. Detect dead session threads via closed channels, mark as crashed
7. Render: tab bar + active session view + optional overlay

The main thread accesses session state only through cached `RenderSnapshot` values — no locks, no shared mutable state.

### Session Threads

Each drive gets a dedicated thread running a `DriveSession`:

```rust
struct DriveSession {
    id: SessionId,              // Unique ID for overlap tracking and linking
    device: PathBuf,
    config: Config,             // Clone of global config at session creation
    screen: Screen,
    disc: DiscState,
    tmdb: TmdbState,
    wizard: WizardState,
    rip: RipState,

    input_rx: mpsc::Receiver<SessionCommand>,
    output_tx: mpsc::Sender<SessionMessage>,
}
```

**Extracted from current `App`:** The per-drive fields (`screen`, `disc`, `tmdb`, `wizard`, `rip`, `device`, background receiver) move into `DriveSession`. Global fields (`overlay`, `config_path`, signal handling) stay on the main thread. Each session receives a `Config` clone at creation time; the main thread sends `ConfigChanged` when settings are saved.

**Session event loop:** Blocks on `input_rx.recv()` with a timeout (for periodic tasks like spinner animation, progress polling from sub-threads). Processes commands, advances the state machine, sends `SessionMessage` on state changes.

**Blocking I/O** (disc scan, TMDb fetch, remux) runs on sub-threads spawned by the session thread — same pattern as today's background threads, but scoped to the session.

**Crash isolation:** Session entry point wraps in `catch_unwind`. A panicking session sends `Notification::SessionCrashed(error)` if possible. The main thread also detects closed channels as a fallback. The tab remains visible with an error state.

### Communication Channels

**Main → Session (`SessionCommand`):**

```rust
enum SessionCommand {
    KeyEvent(KeyEvent),
    LinkTo { context: SharedContext },
    ConfigChanged(Config),
    Shutdown,
}
```

**Session → Main (`SessionMessage`):**

```rust
enum SessionMessage {
    Snapshot(RenderSnapshot),
    Progress(RipProgress),
    Notification(Notification),
}
```

- **`Snapshot`** — Full display-only state. Sent on screen transitions, wizard changes, TMDb results — any meaningful state change. Infrequent (human-speed interactions).
- **`Progress`** — Lightweight rip progress (percent, speed, ETA, current file). Frequent during remux, rate-limited to ~10/sec on the session side.
- **`Notification`** — One-shot events: rip complete, error, drive lost, episodes assigned (for overlap validation), new disc detected.

### Drive Monitor

Dedicated lightweight thread for drive detection and hot-plug:

```rust
struct DriveMonitor {
    known_drives: HashSet<PathBuf>,
    tx: mpsc::Sender<DriveEvent>,
}

enum DriveEvent {
    DriveAppeared(PathBuf),
    DriveDisappeared(PathBuf),
    DiscInserted(PathBuf, String),   // device, volume label
    DiscEjected(PathBuf),
}
```

**Polling:** Every 2 seconds (matching current behavior), calls `detect_optical_drives()` and `get_volume_label()` for each drive. Diffs against known state to emit events.

**Main thread handles events:**

| Event | Action |
|---|---|
| `DriveAppeared` | Auto mode: spawn new session in "waiting for disc" state. Manual mode: add to available drives list. |
| `DriveDisappeared` | Send `Shutdown` to session. If mid-rip, jobs marked failed with "drive disconnected". Tab kept visible with error. |
| `DiscInserted` | Forward to session thread, which begins scanning. |
| `DiscEjected` | Forward to session thread. Idle/done: no-op. Mid-rip: session handles error. |

## TUI Layout

### Tab Bar

Always rendered at the top of the screen. Each tab shows a compact `TabSummary`:

```
 sr0: Ripping 3/8 42%  │  sr1: Playlist Manager  │  sr2: Waiting for disc
```

- Active tab: highlighted/bold
- Error tabs: distinct color (red)
- Ripping tabs: show compact progress even when not active

```rust
struct TabSummary {
    device_name: String,
    state: TabState,  // Idle, Scanning, Wizard, Ripping, Done, Error
    rip_progress: Option<(usize, usize, u8)>,  // current, total, percent
    error: Option<String>,
}
```

### Render Snapshots

Session threads send display-only data for rendering:

```rust
struct RenderSnapshot {
    session_id: SessionId,
    device: PathBuf,
    screen: Screen,
    status_message: String,

    // For Ctrl+L link picker — available once TMDb lookup is complete
    linkable_context: Option<SharedContext>,

    // Screen-specific render data (only active screen populated)
    scanning: Option<ScanningView>,
    tmdb: Option<TmdbView>,
    season: Option<SeasonView>,
    playlist_mgr: Option<PlaylistView>,
    confirm: Option<ConfirmView>,
    dashboard: Option<DashboardView>,
    done: Option<DoneView>,
}
```

Only the active screen's view is populated. Existing render functions are refactored from `fn render_foo(app: &App, frame: &mut Frame)` to `fn render_foo(view: &FooView, frame: &mut Frame, area: Rect)`, where `area` is shrunk to account for the tab bar.

### Keybindings

| Key | Scope | Action |
|---|---|---|
| `Tab` / `Shift+Tab` | Global | Switch active tab |
| `Ctrl+L` | Global | Open link picker (copy context from another session) |
| `Ctrl+N` | Global | New session (manual mode only) |
| `Ctrl+S` | Global | Settings overlay |
| `Ctrl+C` | Global | Quit all sessions |
| `Ctrl+R` | Forwarded | Rescan (per-session) |
| `Ctrl+E` | Forwarded | Eject (per-session) |
| All others | Forwarded | Routed to active session |

## Linked Sessions

Sessions are independent by default. Opt-in linking copies context from one session to another for multi-disc TV workflows.

### SharedContext

```rust
struct SharedContext {
    show_name: String,
    tmdb_show: Option<TmdbShow>,
    season_num: u32,
    next_episode: u32,
    movie_mode: bool,
    episodes: Vec<Episode>,
}
```

### Link Flow

1. User navigates to new tab's TMDb Search screen
2. `Ctrl+L` opens a picker showing other sessions that have completed TMDb lookup
3. User selects source session
4. Main thread extracts `SharedContext` from source session's cached snapshot
5. Sends `SessionCommand::LinkTo { context }` to target session
6. Target session auto-fills TMDb info, season, advances `start_episode` past source's assignments
7. Target session skips to Playlist Manager with pre-populated numbering

### Overlap Validation

When a linked session confirms episode assignments, it sends `Notification::EpisodesAssigned(session_id, Vec<Episode>)`. The main thread checks all linked sessions for overlapping episode numbers. If overlap is detected, a warning is sent back to the offending session before ripping starts — the user must resolve it (re-assign or force proceed).

Linking is a one-time context copy. After linking, sessions are independent — no ongoing cross-thread synchronization.

## Configuration

### New Config Options

```toml
# Drive management mode
# "auto" — auto-create tabs for all detected drives (default)
# "manual" — single tab, Ctrl+N to add drives
multi_drive = "auto"
```

### Existing Options

All existing config options remain and apply globally (shared across sessions). Per-session config overrides are not supported in this iteration.

## CLI Mode

CLI mode (`--no-tui`) remains single-drive for this iteration. The `--device` flag or auto-detect selects one drive. No concurrent ripping in CLI mode.

**`--device` flag in TUI mode:** When `--device` is specified, only that drive gets a session — auto-detect is skipped and the drive monitor only watches that single device. This preserves the existing single-drive behavior for users who explicitly specify a device.

```
// TODO(multi-drive): Add concurrent CLI support with interleaved output
// and drive-prefixed progress lines (e.g., [sr0] Ripping playlist 1...)
```

## Error Handling

| Scenario | Behavior |
|---|---|
| Session thread panics | Tab shows error state with message. Other sessions unaffected. |
| Drive disconnected mid-rip | Jobs marked failed with "drive disconnected". Tab kept visible. |
| Drive disconnected while idle | Tab kept with "drive removed" status (removing tabs unexpectedly is jarring). User can close manually. |
| FFmpeg/remux error | Per-session error, same as current single-drive behavior. |
| Channel full/dropped | Main thread detects closed channel, marks session as crashed. |
| All sessions done | App stays open for new discs (matching current Done screen behavior). |

## Signal Handling

- **First `Ctrl+C`:** Broadcasts `Shutdown` to all session threads. Sets global `CANCELLED` flag. Sessions cancel active remux jobs and clean up partial files.
- **Second `Ctrl+C` within 2 seconds:** Force exit (code 130), same as current behavior.
- **Per-session cancel:** `q` on the active tab's ripping dashboard cancels only that session's rip. Other sessions continue.

## Refactoring Surface

The primary refactoring work is separating per-session state from global state in the current `App` struct:

**Moves to `DriveSession`:**
- `screen`, `disc`, `tmdb`, `wizard`, `rip`
- `pending_rx` (background task receiver)
- `disc_detected_label`
- `eject` (per-session decision)
- `status_message`, `spinner_frame`

**Stays on main thread / global:**
- `config`, `config_path`
- `overlay` (settings)
- `quit`
- `args` (split: device goes per-session, other flags are global)

**Render functions refactored:**
- `wizard.rs`: All `render_*` functions take `*View` structs instead of `&App`
- `dashboard.rs`: `render_dashboard`, `render_done` take view structs
- `tui/settings.rs`: Unchanged (settings overlay is global, rendered by main thread)
- Input handler functions: Move into session thread, operate on `DriveSession` instead of `&mut App`

## Testing Strategy

- **Unit tests:** Session message serialization, overlap validation logic, tab summary derivation, shared context extraction
- **Integration tests:** Multi-session state machine transitions (mock channels, no real drives)
- **Manual testing:** Requires multiple physical drives or mock devices

## Open Questions

None — all questions resolved during design.
