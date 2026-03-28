# MKV Metadata Embedding Design

**Date:** 2026-03-28
**Version target:** v0.10
**Status:** Approved

## Overview

Embed MKV metadata tags into output files during remux using FFmpeg's format-level metadata API. Tags are auto-generated from TMDb/episode/movie data and optionally supplemented with user-defined custom tags. Configurable with a default of enabled.

## Config

New `[metadata]` section in `config.toml`:

```toml
[metadata]
enabled = true  # default: true
tags = { STUDIO = "HBO", COLLECTION = "My Blu-rays" }
```

- `enabled` — bool, default `true`. Overridden at runtime by `--no-metadata` CLI flag (same pattern as `--no-eject`).
- `tags` — optional TOML table of arbitrary string key-value pairs. Merged on top of auto-generated tags; user-defined tags win on conflict.
- Config validation: warn on non-string values in `tags` table. Add `metadata`, `metadata.enabled`, `metadata.tags` to `KNOWN_KEYS`.

## CLI Flag

`--no-metadata` — disables metadata embedding for this run. Overrides `metadata.enabled = true` in config.

## Auto-generated Tags

Tags are written only when a value is available. No empty-string tags are ever written.

### TV Mode

| Tag              | Source                                      | Example                        |
|------------------|---------------------------------------------|--------------------------------|
| `TITLE`          | Episode name from TMDb (or show title if no episode name) | `"The Rains of Castamere"` |
| `SHOW`           | Show name from TMDb or `--title`            | `"Game of Thrones"`            |
| `SEASON_NUMBER`  | Season selection                            | `"3"`                          |
| `EPISODE_SORT`   | Episode number (first episode for multi-ep) | `"9"`                          |
| `DATE_RELEASED`  | `first_air_date` from TMDb                  | `"2013-06-02"`                 |
| `REMUXED_WITH`        | `env!("CARGO_PKG_VERSION")` at build time   | `"bluback v0.9.2"`             |

For multi-episode playlists (e.g., E03-E04): `TITLE` joins episode names with `" / "` separator (e.g., `"Episode 3 / Episode 4"`), `EPISODE_SORT` uses the first episode number.

### Movie Mode

| Tag              | Source                          | Example              |
|------------------|---------------------------------|----------------------|
| `TITLE`          | Movie title from TMDb or `--title` | `"Blade Runner 2049"` |
| `DATE_RELEASED`  | `release_date` from TMDb        | `"2017-10-06"`       |
| `REMUXED_WITH`        | `env!("CARGO_PKG_VERSION")` at build time | `"bluback v0.9.2"`   |

### Fallback (TMDb Skipped)

When TMDb is skipped (via `--title` or Esc in TUI), only `TITLE` (from `--title` or volume label) and `REMUXED_WITH` are written.

### Custom Tag Merge

Custom `tags` from config are merged last, overriding any auto-generated key with the same name.

## Data Flow

### New Struct (`types.rs`)

```rust
pub struct MkvMetadata {
    pub tags: HashMap<String, String>,
}
```

A resolved, ready-to-write tag map. No TV/movie logic — that's handled upstream.

### Build Site (`workflow.rs`)

New function `build_metadata()` takes available context (show name, season, episodes, movie title, year, config custom tags, encoder version) and returns `Option<MkvMetadata>`:
- Returns `None` when metadata is disabled.
- Called alongside `build_output_filename` / `prepare_remux_options`.

### Plumbing

`RemuxOptions` gains a new field:

```rust
pub metadata: Option<MkvMetadata>,
```

Both CLI (`cli.rs`) and TUI (`tui/dashboard.rs`) pass it through when constructing `RemuxOptions`.

### Injection Point (`remux.rs`)

Between chapter injection (current line 261) and `write_header_with` (current line 268):

```rust
if let Some(ref meta) = options.metadata {
    let mut dict = Dictionary::new();
    for (k, v) in &meta.tags {
        dict.set(k, v);
    }
    octx.set_metadata(dict);
}
```

~6 lines in the remux path. Metadata is set on the format context before the header is written.

## Future Work

- **Per-stream metadata titles** (e.g., `"English - DTS-HD MA 5.1"`) — to be implemented alongside per-stream track selection later in v0.10. Add a TODO comment near the stream mapping loop in `remux.rs`.

## Testing

### Unit Tests (`workflow.rs`)

- `build_metadata` with full TV context → all expected tags present
- `build_metadata` with full movie context → movie tags present
- `build_metadata` with TMDb skipped (title-only) → only `TITLE` + `REMUXED_WITH`
- `build_metadata` with custom tags → custom tags present
- `build_metadata` with custom tag overriding auto-generated key → custom wins
- `build_metadata` with metadata disabled → returns `None`
- No empty-string tags emitted in any scenario

### Unit Tests (`config.rs`)

- Parse `[metadata]` section from TOML (enabled + tags)
- Parse missing `[metadata]` section → defaults (`enabled = true`, no custom tags)
- Validation: non-string values in `tags` produce warning
- Roundtrip save/load with metadata section

### Not Tested

FFmpeg actually writing the tags into the MKV file — this is an FFmpeg behavior test, not a bluback logic test. The API call (`octx.set_metadata()`) is straightforward and already exercised by the existing remux infrastructure.
