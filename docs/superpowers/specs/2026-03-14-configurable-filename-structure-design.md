# Configurable Filename Structure

## Problem

Filename format is hardcoded to `S{season}E{episode}_{title}.mkv` for TV and `{title}_({year}).mkv` for movies. Users who organize media for Plex, Jellyfin, or custom folder structures have no way to change this without editing source code.

## Solution

Add a template-based filename system with:
- A TOML config file (`~/.config/bluback/config.toml`) for persistent settings
- CLI flags (`--format`, `--format-preset`) for per-invocation overrides
- Three built-in presets (default, plex, jellyfin)
- Full set of placeholders including media metadata from ffprobe

## Config File

Location: `~/.config/bluback/config.toml`

```toml
tmdb_api_key = "abc123"

# Built-in preset: "default", "plex", "jellyfin"
preset = "default"

# Custom templates (override preset when set)
# tv_format = "{show}/Season {season}/S{season}E{episode} - {title}.mkv"
# movie_format = "{title} ({year})/{title} ({year}).mkv"
```

### TMDb API Key Migration

The existing `~/.config/bluback/tmdb_api_key` flat file and `TMDB_API_KEY` env var are still supported. Resolution order:
1. `tmdb_api_key` field in `config.toml`
2. Contents of `~/.config/bluback/tmdb_api_key` (backwards compat)
3. `TMDB_API_KEY` environment variable (existing fallback, preserved)

No automatic migration — all paths coexist indefinitely.

## CLI Flags

```
--format <TEMPLATE>          Custom filename template string
--format-preset <PRESET>     Built-in preset name: default, plex, jellyfin
```

These are mutually exclusive (enforced via clap `ArgGroup`). Both override the config file.

Since there are separate TV and movie templates, `--format` applies to whichever mode is active (movie mode vs TV mode). The config file has `tv_format` and `movie_format` as separate keys for setting both independently.

Note: `--format` is a single flag that applies to the active mode. Users who need different TV and movie templates simultaneously should use the config file's `tv_format`/`movie_format` keys. If a movie template omits `{part}`, multi-disc movies may produce duplicate filenames — the existing deduplication logic (appending `_2`, `_3`, etc.) handles this gracefully.

## Format Resolution Order

1. `--format` CLI flag (highest priority)
2. `--format-preset` CLI flag
3. `tv_format` / `movie_format` in config.toml (mode-dependent)
4. `preset` in config.toml
5. `default` preset (lowest priority, current behavior)

## Presets

### `default` (current behavior)
- TV: `S{season}E{episode}_{title}.mkv`
- Movie: `{title}_({year})_pt{part}.mkv`

Note: In the default preset, `_pt{part}` is conditionally included — omitted when `{part}` is empty (matching current `make_movie_filename` behavior where `part: None` produces no suffix).

### `plex`
- TV: `{show}/Season {season}/S{season}E{episode} - {title} [Bluray-{resolution}][{audio} {channels}][{codec}].mkv`
- Movie: `{title} ({year})/Movie [Bluray-{resolution}][{audio} {channels}][{codec}].mkv`

### `jellyfin`
- TV: `{show}/Season {season}/S{season}E{episode} - {title}.mkv`
- Movie: `{title} ({year})/{title} ({year}).mkv`

## Placeholders

All placeholders use `{name}` syntax. Season and episode are always zero-padded to 2 digits.

### TV mode
| Placeholder | Example | Source |
|---|---|---|
| `{show}` | `Stargate Universe` | TMDb show name, falls back to volume label show name |
| `{season}` | `01` | User-provided / label-parsed, zero-padded |
| `{episode}` | `03` | Assigned episode number, zero-padded |
| `{title}` | `Air (Part 1)` | TMDb episode name |
| `{playlist}` | `00801` | Raw playlist number from disc |

### Movie mode
| Placeholder | Example | Source |
|---|---|---|
| `{title}` | `The Matrix` | TMDb movie title |
| `{year}` | `1999` | TMDb release year |
| `{part}` | `1` | Multi-disc part number |
| `{playlist}` | `00800` | Raw playlist number |

### Media info (both modes)
| Placeholder | Example | Source |
|---|---|---|
| `{resolution}` | `1080p`, `2160p` | Video stream height + `p` suffix |
| `{width}` | `1920` | Raw pixel width |
| `{height}` | `1080` | Raw pixel height |
| `{codec}` | `h264`, `hevc`, `vc1` | Video codec name |
| `{hdr}` | `HDR10`, `DV`, `HDR10+`, `SDR` | Color transfer / side_data metadata |
| `{aspect_ratio}` | `16:9` | Display aspect ratio |
| `{framerate}` | `23.976` | Video frame rate |
| `{bit_depth}` | `10`, `8` | Bits per raw sample |
| `{profile}` | `Main 10`, `High` | Video codec profile |
| `{audio}` | `truehd`, `dts-hd ma`, `ac3` | Primary audio codec (+ profile where relevant) |
| `{channels}` | `5.1`, `7.1`, `2.0` | Primary audio channel layout |
| `{audio_lang}` | `eng`, `jpn` | Primary audio language tag |

### Disc info (both modes)
| Placeholder | Example | Source |
|---|---|---|
| `{disc}` | `2` | Disc number from volume label |
| `{label}` | `SGU_BR_S1D2` | Raw volume label string |

### HDR Detection Logic

Derived from ffprobe JSON fields on the video stream (`-print_format json -show_streams`):

- Check `color_transfer` field on the video stream:
  - `"smpte2084"` → check `side_data_list` array for entry with `"side_data_type": "DOVI configuration record"` → `DV` if present, else `HDR10`
  - `"arib-std-b67"` → `HLG`
- Check `side_data_list` for `"side_data_type": "HDR Dynamic Metadata SMPTE2094-40"` → `HDR10+`
- Otherwise → `SDR`

Note: `side_data_list` is a JSON array nested under each stream object. Requires ffprobe invocation with `-show_streams` (not `-show_format`) to access per-stream side data.

### Edge Cases

- **Unknown placeholder**: Left as literal text (e.g., `{foo}` stays `{foo}`)
- **Empty value**: Rendered as empty string. Bracket groups cleaned up (see below)
- **No episode info / no TMDb match**: Falls back to `playlist{num}.mkv` (unchanged from current behavior)
- **No TMDb but `{show}` used**: Falls back to show name parsed from volume label (`LabelInfo.show`). If label also can't be parsed, `{show}` renders as `Unknown`
- **Probe failure**: All media info placeholders render empty; bracket cleanup applies
- **Path traversal**: `..` path segments are stripped from rendered templates to prevent writing outside the output directory

### Bracket Cleanup Algorithm

After placeholder substitution, bracket cleanup removes bracket groups that contain only whitespace/separators:

1. Regex: `\[[^\[\]]*\]` — match innermost bracket groups
2. If the content inside the brackets (after placeholder substitution) is empty or contains only whitespace, hyphens, or spaces, remove the entire bracket group including surrounding whitespace
3. Apply iteratively until no more empty groups remain (handles nested cases, though unlikely in practice)
4. Clean up resulting double-spaces and leading/trailing whitespace per path component

Example: `[Bluray-{resolution}][{audio} {channels}][{codec}]` with only `{resolution}=1080p` and `{codec}=hevc` filled → `[Bluray-1080p][hevc]`

## Subdirectory Handling

When the rendered template contains `/`, the path is treated as relative to the `-o` output directory. Parent directories are created via `std::fs::create_dir_all` before the file is written.

## Architecture Changes

### New file: `src/config.rs`
- `Config` struct with serde TOML deserialization
- `load_config()` → reads `~/.config/bluback/config.toml`, returns `Config` (with defaults if file missing)
- `Config::tmdb_api_key()` → returns key from TOML, falling back to flat file
- `Config::resolve_format(is_movie: bool, cli_format: Option<&str>, cli_preset: Option<&str>) -> String` — implements the resolution order

### New struct in `src/types.rs`: `MediaInfo`
```rust
pub struct MediaInfo {
    pub resolution: String,      // "1080p"
    pub width: u32,
    pub height: u32,
    pub codec: String,           // "hevc"
    pub hdr: String,             // "HDR10", "SDR", etc.
    pub aspect_ratio: String,    // "16:9"
    pub framerate: String,       // "23.976"
    pub bit_depth: String,       // "10"
    pub profile: String,         // "Main 10"
    pub audio: String,           // "truehd"
    pub channels: String,        // "5.1"
    pub audio_lang: String,      // "eng"
}
```

With `impl MediaInfo` providing `to_vars(&self) -> HashMap<&str, String>` for template substitution.

### Modified: `src/disc.rs`
- `probe_streams` is **left completely unchanged** — it continues to use text parsing for `StreamInfo` (audio stream lines + subtitle count). This preserves `build_map_args` in `rip.rs` which pattern-matches on those raw text lines for "5.1", "7.1", "surround", "stereo"
- New function: `probe_media_info(device: &str, playlist_num: &str) -> Option<MediaInfo>` — a **separate** ffprobe invocation using `-print_format json -show_streams` to extract structured video/audio metadata into `MediaInfo`
- The two functions may be called on the same playlist (one for stream mapping, one for filename metadata) — this is two lightweight ffprobe calls, not a performance concern for a disc ripping tool

### Modified: `src/util.rs`
- New function: `render_template(template: &str, vars: &HashMap<&str, String>) -> String`
  - Replaces `{key}` with values from the map
  - Runs bracket cleanup algorithm (see Edge Cases section)
  - Sanitizes per path component: strips characters unsafe for filesystems (`/`, `\`, `:`, `*`, `?`, `"`, `<`, `>`, `|`, null bytes) and `..` segments. **Spaces are preserved** (unlike the existing `sanitize_filename` which replaces spaces with underscores)
  - Strips leading/trailing whitespace per component
- Existing `sanitize_filename` is **unchanged** — the `default` preset continues to use it for the title component (producing underscored names matching current behavior). Note: `sanitize_filename` does not strip `\` or null bytes; adding those is a low-risk improvement to make as part of this change
- New function: `sanitize_path_component(s: &str) -> String` — like `sanitize_filename` but preserves spaces, for use by non-default presets
- Refactor `make_filename` and `make_movie_filename` to build a vars map and call `render_template` with the resolved format string
- Updated signatures to accept the format template and `Option<&MediaInfo>`

### Modified: `src/main.rs`
- Add `--format` and `--format-preset` to `Args` with `ArgGroup` for mutual exclusivity
- Load config via `config::load_config()` early in `main()`
- Pass `Config` to TUI and CLI runners

### Modified: `src/tmdb.rs`
- `get_api_key()` becomes a thin wrapper: delegates entirely to `config.tmdb_api_key()` which implements the full resolution chain (TOML → flat file → env var). The existing flat file and env var logic in `tmdb.rs` is removed since `Config::tmdb_api_key()` handles it
- `save_api_key()` unchanged (still writes to the flat file path)

### Modified: `src/tui/wizard.rs` and `src/cli.rs`
- Call `probe_media_info` for selected playlists (separate ffprobe call from `probe_streams`)
- Thread `MediaInfo` + resolved format string into filename generation
- Before writing output files, call `create_dir_all` on parent directory when path contains `/`

### Modified: `src/tui/dashboard.rs`
- Update `create_dir_all` call at rip initiation (currently only creates the output directory) to create the full parent path when the filename template contains subdirectories

### Modified: `src/tui/mod.rs`
- Add `config: Config` to `App` state

## Dependencies

Add `toml` crate for config file parsing. Already uses `serde` + `serde_json`, so the incremental cost is minimal.

## Testing

### New tests in `src/util.rs`
- `render_template` with all placeholder types
- `render_template` with empty values and bracket cleanup
- `render_template` with unknown placeholders (left as-is)
- `render_template` with subdirectory paths
- `render_template` with filesystem-unsafe characters in values

### New tests in `src/config.rs`
- TOML parsing with all fields
- TOML parsing with missing optional fields
- Format resolution order (CLI > config > default)
- TMDb API key fallback to flat file

### New tests in `src/disc.rs`
- `MediaInfo` parsing from sample ffprobe JSON output
- HDR detection logic for each variant (HDR10, DV, HLG, HDR10+, SDR)

### Existing tests
- `make_filename` and `make_movie_filename` tests updated to pass format template parameter
- All existing tests continue to pass (default preset matches current behavior)

## Backwards Compatibility

- No config file + no CLI flags = identical behavior to current version
- Existing `tmdb_api_key` flat file continues to work
- Default preset produces the same filenames as the current hardcoded format
- No breaking changes to existing CLI flags

## Notes

- Templates must include the file extension (e.g., `.mkv`). If omitted, the output file will have no extension. No automatic extension appending.
- The `plex` movie preset uses the literal `Movie` for the filename component (e.g., `Movie [Bluray-1080p]...`). This matches Plex's convention where the movie title lives in the directory name, not the filename.
