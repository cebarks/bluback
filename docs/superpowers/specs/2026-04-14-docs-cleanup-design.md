# Documentation Cleanup — Design Spec

**Date:** 2026-04-14
**Current version:** v0.11.0

## Problem

The README.md (319 lines) has grown into a reference manual — full CLI options table, env vars table, config examples, TUI keybindings, feature deep-dives — making it hard to scan for newcomers. Much of the content is stale: `--min-duration` was renamed to `--min-probe-duration` (default changed from 900 to 30), `--hide-specials` is undocumented, env vars table is missing several entries (`BLUBACK_AUTO_DETECT`, `BLUBACK_HISTORY`, `BLUBACK_HISTORY_RETENTION`), and features like batch mode, hooks, metadata, history, and auto-detection have no README coverage at all. CLAUDE.md (365 lines) duplicates much of the same reference material with the same staleness. The roadmap is significantly out of date (milestone assignments no longer match reality). The `docs/superpowers/` directory of 63 internal spec/plan files has no explanation.

## Goals

1. README becomes a concise landing page (~100-150 lines) — what, why, install, quick start, links
2. Reference material moves to focused docs pages
3. CLAUDE.md trims duplicated reference material, keeps architecture/design decision context
4. Roadmap reflects current project state
5. Internal development artifacts are briefly explained

## Scope

| File | Action |
|------|--------|
| `README.md` | Rewrite as landing page |
| `docs/configuration.md` | New — config file, env vars, filename templates |
| `docs/cli-reference.md` | New — full CLI options, history subcommand, exit codes |
| `docs/features.md` | New — stream selection, verification, chapters, batch, hooks, metadata, history, auto-detection |
| `docs/keybindings.md` | New — TUI keybindings + screen flow |
| `CLAUDE.md` | Trim duplicates, update stale content |
| `docs/ROADMAP-1.0.md` | Archive as `-original.md`, write fresh version |
| `docs/superpowers/README.md` | New — brief explainer |
| `docs/macos-installation.md` | Review pass for accuracy |

## 1. README.md

Target: ~100-150 lines. Structure:

```
# bluback

One-paragraph description (lossless Blu-ray backup, FFmpeg bindings, TUI/CLI).

## Features

Bullet list of key capabilities:
- Lossless remux (no re-encoding)
- TUI wizard + headless CLI
- TMDb integration for automatic episode naming
- Chapter preservation
- Per-stream track selection
- Batch mode (continuous multi-disc ripping)
- Multi-drive support with parallel sessions
- Rip verification
- Post-rip hooks
- Rip history database
- Auto-detection heuristics
- MKV metadata embedding
- macOS + Linux

## Requirements

Short version:
- FFmpeg shared libraries + libbluray + libaacs + KEYDB.cfg
- Blu-ray drive
- Linux or macOS
- Optional: TMDb API key

Link to build deps section or docs/configuration.md for details.

## Installation

### From crates.io
cargo install bluback

### Pre-built binaries
Link to releases page. Note Linux x86_64 + aarch64 available.

### From source
Build deps table (Fedora/Ubuntu/Arch/macOS one-liners).
git clone + cargo build --release.
Link to docs/macos-installation.md for macOS details.

## Quick Start

4-5 essential examples:
- bluback (auto-detect, TUI)
- bluback -d /dev/sr0 -o ~/rips
- bluback --movie -o ~/movies
- bluback --batch (continuous ripping)
- bluback --check (validate setup)

## Configuration

2-3 sentences: config file location, --settings / Ctrl+S, env var overrides.
Link to docs/configuration.md.

## Documentation

Links table:
- Configuration → docs/configuration.md
- CLI Reference → docs/cli-reference.md
- Features Guide → docs/features.md
- TUI Keybindings → docs/keybindings.md
- macOS Installation → docs/macos-installation.md
- Roadmap → docs/ROADMAP-1.0.md

## AACS Decryption

Short version (4-5 lines): need KEYDB.cfg, MKBv72+ caveat, link to macOS guide
for libmmbd setup.

## AI Disclosure

Keep as-is.

## License

Keep as-is.
```

## 2. docs/configuration.md

Content moved from README + expanded. **All moved content must be verified against current code** — do not copy stale entries verbatim.

- Config file location and resolution (`--config` → `BLUBACK_CONFIG` → default)
- Full TOML example with all sections (current README example + `[streams]`, `[metadata]`, `[post_rip]`, `[post_session]`, `[history]`). Config tables like `[post_rip]` and `[metadata]` live here as config reference; `docs/features.md` covers their behavior and usage.
- Environment variables table — verify against `src/types.rs` `import_env_vars()`. Known stale entries: `BLUBACK_MIN_DURATION` → `BLUBACK_MIN_PROBE_DURATION`. Known missing: `BLUBACK_AUTO_DETECT`, `BLUBACK_HISTORY`, `BLUBACK_HISTORY_RETENTION`.
- Filename templates: placeholder list, bracket groups, priority chain
- `--settings` standalone mode and `Ctrl+S` overlay
- TMDb API key resolution chain

## 3. docs/cli-reference.md

Content moved from README. **Must be generated from `--help` output**, not copied from stale README.

- Full options table — generated from `bluback --help`, not the README's outdated version. Known changes: `--min-duration` → `--min-probe-duration` (default 30), `--hide-specials` (new, undocumented)
- History subcommand usage and flags (from `bluback history --help` and subcommand `--help`)
- Exit codes table
- Flag interactions and conflicts (e.g., `--format` vs `--format-preset`, `--batch` conflicts, `--no-history` vs `--ignore-history`)
- Headless mode notes (`--yes` auto-enable, `--title`/`--playlists` for scripting)
- Note: `history clear --yes` does NOT auto-enable on non-TTY (unlike main `--yes`)

## 4. docs/features.md

Mix of content moved from README and **new content written from CLAUDE.md/code** for features that were never documented in user-facing docs:

**Moved from README:**
- **Stream Selection** — config defaults, CLI flags, TUI track picker
- **Rip Verification** — quick vs full, config/CLI, TUI prompts
- **Chapter Preservation** — how it works, MPLS extraction

**New content** (these features exist but have no user-facing documentation — write from CLAUDE.md and code):
- **Batch Mode** — continuous ripping workflow, episode auto-advance, config/CLI
- **Post-Rip Hooks** — template variables, blocking/non-blocking (config syntax in `docs/configuration.md`, behavior and usage here)
- **MKV Metadata** — auto-generated tags, custom tags, `--no-metadata` (config syntax in `docs/configuration.md`, behavior here)
- **Rip History** — database location, `bluback history` subcommand, episode continuation, duplicate detection, retention
- **Auto-Detection** — heuristics overview, confidence levels, TMDb runtime matching
- **Multi-Drive** — parallel sessions, tab UI, inter-session linking

Each section: what it does, how to enable, example usage.

## 5. docs/keybindings.md

Content moved from README + expanded:

- Global keybindings
- Per-screen keybindings (TMDb Search, Season, Playlist Manager, Ripping, Done)
- Settings panel keybindings
- History overlay keybindings
- Screen flow diagram (TV mode vs Movie mode)

## 6. CLAUDE.md

### Remove (now in docs/)

- "CLI Flags" section — full options table and history subcommand (→ `docs/cli-reference.md`)
- "TUI Keybindings" section (→ `docs/keybindings.md`)
- "Config Fields" bullet list (→ `docs/configuration.md`). Note: `min_duration` is now `min_probe_duration`.
- "Dependencies" table (Cargo.toml is authoritative)
- "Environment variable overrides" list within Key Design Decisions (→ `docs/configuration.md`). Note: several env var names are stale.

### Keep and update

- "What This Is" — keep, verify accurate
- "Background & Context" (Why not MakeMKV, How it works) — keep, valuable context
- "Build Requirements" / "Runtime Requirements" — keep, needed for dev setup
- "AACS Decryption Details" — keep, critical gotchas not obvious from code
- "Build & Test Commands" + "Pre-Commit Checklist" — keep
- "Architecture" section — keep and update:
  - Data flow list (verify module descriptions match current code)
  - "Two UI Modes" (verify)
  - "TUI Screen Flow" (verify)
  - "Filename Format Resolution" (verify)
- "Key Design Decisions" — trim to decisions that document gotchas, workarounds for external bugs, or rationale not inferable from the code. Keep: AACS path naming, libbluray stderr suppression, fork safety, signal handling, MountGuard, blocking I/O choice, episode reassignment logic. Remove: entries that describe straightforward user-facing features whose behavior is now covered in `docs/features.md` or `docs/keybindings.md` (e.g., "All playlists visible", "TMDb API key" lookup chain, "Settings overlay" basic description). These entries are being trimmed, not relocated — their user-facing aspects are covered elsewhere.
- "Testing" section — keep the summary (test counts, what's tested where), update counts if stale

### Target

~250 lines (down from 365). Architecture and design decisions remain the primary value — these aren't derivable from docs/ pages and are specifically useful for AI-assisted development.

## 7. Roadmap

### Archive

Rename `docs/ROADMAP-1.0.md` → `docs/ROADMAP-1.0-original.md` (no content changes).

### New `docs/ROADMAP-1.0.md`

Short, accurate document (~80-100 lines):

```
# bluback Roadmap

Current version: v0.11.0

## Released

| Version | Theme | Highlights |
|---------|-------|------------|
| v0.6 | Stability & Safety | Error handling, signal handling, overwrite protection, exit codes, AACS backend |
| v0.7 | Architecture & CLI | Workflow extraction, specials CLI, headless progress, --check, --list-playlists |
| v0.8 | macOS Support | Platform-specific disc ops, Homebrew library discovery, macOS CI |
| v0.9 | Multi-Drive & CI | Parallel sessions, tab UI, drive monitor, 5-platform CI |
| v0.10 | Quality of Life | Log files, MKV metadata, post-rip hooks, verification, track selection, batch mode, auto-detection |
| v0.11 | History | SQLite rip history, episode continuation, duplicate detection |

## Upcoming

### v0.12 — DVD Support
(brief description of what's planned)

### v0.13 — UHD Blu-ray
(brief description)

### v0.14 — Distribution
(shell completions, man page)

### v1.0 — Release
(README rewrite — happening now, integration testing, final release)

## Post-1.0
(bullet list of deferred items: GUI, Windows, resume, pause, notifications, transcoding)

## Original Planning Document
See [ROADMAP-1.0-original.md](ROADMAP-1.0-original.md) for the detailed
milestone planning document from March 2026.
```

Milestone numbers adjusted to reflect actual shipping: auto-detection shipped in v0.10.6 (part of v0.10), history shipped as v0.11. DVD/UHD shift to v0.12/v0.13.

## 8. docs/superpowers/README.md

```
# Development Specs & Plans

Internal design specifications and implementation plans generated during
bluback development. Each feature goes through a brainstorming → spec → plan →
implementation cycle.

- `specs/` — Design documents describing what to build and why
- `plans/` — Step-by-step implementation plans derived from specs
```

## 9. docs/macos-installation.md

Review pass — check for:
- Release binary section references a placeholder `vVERSION` — update or note that users should check the releases page
- Verify Homebrew formula patching instructions are still accurate
- Verify symlink paths are correct
- No content restructuring needed — the guide is well-organized

## Cross-Cutting Concerns

### Link integrity

All internal links between docs pages use relative paths. README links to `docs/*.md`. Docs pages can cross-reference each other.

### Content accounting

All reference material removed from README or CLAUDE.md has a destination in the new docs structure. CLAUDE.md key design decisions that describe straightforward user-facing features (now covered by `docs/features.md` or `docs/keybindings.md`) are trimmed — their user-facing aspects are documented in the new docs, and the implementation details are self-evident from the code.

### Stale content must be verified

All content being moved to new docs pages must be verified against current `--help` output, source code, and `Cargo.toml` — not copied verbatim from stale README/CLAUDE.md. Known stale items are called out in each section above.

### What's NOT changing

- `docs/superpowers/specs/*` and `docs/superpowers/plans/*` — untouched
- Source code — no code changes
- Cargo.toml — no changes
