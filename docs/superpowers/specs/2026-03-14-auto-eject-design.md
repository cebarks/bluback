# Auto-Eject on Completion

## Summary

Add the ability to automatically eject the Blu-ray disc tray after all selected playlists have been successfully ripped. Controlled via `config.toml` and CLI flags.

## Motivation

When batch-ripping discs, users want a hands-off workflow. Auto-eject signals completion and allows immediate disc swap without returning to the terminal to run `eject` manually.

## Design

### Config & CLI

**Config** (`~/.config/bluback/config.toml`):
```toml
eject = true
```

- New `eject: Option<bool>` field on `Config` struct
- Defaults to `false` when absent

**CLI flags**:
```
--eject       Eject disc after successful rip
--no-eject    Don't eject disc after rip
```

- Mutually exclusive via `conflicts_with` (errors if both passed)
- Neither flag = defer to config value

**Priority chain** (highest to lowest): `--eject`/`--no-eject` CLI flag > `eject` in config.toml > `false`

### Eject Function

Location: `disc.rs`

```rust
pub fn eject_disc(device: &str) -> anyhow::Result<()>
```

- Shells out to `eject <device>` via `std::process::Command`
- Returns `Err` on non-zero exit
- No startup presence check for `eject` binary (unlike ffmpeg/ffprobe) -- missing `eject` shouldn't prevent the tool from running

### Integration Points

**CLI mode** (`cli.rs`):
- The rip loop currently doesn't track failures — it prints an error and `continue`s. Add a `had_failure` bool that gets set to `true` on ffmpeg error or probe failure. Skips due to existing output files count as success (user already has the file).
- After the "All done!" message, if eject is enabled and `!had_failure`, call `disc::eject_disc()`
- On eject failure: print a warning, still return `Ok(())`

**TUI mode** (`dashboard.rs`):
- Non-blocking eject via background thread + mpsc channel (same pattern as ffmpeg progress reading).
- Add `eject_rx: Option<mpsc::Receiver<anyhow::Result<()>>>` to `App` struct.
- In `tick()`, when all jobs complete and all succeeded, if eject is enabled: spawn `eject_disc()` in a `thread::spawn`, store the receiver on `app.eject_rx`, and set `app.status_message` to "Ejecting disc...". Stay on `Screen::Ripping` (the title bar already shows status).
- On subsequent `tick()` calls, poll `eject_rx`. When result arrives: clear `eject_rx`, set `app.status_message` to warning on error (or clear on success), then transition to `Screen::Done`.
- Thread the resolved eject setting through `App` struct via `app.eject: bool`.

**CLI mode** (`cli.rs`):
- Blocking eject is fine (no UI to freeze). Print "Ejecting disc..." before the call.

**Dry-run**: Eject is skipped entirely. CLI mode returns early before the rip loop. TUI mode goes directly to `Screen::Done` with empty `rip_jobs`, bypassing `tick()`'s completion path.

**User abort**: No eject. TUI abort kills the child and sets `quit = true`, bypassing `tick()`. CLI Ctrl+C terminates the process.

### Eject Resolution Helper

Add a method to `Config` or a standalone function to resolve the final eject decision:

```rust
pub fn should_eject(&self, cli_eject: Option<bool>) -> bool
```

- `cli_eject = Some(true)` -> true
- `cli_eject = Some(false)` -> false
- `cli_eject = None` -> `self.eject.unwrap_or(false)`

### Testing

- Config parsing: `eject = true`, `eject = false`, absent (defaults to `false`)
- Resolution priority: CLI flag overrides config value
- No hardware/integration tests for actual eject (thin shell-out, same pattern as ffmpeg)

## Files Modified

| File | Changes |
|------|---------|
| `src/config.rs` | Add `eject` field to `Config`, add `should_eject()` method, add tests |
| `src/main.rs` | Add `--eject`/`--no-eject` flags to `Args`, resolve and pass to runners |
| `src/disc.rs` | Add `eject_disc()` function |
| `src/cli.rs` | Call eject after successful completion |
| `src/tui/mod.rs` | Add eject setting to `App` struct |
| `src/tui/dashboard.rs` | Call eject on all-success completion |

## Non-Goals

- Eject on partial failure (only eject when all playlists succeed)
- Eject during dry-run
- Checking for `eject` binary at startup
