# Scoped makemkvcon Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace global `kill_makemkvcon_children()` calls at probe/remux sites with operation-scoped RAII guards that only kill the makemkvcon processes they spawned, preventing cross-session interference in multi-drive mode and rescan race conditions.

**Architecture:** A `MakemkvconGuard` struct tracks makemkvcon PIDs spawned during its lifetime via before/after `/proc` diffing. A `SPAWN_LOCK` mutex serializes device opens so PID attribution is accurate. The guard's `Drop` impl kills only its tracked PIDs. The global `kill_makemkvcon_children()` remains exclusively for atexit/panic/force-exit paths where killing everything is correct.

**Tech Stack:** Rust, libc (Linux-specific `/proc` and `kill`/`waitpid` syscalls), `std::sync::Mutex`

---

## File Map

- **Modify:** `src/aacs.rs` — Add `MakemkvconGuard`, `SPAWN_LOCK`, `list_makemkvcon_children_set()`, `kill_makemkvcon_pids()`
- **Modify:** `src/media/probe.rs` — Use guard in `probe_playlist()`, remove global kill at line 122
- **Modify:** `src/media/remux.rs` — Use guard in `remux()`
- **Modify:** `src/tui/dashboard.rs` — Remove pre/post-remux global kills (guard handles it)
- **Modify:** `src/cli.rs` — Remove pre/post-remux global kills and verbose-probe kill (guard handles it)
- **Modify:** `src/session.rs` — Remove post-probe global kill (guard handles it)
- **Modify:** `src/tui/wizard.rs` — Remove post-probe global kill (guard handles it)

---

### Task 1: Add MakemkvconGuard and supporting functions to aacs.rs

**Files:**
- Modify: `src/aacs.rs`

- [ ] **Step 1: Write tests for list_makemkvcon_children_set and kill_makemkvcon_pids**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of `src/aacs.rs`:

```rust
#[test]
fn test_list_makemkvcon_children_set_returns_set() {
    // No makemkvcon processes on CI — should return empty set
    let pids = list_makemkvcon_children_set();
    assert!(pids.is_empty());
}

#[test]
fn test_kill_makemkvcon_pids_empty_is_noop() {
    // Should not panic or error on empty slice
    kill_makemkvcon_pids(&[]);
}

#[test]
fn test_kill_makemkvcon_pids_nonexistent_pid() {
    // Killing a non-existent PID should not panic (ESRCH is fine)
    kill_makemkvcon_pids(&[999_999_999]);
}

#[test]
fn test_makemkvcon_guard_drop_empty() {
    // Guard with no tracked PIDs should drop cleanly
    let guard = MakemkvconGuard::new();
    drop(guard);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_list_makemkvcon_children_set test_kill_makemkvcon_pids test_makemkvcon_guard 2>&1`
Expected: FAIL — functions and struct not yet defined.

- [ ] **Step 3: Implement MakemkvconGuard, list_makemkvcon_children_set, and kill_makemkvcon_pids**

Add these items to `src/aacs.rs`. The `SPAWN_LOCK` goes near the existing `SCAN_PGIDS` static. The functions and struct go before the existing `kill_makemkvcon_children()` function.

Add `use std::collections::HashSet;` to the imports at the top.

Add the `SPAWN_LOCK` static right after `SCAN_PGIDS`:

```rust
/// Serializes device opens so that MakemkvconGuard's before/after PID
/// diffing correctly attributes makemkvcon processes to the operation
/// that spawned them.
static SPAWN_LOCK: Mutex<()> = Mutex::new(());
```

Add these functions and the struct before `kill_makemkvcon_children()`:

```rust
/// List all makemkvcon processes that are children of this process.
/// Returns a set of PIDs for diffing (used by MakemkvconGuard).
#[cfg(target_os = "linux")]
pub fn list_makemkvcon_children_set() -> HashSet<i32> {
    let our_pid = std::process::id();
    let mut pids = HashSet::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Ok(pid) = name.to_string_lossy().parse::<i32>() else {
            continue;
        };
        let status_path = format!("/proc/{}/status", pid);
        let Ok(status) = std::fs::read_to_string(&status_path) else {
            continue;
        };
        let is_our_child = status.lines().any(|line| {
            line.strip_prefix("PPid:")
                .and_then(|rest| rest.trim().parse::<u32>().ok())
                == Some(our_pid)
        });
        if !is_our_child {
            continue;
        }
        let comm_path = format!("/proc/{}/comm", pid);
        let Ok(comm) = std::fs::read_to_string(&comm_path) else {
            continue;
        };
        if comm.trim() == "makemkvcon" {
            pids.insert(pid);
        }
    }
    pids
}

#[cfg(not(target_os = "linux"))]
pub fn list_makemkvcon_children_set() -> HashSet<i32> {
    HashSet::new()
}

/// Kill specific makemkvcon processes by PID.
pub fn kill_makemkvcon_pids(pids: &[i32]) {
    for &pid in pids {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
            libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG);
        }
    }
}

/// RAII guard that tracks makemkvcon processes spawned during device opens
/// and kills only those processes when dropped.
///
/// Each device-opening operation (probe_playlist, remux) should create a guard
/// and call `track_open()` around the FFmpeg `format::input*` call. The guard
/// snapshots makemkvcon children before and after the open, recording only the
/// new PIDs. On drop, it kills exactly those PIDs — no cross-session interference.
///
/// The `SPAWN_LOCK` is held during `track_open()` to serialize device opens,
/// ensuring accurate PID attribution across concurrent sessions.
pub struct MakemkvconGuard {
    pids: Vec<i32>,
}

impl MakemkvconGuard {
    pub fn new() -> Self {
        Self { pids: Vec::new() }
    }

    /// Execute a device-opening closure, tracking any makemkvcon processes
    /// it spawns. The SPAWN_LOCK is held during the closure to prevent
    /// concurrent opens from confusing the before/after PID diff.
    #[cfg(target_os = "linux")]
    pub fn track_open<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _lock = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let before = list_makemkvcon_children_set();
        let result = f();
        let after = list_makemkvcon_children_set();
        self.pids.extend(after.difference(&before));
        result
    }

    #[cfg(not(target_os = "linux"))]
    pub fn track_open<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        f()
    }
}

impl Drop for MakemkvconGuard {
    fn drop(&mut self) {
        if !self.pids.is_empty() {
            kill_makemkvcon_pids(&self.pids);
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -- test_list_makemkvcon_children_set test_kill_makemkvcon_pids test_makemkvcon_guard 2>&1`
Expected: PASS (4 tests).

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`
Expected: No warnings.

- [ ] **Step 6: Commit**

```bash
git add src/aacs.rs
git commit -m "feat: add MakemkvconGuard for scoped makemkvcon cleanup"
```

---

### Task 2: Use MakemkvconGuard in probe_playlist

**Files:**
- Modify: `src/media/probe.rs:518-654` (`probe_playlist` function)
- Modify: `src/media/probe.rs:122` (remove global kill after probe loop)

- [ ] **Step 1: Wrap open_bluray in probe_playlist with a guard**

In `src/media/probe.rs`, modify `probe_playlist()` to create a guard and wrap the `open_bluray` call. Replace:

```rust
pub fn probe_playlist(
    device: &str,
    playlist_num: &str,
) -> Result<(MediaInfo, StreamInfo), MediaError> {
    let ctx = open_bluray(device, Some(playlist_num))?;
```

With:

```rust
pub fn probe_playlist(
    device: &str,
    playlist_num: &str,
) -> Result<(MediaInfo, StreamInfo), MediaError> {
    let mut guard = crate::aacs::MakemkvconGuard::new();
    let ctx = guard.track_open(|| open_bluray(device, Some(playlist_num)))?;
```

The guard is dropped when `probe_playlist` returns (success or error), killing only the makemkvcon this call spawned.

- [ ] **Step 2: Remove the global kill_makemkvcon_children call after the probe loop**

In `src/media/probe.rs`, in `scan_playlists_with_progress()`, remove these lines (around line 118-122):

```rust
    // Each probe_playlist call opens the device via libbluray, which may spawn
    // makemkvcon when using the libmmbd backend. These child processes aren't
    // killed when the FFmpeg context is dropped, so clean them up now to
    // prevent interference with subsequent device opens (e.g., remux).
    crate::aacs::kill_makemkvcon_children();
```

Each `probe_playlist` call now has its own guard that cleans up automatically.

- [ ] **Step 3: Remove global kill_makemkvcon_children from session.rs probe thread**

In `src/session.rs`, in the probe thread spawned by `start_unfiltered_probe()`, remove the `crate::aacs::kill_makemkvcon_children();` call (around line 567). Each `probe_playlist` inside the loop now cleans up via its own guard.

- [ ] **Step 4: Remove global kill_makemkvcon_children from wizard.rs probe thread**

In `src/tui/wizard.rs`, in the on-demand probe thread (around line 1417), remove the `crate::aacs::kill_makemkvcon_children();` call. The single `probe_playlist` call uses its own guard.

- [ ] **Step 5: Remove global kill_makemkvcon_children from cli.rs verbose probe**

In `src/cli.rs`, in the `list_playlists` function, remove the `crate::aacs::kill_makemkvcon_children();` call after the verbose info collect (around line 174). Each fallback `probe_playlist` call in the `.or_else()` now uses its own guard.

- [ ] **Step 6: Run tests and clippy**

Run: `cargo test 2>&1 && cargo clippy -- -D warnings 2>&1`
Expected: All tests pass, no clippy warnings.

- [ ] **Step 7: Commit**

```bash
git add src/media/probe.rs src/session.rs src/tui/wizard.rs src/cli.rs
git commit -m "refactor: use MakemkvconGuard in probe_playlist, remove global kills"
```

---

### Task 3: Use MakemkvconGuard in remux

**Files:**
- Modify: `src/media/remux.rs:136-162` (`remux` function)
- Modify: `src/tui/dashboard.rs:778-789` (TUI rip thread)
- Modify: `src/cli.rs:1294-1339` (CLI rip loop)

- [ ] **Step 1: Wrap the device open in remux with a guard**

In `src/media/remux.rs`, modify `remux()` to create a guard and wrap the input context open. Replace:

```rust
    // Open input: bluray:{device} with playlist option
    let input_url = format!("bluray:{}", options.device);
    let mut opts = Dictionary::new();
    opts.set("playlist", &options.playlist);

    let mut ictx = format::input_with_dictionary(&input_url, opts).map_err(|e| {
        if let Some(aacs_err) = classify_aacs_error(&e) {
            return aacs_err;
        }
        MediaError::Ffmpeg(e)
    })?;
```

With:

```rust
    // Open input: bluray:{device} with playlist option.
    // The guard tracks makemkvcon spawned during the open and kills it on drop,
    // scoped to this remux only (no cross-session interference in multi-drive mode).
    let mut _mkv_guard = crate::aacs::MakemkvconGuard::new();
    let input_url = format!("bluray:{}", options.device);
    let mut opts = Dictionary::new();
    opts.set("playlist", &options.playlist);

    let mut ictx = _mkv_guard.track_open(|| {
        format::input_with_dictionary(&input_url, opts).map_err(|e| {
            if let Some(aacs_err) = classify_aacs_error(&e) {
                return aacs_err;
            }
            MediaError::Ffmpeg(e)
        })
    })?;
```

The guard lives for the entire `remux()` function and is dropped after `ictx` is dropped (Rust drops locals in reverse declaration order, but `_mkv_guard` is declared before `ictx`, so `ictx` drops first, then `_mkv_guard` kills makemkvcon).

- [ ] **Step 2: Remove pre/post-remux global kills from TUI dashboard**

In `src/tui/dashboard.rs`, in the rip thread (around lines 778-789), remove both `kill_makemkvcon_children` calls. Replace:

```rust
    std::thread::spawn(move || {
        // Kill stale makemkvcon from prior device opens (scan, probe) so they
        // don't interfere with the remux's libbluray/AACS initialization.
        crate::aacs::kill_makemkvcon_children();

        let tx_progress = tx.clone();
        let result = crate::media::remux::remux(options, |progress| {
            let _ = tx_progress.send(Ok(progress.clone()));
        });

        // Clean up makemkvcon spawned during this remux.
        crate::aacs::kill_makemkvcon_children();
```

With:

```rust
    std::thread::spawn(move || {
        let tx_progress = tx.clone();
        let result = crate::media::remux::remux(options, |progress| {
            let _ = tx_progress.send(Ok(progress.clone()));
        });
```

The guard inside `remux()` handles cleanup scoped to that operation.

- [ ] **Step 3: Remove pre/post-remux global kills from CLI rip loop**

In `src/cli.rs`, in `rip_selected()`, remove the pre-remux kill (around line 1294-1296):

```rust
        // Kill stale makemkvcon from prior device opens (scan, probe) so they
        // don't interfere with the remux's libbluray/AACS initialization.
        crate::aacs::kill_makemkvcon_children();
```

And remove the post-remux kill (around line 1338-1339):

```rust
        // Clean up makemkvcon spawned during this remux.
        crate::aacs::kill_makemkvcon_children();
```

The guard inside `remux()` handles cleanup.

- [ ] **Step 4: Run tests and clippy**

Run: `cargo test 2>&1 && cargo clippy -- -D warnings 2>&1`
Expected: All tests pass, no clippy warnings.

- [ ] **Step 5: Run cargo fmt**

Run: `rustup run stable cargo fmt -- --check 2>&1`
Expected: No formatting issues.

- [ ] **Step 6: Commit**

```bash
git add src/media/remux.rs src/tui/dashboard.rs src/cli.rs
git commit -m "refactor: use MakemkvconGuard in remux, remove global kills at rip sites"
```

---

### Task 4: Verify no remaining non-atexit global kill calls

**Files:**
- All `src/` files

- [ ] **Step 1: Search for remaining kill_makemkvcon_children calls**

Run: `rg 'kill_makemkvcon_children' src/ --line-number`

Expected remaining calls (global-kill-everything contexts only):
- `src/aacs.rs` — the function definition itself
- `src/main.rs` — atexit handler (`cleanup_makemkvcon`)
- `src/main.rs` — force-exit Ctrl+C handler
- `src/main.rs` — `run()` exit cleanup
- `src/tui/coordinator.rs` — session panic handler

Any other call site is a bug — it should use `MakemkvconGuard` or be removed.

- [ ] **Step 2: Run the full test suite one final time**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 3: Run clippy and fmt**

Run: `cargo clippy -- -D warnings 2>&1 && rustup run stable cargo fmt -- --check 2>&1`
Expected: Clean.
