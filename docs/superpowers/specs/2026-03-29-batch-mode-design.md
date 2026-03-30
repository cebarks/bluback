# Batch Mode Design

**Date:** 2026-03-29
**Version target:** v0.10
**Scope:** Continuous batch ripping — rip, eject, wait for next disc, restart wizard

## Overview

Batch mode enables continuous multi-disc ripping sessions. After a disc finishes ripping, the drive ejects, bluback waits for the next disc, and restarts the wizard automatically. The user reviews TMDb/episode/playlist settings per disc (semi-automatic), but doesn't need to restart the tool or press Enter to continue.

This is a standalone feature. Disc history / duplicate detection is deferred to v0.11.

## Configuration & CLI

### Config

New `batch` boolean in `config.toml` (root level, alongside `eject`, `overwrite`):

```toml
batch = false  # default
```

Add `"batch"` to `KNOWN_KEYS`. Update `to_toml_string()` to include `batch` in commented-defaults output. Update `validate_raw_toml()` if needed.

### CLI flags

```
--batch       Enable batch mode (rip → eject → wait → repeat)
--no-batch    Disable batch mode (overrides config)
```

`--batch` and `--no-batch` are mutually exclusive (clap argument group).

`--batch` conflicts with `--dry-run`, `--list-playlists`, `--check`, and `--settings` (clap `conflicts_with`). These modes are single-shot by nature and looping makes no sense.

### Environment variable

`BLUBACK_BATCH` — same override semantics as other `BLUBACK_*` env vars.

### Settings panel

New Toggle item: "Batch mode" — added to the settings panel, toggleable mid-session via `Ctrl+S`.

### Eject behavior

`--batch` and `--no-eject` are `conflicts_with` in clap — batch mode requires eject to swap discs, so requesting both is a user error. When batch mode is active (via config or `--batch`), eject is forced true. If a user has `eject = false` in config but enables batch, the batch override wins with no warning needed.

## TUI Behavior

### Done screen auto-restart

The Done screen disc detection involves two code paths:

1. **`dashboard.rs` — `check_all_done_session()`**: When all rip jobs complete, calls `session.start_disc_scan()` to begin background disc polling (2-second interval using `disc::get_volume_label()`), then overrides the screen to `Done` to show results.
2. **`session.rs` — `poll_background()`**: When `BackgroundResult::DiscFound` arrives, sets `disc_detected_label` which triggers the popup on the Done screen.
3. **`session.rs` — `handle_key()`**: On the Done screen with `disc_detected_label` set, waits for user to press Enter before calling `reset_for_rescan()` + `start_disc_scan()`.

**Batch mode change:** In step 2, when batch mode is enabled and the screen is `Done`, skip setting `disc_detected_label` and instead immediately call `reset_for_rescan()` + `start_disc_scan()`. No popup, no Enter required. The existing disc polling infrastructure handles everything else.

When batch mode is disabled, existing behavior is preserved (popup with "Press Enter to start").

Toggling batch mode on via settings while on the Done screen takes effect on the _next_ disc detection, not retroactively.

### Disc count indicator

Add `batch_disc_count: Option<u32>` to `DashboardView` and `DoneView` in `types.rs`. When `Some(n)`, the block title renders as `"bluback — Disc 3 | Batch"` instead of `"bluback"`. The counter is a session-level field that `reset_for_rescan()` does NOT clear — it persists across disc resets and increments each time a new disc rip begins.

### Eject on completion (new TUI functionality)

TUI currently has no auto-eject after ripping — eject only happens via `Ctrl+E`. Batch mode adds auto-eject to `check_all_done_session()` in `dashboard.rs`: after all jobs complete and before transitioning to the Done screen, call `disc::eject_disc()`. This only fires when batch mode is enabled. Eject failure is logged but does not prevent the Done screen transition or disc polling.

### Quit behavior

- During ripping: existing behavior (confirm abort, clean up partial files).
- On Done screen in batch mode: `q` or `Ctrl+C` exits immediately — the user explicitly wants to stop the loop. The coordinator's existing `CANCELLED` check and `shutdown_all()` handle TUI signal cleanup.

### Multi-drive interaction

Batch mode is a config/CLI flag propagated to each `DriveSession` via the coordinator's `spawn_session()`. Each drive batches independently — when one drive finishes and ejects, it waits for a new disc on that drive while other drives continue their own workflows.

## CLI Behavior

### Loop structure

The batch loop cannot simply wrap `cli::run()` because `run()` bails early when no device is found. Instead, implement a `cli::run_batch()` function:

1. Poll for disc using `disc::get_volume_label()` (reuse same function TUI uses, 2-second interval)
2. Disc detected → run existing scan/TMDb/wizard/rip flow (extracted from `cli::run()` internals)
3. Eject disc
4. Print per-disc summary
5. Print "Waiting for next disc..."
6. Check `CANCEL` AtomicBool — if set, break to aggregate summary
7. Go to step 1

`main.rs` dispatches to `cli::run_batch()` instead of `cli::run()` when batch is enabled.

### Episode auto-advance

A running episode counter persists across loop iterations:
- First iteration uses `--start-episode` if provided, otherwise 1.
- After each disc, advance by the total number of **assigned episodes** (not playlist count). Multi-episode playlists count all their episodes: a playlist assigned `E03-E04` contributes 2 to the counter.
- Specials do not advance the counter.
- Example: disc 1 rips 4 playlists assigned episodes 1, 2, 3-4, 5 → 5 episodes total → disc 2 auto-starts at episode 6.
- In non-`--yes` mode where the user manually overrides episode assignments, the auto-advance uses the **actual** assignments from the completed disc, not the initial auto-assignment.

### Headless operation

- `--batch --yes`: fully unattended. All prompts auto-accepted, episodes auto-assigned with auto-advance, playlists auto-selected per existing `--yes` behavior.
- `--batch` without `--yes`: semi-automatic. Prompts for TMDb/episode confirmation each disc.

### Summary output

- After each disc: existing per-disc summary (`"All done! Ripped X playlist(s) to DIR"`).
- On exit (Ctrl+C or loop end): aggregate line — `"Batch complete: X discs, Y files, Z failures"`.
- "Waiting for next disc..." printed once per wait cycle, not every poll.

### `--start-episode` interaction

Sets the initial episode for the first disc only. Subsequent discs auto-advance from there.

## Hooks

`post_rip` (per-file) and `post_session` (per-disc) hooks fire with their existing semantics. In batch mode, `post_session` fires after **each disc** completes, not once at the end of the batch. Hook template variables (`{total}`, `{succeeded}`, `{failed}`, etc.) reflect the current disc only. Batch-aggregate hook variables are a future enhancement (see #5 below).

## Logging

In batch mode, each disc iteration should emit a new session header via `logging::session_header()` to clearly delineate per-disc log sections. The log file is per-process, so all discs in a batch accumulate in one log file.

## Error Handling

### Rip failure

- Failed playlists: log error, clean up partial MKV (existing behavior), continue with remaining playlists on that disc.
- After disc completes with failures: still eject, still wait for next disc. Don't exit batch loop.
- Failed playlists tracked in per-disc summary.

### Scan failure

If a detected disc fails to scan (e.g., unreadable, AACS error before rip): treat as disc failure. Log error, eject, continue waiting for next disc.

### Same disc re-inserted

Without disc history, no duplicate detection. Re-inserting the same disc rips it again. This is documented as expected behavior — disc history (v0.11) will address it.

### Mixed content across discs

Each disc runs through the full wizard. Users can switch TV/movie mode, change TMDb title, adjust settings per disc. Batch mode does not assume all discs are the same content.

In CLI `--yes` mode, all discs use the same flags (`--title`, `--movie`, `--season`, etc.).

### Episode auto-advance with specials

Specials don't advance the episode counter. Only regular episodes count.

## Files Modified

| File | Changes |
|------|---------|
| `src/config.rs` | Add `batch` field, TOML parsing, `KNOWN_KEYS` entry, `to_toml_string()`, env var override |
| `src/main.rs` | Dispatch to `cli::run_batch()` when batch enabled, pass batch flag to TUI |
| `src/cli.rs` | New `run_batch()` with outer loop, disc polling via `disc::get_volume_label()`, episode auto-advance counter, aggregate summary |
| `src/session.rs` | In `poll_background()`: auto-restart on `DiscFound` when batch enabled (skip `disc_detected_label` popup). Add `batch_disc_count: u32` field preserved across `reset_for_rescan()` |
| `src/tui/dashboard.rs` | In `check_all_done_session()`: auto-eject when batch enabled (new TUI functionality). Pass `batch_disc_count` to view types |
| `src/types.rs` | Add `batch_disc_count: Option<u32>` to `DashboardView` and `DoneView`. Add Batch toggle to `SettingsState` |
| `src/tui/settings.rs` | Handle Batch toggle input |
| `src/disc.rs` | No changes needed — `get_volume_label()` and `eject_disc()` already exist and are reusable |
| `src/logging.rs` | Emit session header per-disc in batch mode |

No new files required.

## Testing

### Unit tests

- Config: `batch` field parsing, default value, env var override, commented-defaults output, `KNOWN_KEYS` inclusion
- CLI: episode auto-advance logic — regular episodes advance counter, specials skipped, multi-episode playlists count all episodes (e.g., `E03-E04` = 2), manual overrides respected
- CLI: aggregate summary formatting (`"Batch complete: X discs, Y files, Z failures"`)
- Clap: `--batch` conflicts with `--dry-run`, `--list-playlists`, `--check`, `--settings`, `--no-eject`

### Integration-level behavior (manual)

- TUI: enable batch → rip disc → auto-ejects → insert new disc → auto-restarts wizard (no popup)
- TUI: toggle batch off mid-session → reverts to popup behavior
- TUI: disc counter increments across discs, survives `reset_for_rescan()`
- CLI: `--batch --yes --title "Show" --season 1` → rips multiple discs unattended with episode auto-advance
- CLI: Ctrl+C between discs → clean exit with aggregate summary
- Failure: bad disc mid-batch → ejects, waits for next disc
- Multi-drive: each drive batches independently

## Future Enhancements

1. **Fully unattended TUI batch** — auto-reuse previous TMDb/season/episode settings without presenting wizard screens. Good for box set binge-ripping where all discs are the same show.
2. **Batch dashboard** — persistent stats sidebar showing disc count, cumulative file count/size, and failure log across all discs in the session.
3. **Disc history integration** — skip already-ripped discs automatically via `history.json` database (v0.11 roadmap item).
4. **Queue-based batch** — scan and queue multiple discs for review before committing to rip. More relevant for multi-drive setups.
5. **Batch-specific hooks** — `[post_batch]` config section with aggregate template variables (`{disc_count}`, `{total_files}`, `{total_size}`), distinct from `[post_session]` which fires per-disc.
6. **Desktop notifications** — notify-send / macOS notifications when batch needs attention or completes.
7. **Configurable poll interval** — let users adjust the 2-second disc detection interval.
