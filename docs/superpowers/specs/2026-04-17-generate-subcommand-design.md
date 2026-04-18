# Shell Completions & Man Page Generation

**Date:** 2026-04-17
**Status:** Approved
**Version target:** v0.14

## Goal

Add a `bluback generate` subcommand that produces shell completions (bash, zsh, fish) and a man page, and include these in release tarballs.

## Subcommand Structure

```
bluback generate completions <SHELL>   # bash | zsh | fish → stdout
bluback generate man                   # man page → stdout
```

Output goes to stdout. Users pipe to their desired location:

```bash
bluback generate completions bash > ~/.local/share/bash-completion/completions/bluback
bluback generate completions zsh > ~/.local/share/zsh/site-functions/_bluback
bluback generate completions fish > ~/.config/fish/completions/bluback.fish
bluback generate man > ~/.local/share/man/man1/bluback.1
```

### Dispatch

Dispatched via argv pre-check in `main()`, identical to the existing `history` subcommand pattern. The check fires before clap parses `Args`, avoiding flag conflicts.

```rust
if raw_args.get(1).map(|s| s.as_str()) == Some("generate") {
    // parse GenerateArgs from &raw_args[1..], dispatch, return
}
```

### Module: `src/generate.rs`

New module containing:

- `GenerateArgs` — `#[derive(Parser)]` struct with a `GenerateCommand` subcommand enum (`Completions { shell: Shell }`, `Man`).
- `run_generate(args: GenerateArgs)` — dispatches to completion or man page generation.
- `full_command()` — returns `Args::command()` augmented with `history` and `generate` subcommands so completions cover the full CLI surface.

### Augmented Command

Because `history` and `generate` are dispatched before clap parsing, they don't appear in `Args::command()`. The `full_command()` function composes the existing derive-generated `Command` objects to avoid duplication:

```rust
fn full_command() -> clap::Command {
    use clap::CommandFactory;
    Args::command()
        .subcommand(history_cli::HistoryArgs::command().name("history"))
        .subcommand(GenerateArgs::command().name("generate"))
}
```

This reuses the derive-generated argument definitions from `HistoryArgs` and `GenerateArgs`, so completions automatically include all flags and subcommands (e.g., `history list --limit`, `history clear --older-than`). Changes to those structs propagate without touching `full_command()`.

## Dependencies

Added to `[dependencies]` (not build-deps, since generation happens at runtime):

- `clap_complete = "4"` — shell completion script generation
- `clap_mangen = "0.3"` — man page generation (0.3.x targets clap 4.x)

## Release Artifacts

### Tarball Structure

```
bluback-<target>.tar.gz
├── bluback
├── LICENSE
├── README.md
├── completions/
│   ├── bluback.bash
│   ├── bluback.zsh
│   └── bluback.fish
└── man/
    └── bluback.1
```

### Workflow Changes

Each build job (linux x86_64, linux aarch64, macOS aarch64) adds a post-build step to generate completions and man page, then includes them in the tarball. All runners are native, so the built binary is runnable on the build host.

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

## Testing

### Integration Tests (`tests/generate_test.rs`)

Run the binary with each generate subcommand, assert exit code 0 and non-empty stdout:

- `bluback generate completions bash`
- `bluback generate completions zsh`
- `bluback generate completions fish`
- `bluback generate man`

### Unit Test (in `src/generate.rs`)

Verify `full_command()` includes `history` and `generate` subcommands, ensuring completions cover the full CLI.

## Documentation

Update `docs/cli-reference.md` with:

- `generate` subcommand usage
- Shell-specific installation paths for completions (bash, zsh, fish)
- Man page installation path

## Scope Exclusions

- PowerShell completions (deferred to Windows support, post-1.0)
- Packaging for distro package managers (homebrew formula, AUR, etc.)
- `build.rs` approach — runtime subcommand is simpler and more portable
