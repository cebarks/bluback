# Post-Rip Hooks Design Spec

## Overview

Add user-configurable shell commands that execute after individual playlist rips and after an entire session completes. Hooks enable automation workflows like media library refreshes, file moves, notifications, and custom post-processing without modifying bluback itself.

## Config

Two new TOML table sections:

```toml
[post_rip]
command = "notify-send 'Done' '{filename}'"
on_failure = false    # default: only fire on success
blocking = true       # default: wait for hook to finish before next rip
log_output = true     # default: capture stdout/stderr to log file

[post_session]
command = "echo 'Finished {succeeded}/{total} files from {label}'"
on_failure = false
blocking = true
log_output = true
```

Both tables are entirely optional. No hook runs if `command` is not set.

**New struct:**

```rust
pub struct HookConfig {
    pub command: Option<String>,
    pub on_failure: Option<bool>,
    pub blocking: Option<bool>,
    pub log_output: Option<bool>,
}
```

Added as `pub post_rip: Option<HookConfig>` and `pub post_session: Option<HookConfig>` on the `Config` struct. Same struct, different table names.

**Defaults:**
- `on_failure`: `false` (hooks only fire on success)
- `blocking`: `true` (wait for hook before proceeding)
- `log_output`: `true` (capture hook output to log)

**KNOWN_KEYS additions:**
- `post_rip`, `post_rip.command`, `post_rip.on_failure`, `post_rip.blocking`, `post_rip.log_output`
- `post_session`, `post_session.command`, `post_session.on_failure`, `post_session.blocking`, `post_session.log_output`

**No env var overrides** for hook config — hooks are an advanced feature and table fields don't map cleanly to flat env vars.

## CLI Flag

`--no-hooks` — disables all hooks for this run. Useful for scripted/headless contexts where hooks might interfere. Logged at debug level when active.

No per-invocation CLI override for the command itself.

## Template Variables

Template substitution replaces `{var}` placeholders in the command string before passing to `sh -c`. Unknown variables are left as-is (forward compatibility).

**Per-file hook (`post_rip`):**

| Variable | Description | Example |
|----------|-------------|---------|
| `{file}` | Full output path | `/output/Show S01E03 Title.mkv` |
| `{filename}` | Filename only | `Show S01E03 Title.mkv` |
| `{dir}` | Output directory | `/output` |
| `{size}` | File size in bytes | `4831838208` |
| `{chapters}` | Chapters embedded | `12` |
| `{title}` | Show name or movie title | `Breaking Bad` |
| `{season}` | Season number (empty for movies) | `1` |
| `{episode}` | Episode number (empty for movies) | `3` |
| `{episode_name}` | Episode name from TMDb | `...And the Bag's in the River` |
| `{playlist}` | Playlist number | `00800` |
| `{label}` | Disc volume label | `BREAKING_BAD_S1_D1` |
| `{mode}` | `tv` or `movie` | `tv` |
| `{device}` | Device path | `/dev/sr0` |
| `{status}` | `success` or `failed` | `success` |
| `{error}` | Error message (empty on success) | |

**Per-session hook (`post_session`):**

All per-file variables except `{file}`, `{filename}`, `{size}`, `{chapters}`, `{episode}`, `{episode_name}`, `{playlist}`, `{status}`, `{error}`, plus:

| Variable | Description | Example |
|----------|-------------|---------|
| `{total}` | Total files attempted | `4` |
| `{succeeded}` | Files ripped successfully | `3` |
| `{failed}` | Number of failures | `1` |
| `{skipped}` | Number skipped (overwrite off) | `0` |

Session-level variables: `{title}`, `{season}`, `{label}`, `{device}`, `{mode}`, `{dir}`.

Empty variables (e.g., `{season}` in movie mode) are replaced with empty string.

## Execution

- Commands run via `sh -c "<expanded_command>"` using `std::process::Command`
- **Blocking mode (default):** Wait for process to exit, then proceed to next rip/exit
- **Non-blocking mode:** `std::thread::spawn` the execution, proceed immediately. Process may outlive bluback — this is documented and acceptable.
- **Output capture:** When `log_output = true`, stdout and stderr are captured via `Command::output()` (blocking) or read from spawned thread (non-blocking), logged via `log::info!` (stdout) and `log::warn!` (stderr).

## Integration Points

### CLI (`src/cli.rs`)

**Per-file hook:** After `remux()` returns and file size is logged (success) or partial file is deleted (failure), before the loop continues to the next playlist.

**Per-session hook:** After the playlist loop exits, before the function returns.

### TUI (`src/tui/dashboard.rs`)

**Per-file hook:** In `poll_active_job_session()`, after job status is set to `Done(file_size)` or `Failed(msg)`, before `start_next_job_session()` is called.

**Per-session hook:** In `check_all_done_session()`, after disc unmount, before transitioning to `Screen::Done`.

### Hook module (`src/hooks.rs`)

New module with:

```rust
/// Context for template variable expansion
pub struct HookContext {
    pub vars: HashMap<String, String>,
}

/// Run a post-rip hook if configured
pub fn run_post_rip(config: &Config, ctx: &HookContext, no_hooks: bool)

/// Run a post-session hook if configured
pub fn run_post_session(config: &Config, ctx: &HookContext, no_hooks: bool)

/// Expand {var} placeholders in command string
fn expand_template(command: &str, vars: &HashMap<String, String>) -> String

/// Execute shell command, capture output, log results
fn execute_hook(command: &str, blocking: bool, log_output: bool)
```

CLI and TUI build a `HookContext` with the appropriate variables and call `run_post_rip` / `run_post_session`. The hooks module handles all execution and logging logic.

## Error Handling

- **Hook fails (non-zero exit):** Log warning with exit code and stderr. Never fail the rip.
- **Command not found:** Logged as warning, same as non-zero exit.
- **No command configured:** Silently skipped, no log noise.
- **`--no-hooks`:** Both hooks skipped, logged at debug level.
- **Cancel/Ctrl+C during blocking hook:** Child process killed when parent exits (standard `sh -c` SIGTERM behavior). No special handling.
- **Non-blocking hook outlives bluback:** Acceptable and documented.

## Settings Panel

New "Hooks" separator section after the existing "Logging" section:

```
── Hooks ──────────────────────
Post-Rip Command      [                    ]   (Text)
  Run on Failure      [OFF]                    (Toggle)
  Blocking            [ON]                     (Toggle)
  Log Output          [ON]                     (Toggle)
Post-Session Command  [                    ]   (Text)
  Run on Failure      [OFF]                    (Toggle)
  Blocking            [ON]                     (Toggle)
  Log Output          [ON]                     (Toggle)
```

Standard settings panel behavior: changes apply to session immediately, `Ctrl+S` persists to config file with commented-out defaults for unset values.

## Security

TODO(debt): Research shell injection risk from template substitution. Filenames with shell metacharacters (`$`, `` ` ``, `"`, `;`, etc.) in template variables could be exploited when expanded into the `sh -c` command string. Future fix options: switch to environment variables for value passing, or add shell escaping to expanded values.

## Testing

- **Unit tests in `hooks.rs`:** `expand_template()` with known vars, unknown vars (left as-is), empty vars, mixed content
- **Unit tests in `config.rs`:** `HookConfig` TOML parsing for both `[post_rip]` and `[post_session]` tables, save/load roundtrip, commented defaults, validation of known keys
- **Unit tests in `types.rs`:** Settings panel construction with hook items, `to_config()` roundtrip
- No integration tests requiring shell execution — hook execution is inherently environment-dependent

## Files Changed

- New: `src/hooks.rs`
- Modified: `src/config.rs` (new structs, KNOWN_KEYS, save/load, validation)
- Modified: `src/main.rs` (new `--no-hooks` CLI flag, `mod hooks`)
- Modified: `src/cli.rs` (call `run_post_rip` / `run_post_session`)
- Modified: `src/tui/dashboard.rs` (call `run_post_rip` / `run_post_session`)
- Modified: `src/types.rs` (settings panel items, `to_config` roundtrip)
- Modified: `src/tui/settings.rs` (if rendering changes needed)
