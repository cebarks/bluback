# Generate Subcommand Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `bluback generate completions <shell>` and `bluback generate man` subcommands, and include generated files in release tarballs.

**Architecture:** New `src/generate.rs` module with a derive-based `GenerateArgs` struct dispatched via argv pre-check in `main()` (same pattern as `history`). A `full_command()` function composes `Args::command()` with `HistoryArgs::command()` and `GenerateArgs::command()` so completions cover the full CLI. Two new dependencies: `clap_complete` for shell completions, `clap_mangen` for man pages.

**Tech Stack:** Rust, clap 4 (derive), clap_complete 4, clap_mangen 0.3

---

## File Map

| Action | File | Purpose |
|--------|------|---------|
| Create | `src/generate.rs` | `GenerateArgs`, `GenerateCommand`, `run_generate()`, `full_command()` |
| Modify | `src/main.rs:1` | Add `mod generate;` declaration |
| Modify | `src/main.rs:255-266` | Add argv pre-check dispatch for `generate` |
| Modify | `Cargo.toml:11-27` | Add `clap_complete` and `clap_mangen` dependencies |
| Create | `tests/generate_test.rs` | Integration tests for all generate subcommands |
| Modify | `docs/cli-reference.md:78-111` | Add `generate` subcommand documentation |
| Modify | `.github/workflows/release.yml:84-96,117-129` | Add completions/man generation + updated packaging |

---

### Task 1: Add dependencies

**Files:**
- Modify: `Cargo.toml:11-27`

- [ ] **Step 1: Add clap_complete and clap_mangen to Cargo.toml**

Add these two lines to the `[dependencies]` section, after the existing `clap` line:

```toml
clap_complete = "4"
clap_mangen = "0.3"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles cleanly (new deps unused but that's fine — next task uses them)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: add clap_complete and clap_mangen dependencies"
```

---

### Task 2: Create src/generate.rs with GenerateArgs and full_command()

**Files:**
- Create: `src/generate.rs`
- Modify: `src/main.rs:1` (add `mod generate;`)

- [ ] **Step 1: Write the unit test for full_command()**

Create `src/generate.rs` with the test first. The test verifies that `full_command()` includes `history` and `generate` subcommands with their sub-subcommands:

```rust
use std::io;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::history_cli;
use crate::Args;

#[derive(Parser, Debug)]
#[command(name = "bluback generate", about = "Generate shell completions and man pages")]
pub struct GenerateArgs {
    #[command(subcommand)]
    pub command: GenerateCommand,
}

#[derive(Subcommand, Debug)]
pub enum GenerateCommand {
    /// Generate shell completion script
    Completions {
        /// Target shell
        shell: Shell,
    },
    /// Generate man page
    Man,
}

/// Build the full CLI command tree including pre-dispatched subcommands
/// (history, generate) so completions and man pages cover the entire CLI.
pub fn full_command() -> clap::Command {
    Args::command()
        .subcommand(history_cli::HistoryArgs::command().name("history"))
        .subcommand(GenerateArgs::command().name("generate"))
}

pub fn run_generate(args: GenerateArgs) -> anyhow::Result<()> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_command_includes_subcommands() {
        let cmd = full_command();
        let sub_names: Vec<&str> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(sub_names.contains(&"history"), "missing history subcommand");
        assert!(sub_names.contains(&"generate"), "missing generate subcommand");
    }

    #[test]
    fn full_command_history_has_list() {
        let cmd = full_command();
        let history = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "history")
            .expect("history subcommand exists");
        let sub_names: Vec<&str> = history.get_subcommands().map(|s| s.get_name()).collect();
        assert!(sub_names.contains(&"list"), "history missing list subcommand");
        assert!(sub_names.contains(&"show"), "history missing show subcommand");
        assert!(sub_names.contains(&"stats"), "history missing stats subcommand");
    }

    #[test]
    fn full_command_generate_has_completions_and_man() {
        let cmd = full_command();
        let generate = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "generate")
            .expect("generate subcommand exists");
        let sub_names: Vec<&str> = generate.get_subcommands().map(|s| s.get_name()).collect();
        assert!(sub_names.contains(&"completions"), "generate missing completions subcommand");
        assert!(sub_names.contains(&"man"), "generate missing man subcommand");
    }
}
```

- [ ] **Step 2: Add mod declaration to main.rs**

Add `mod generate;` to the module declarations at the top of `src/main.rs`, after `mod duration;`:

```rust
mod generate;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test full_command -- --test-threads=1`
Expected: 3 tests pass. The `todo!()` in `run_generate` doesn't matter since it's not called.

- [ ] **Step 4: Commit**

```bash
git add src/generate.rs src/main.rs
git commit -m "feat: add generate module with GenerateArgs and full_command()"
```

---

### Task 3: Implement run_generate()

**Files:**
- Modify: `src/generate.rs`

- [ ] **Step 1: Implement run_generate()**

Replace the `todo!()` body in `run_generate` with the actual dispatch logic:

```rust
pub fn run_generate(args: GenerateArgs) -> anyhow::Result<()> {
    match args.command {
        GenerateCommand::Completions { shell } => {
            let mut cmd = full_command();
            clap_complete::generate(shell, &mut cmd, "bluback", &mut io::stdout());
        }
        GenerateCommand::Man => {
            let cmd = full_command();
            let man = clap_mangen::Man::new(cmd);
            man.render(&mut io::stdout())?;
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles cleanly, no warnings

- [ ] **Step 3: Commit**

```bash
git add src/generate.rs
git commit -m "feat: implement shell completion and man page generation"
```

---

### Task 4: Wire up argv dispatch in main()

**Files:**
- Modify: `src/main.rs:255-266`

- [ ] **Step 1: Add generate dispatch after history dispatch**

In `src/main.rs`, immediately after the history pre-check block (after line 266's closing `}`), add the generate pre-check:

```rust
    // Early intercept for generate subcommand
    if raw_args.get(1).map(|s| s.as_str()) == Some("generate") {
        use clap::Parser;
        let gen_args = generate::GenerateArgs::parse_from(&raw_args[1..]);
        if let Err(e) = generate::run_generate(gen_args) {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
        return;
    }
```

- [ ] **Step 2: Smoke test manually**

Run: `cargo run -- generate completions bash 2>/dev/null | head -5`
Expected: output starts with bash completion script (e.g., `_bluback()` function definition or similar)

Run: `cargo run -- generate man 2>/dev/null | head -3`
Expected: output starts with `.TH` or `.ie` (troff man page format)

- [ ] **Step 3: Verify existing tests still pass**

Run: `cargo test`
Expected: all existing tests pass (the new dispatch doesn't interfere with normal Args parsing)

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire up generate subcommand dispatch in main()"
```

---

### Task 5: Integration tests

**Files:**
- Create: `tests/generate_test.rs`

- [ ] **Step 1: Write integration tests**

Create `tests/generate_test.rs`:

```rust
use std::process::Command;

fn bluback() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bluback"))
}

#[test]
fn generate_completions_bash() {
    let out = bluback()
        .args(["generate", "completions", "bash"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.is_empty(), "bash completions should not be empty");
    assert!(stdout.contains("bluback"), "bash completions should reference the binary name");
}

#[test]
fn generate_completions_zsh() {
    let out = bluback()
        .args(["generate", "completions", "zsh"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.is_empty(), "zsh completions should not be empty");
    assert!(stdout.contains("bluback"), "zsh completions should reference the binary name");
}

#[test]
fn generate_completions_fish() {
    let out = bluback()
        .args(["generate", "completions", "fish"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.is_empty(), "fish completions should not be empty");
    assert!(stdout.contains("bluback"), "fish completions should reference the binary name");
}

#[test]
fn generate_man_page() {
    let out = bluback()
        .args(["generate", "man"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.is_empty(), "man page should not be empty");
    assert!(stdout.contains("bluback"), "man page should reference the binary name");
}

#[test]
fn generate_completions_invalid_shell() {
    let out = bluback()
        .args(["generate", "completions", "invalid"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "invalid shell should fail");
}

#[test]
fn generate_no_subcommand_shows_help() {
    let out = bluback()
        .args(["generate"])
        .output()
        .unwrap();
    // clap shows help/error when required subcommand is missing
    assert!(!out.status.success());
}
```

- [ ] **Step 2: Run the integration tests**

Run: `cargo test --test generate_test`
Expected: all 6 tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/generate_test.rs
git commit -m "test: add integration tests for generate subcommand"
```

---

### Task 6: Update CLI reference docs

**Files:**
- Modify: `docs/cli-reference.md`

- [ ] **Step 1: Add generate subcommand section**

In `docs/cli-reference.md`, add a new section after the History Subcommand section (after line 111, before the Exit Codes section). Insert:

```markdown
## Generate Subcommand

```
bluback generate <COMMAND>
```

| Command | Description |
|---------|-------------|
| `completions <SHELL>` | Generate shell completion script (`bash`, `zsh`, or `fish`) |
| `man` | Generate man page |

Output goes to stdout. Pipe to the appropriate location for your shell:

```bash
# Bash
bluback generate completions bash > ~/.local/share/bash-completion/completions/bluback

# Zsh
bluback generate completions zsh > ~/.local/share/zsh/site-functions/_bluback

# Fish
bluback generate completions fish > ~/.config/fish/completions/bluback.fish

# Man page
bluback generate man | sudo tee /usr/local/share/man/man1/bluback.1 > /dev/null
sudo mandb
```
```

- [ ] **Step 2: Commit**

```bash
git add docs/cli-reference.md
git commit -m "docs: add generate subcommand to CLI reference"
```

---

### Task 7: Update release workflow

**Files:**
- Modify: `.github/workflows/release.yml:84-96,117-129`

- [ ] **Step 1: Update build-linux job**

In `.github/workflows/release.yml`, replace the `Package` step in `build-linux` (lines 87-91) with a generate step followed by the updated package step:

```yaml
      - name: Generate completions and man page
        run: |
          mkdir -p completions man
          target/release/bluback generate completions bash > completions/bluback.bash
          target/release/bluback generate completions zsh > completions/bluback.zsh
          target/release/bluback generate completions fish > completions/bluback.fish
          target/release/bluback generate man > man/bluback.1

      - name: Package
        run: |
          tar czf bluback-${{ matrix.target }}.tar.gz \
            -C target/release bluback \
            -C "$GITHUB_WORKSPACE" LICENSE README.md completions man
```

- [ ] **Step 2: Update build-macos job**

Apply the same change to `build-macos` — replace the `Package` step (lines 120-124) with the same generate + package steps as above.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: include shell completions and man page in release tarballs"
```

---

### Task 8: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: all tests pass (unit + integration)

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: no warnings

- [ ] **Step 3: Run formatter**

Run: `rustup run stable cargo fmt`
Expected: no changes (or apply if needed)

- [ ] **Step 4: Verify generated output looks correct**

Run: `cargo run -- generate completions bash | wc -l`
Expected: substantial output (100+ lines)

Run: `cargo run -- generate completions zsh | wc -l`
Expected: substantial output (100+ lines)

Run: `cargo run -- generate completions fish | wc -l`
Expected: substantial output (50+ lines)

Run: `cargo run -- generate man | wc -l`
Expected: substantial output (50+ lines)

- [ ] **Step 5: Verify completions include history and generate subcommands**

Run: `cargo run -- generate completions bash 2>/dev/null | grep -c "history\|generate"`
Expected: non-zero count (both subcommands appear in completions)
