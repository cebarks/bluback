# MKV Metadata Embedding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Embed MKV metadata tags (title, show, season, episode, date, encoder) into output files during remux, with configurable custom tags.

**Architecture:** Add `MkvMetadata` struct to types, `build_metadata()` to workflow, thread metadata through `RemuxOptions`, and inject via `octx.set_metadata()` before header write. Config gets a `[metadata]` section; CLI gets `--no-metadata`.

**Tech Stack:** Rust, ffmpeg-the-third (Dictionary/set_metadata API), toml, clap

---

### Task 1: Add `MkvMetadata` struct and extend `RemuxOptions`

**Files:**
- Modify: `src/types.rs:1-2` (add struct)
- Modify: `src/media/remux.rs:25-34` (add field to RemuxOptions)

- [ ] **Step 1: Add `MkvMetadata` to `src/types.rs`**

Add after the `ChapterMark` struct (line 75):

```rust
/// Resolved MKV metadata tags ready to write to the output container.
#[derive(Debug, Clone, Default)]
pub struct MkvMetadata {
    pub tags: HashMap<String, String>,
}
```

- [ ] **Step 2: Add `metadata` field to `RemuxOptions` in `src/media/remux.rs`**

Add the field after `reserve_index_space_kb`:

```rust
pub struct RemuxOptions {
    pub device: String,
    pub playlist: String,
    pub output: PathBuf,
    pub chapters: Vec<ChapterMark>,
    pub stream_selection: StreamSelection,
    pub cancel: Arc<AtomicBool>,
    /// KB of void space to reserve after the MKV header for the seek index.
    pub reserve_index_space_kb: u32,
    /// MKV metadata tags to embed in the output file.
    pub metadata: Option<crate::types::MkvMetadata>,
}
```

- [ ] **Step 3: Fix compilation — update `prepare_remux_options` in `src/workflow.rs`**

Add `metadata: None` to the `RemuxOptions` struct literal at line 50:

```rust
    RemuxOptions {
        device: device.to_string(),
        playlist: playlist.num.clone(),
        output: output.to_path_buf(),
        chapters,
        stream_selection,
        cancel,
        reserve_index_space_kb,
        metadata: None,
    }
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished` with no errors

- [ ] **Step 5: Commit**

```
feat: add MkvMetadata struct and metadata field to RemuxOptions
```

---

### Task 2: Inject metadata in remux path

**Files:**
- Modify: `src/media/remux.rs:260-268` (inject metadata before header write)

- [ ] **Step 1: Add metadata injection between chapter injection and header write**

In `src/media/remux.rs`, after the `inject_chapters` call (line 261) and before the muxer_opts/write_header block (line 263), insert:

```rust
    // Inject MKV metadata tags before writing header
    if let Some(ref meta) = options.metadata {
        let mut dict = Dictionary::new();
        for (k, v) in &meta.tags {
            dict.set(k, v);
        }
        octx.set_metadata(dict);
        log::debug!("Injected {} metadata tag(s)", meta.tags.len());
    }
```

- [ ] **Step 2: Add per-stream metadata TODO comment**

In `src/media/remux.rs`, near the stream mapping loop (after line 247 `stream_map[in_idx] = out_idx;`), add:

```rust
        // TODO: Per-stream metadata titles (e.g. "English - DTS-HD MA 5.1")
        // to be implemented alongside per-stream track selection in v0.10
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished` with no errors

- [ ] **Step 4: Commit**

```
feat: inject MKV metadata tags during remux
```

---

### Task 3: Add `build_metadata()` to workflow

**Files:**
- Modify: `src/workflow.rs` (add function + tests)

- [ ] **Step 1: Write failing tests for `build_metadata`**

Add to the `#[cfg(test)] mod tests` block in `src/workflow.rs`:

```rust
    #[test]
    fn test_build_metadata_tv_full() {
        let meta = build_metadata(
            true,  // enabled
            false, // movie_mode
            Some("Game of Thrones"),
            Some(3),
            &[crate::types::Episode {
                episode_number: 9,
                name: "The Rains of Castamere".into(),
                runtime: None,
            }],
            None,   // movie_title
            None,   // movie_year
            Some("2013-06-02"),
            &HashMap::new(),
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "The Rains of Castamere");
        assert_eq!(meta.tags["SHOW"], "Game of Thrones");
        assert_eq!(meta.tags["SEASON_NUMBER"], "3");
        assert_eq!(meta.tags["EPISODE_SORT"], "9");
        assert_eq!(meta.tags["DATE_RELEASED"], "2013-06-02");
        assert!(meta.tags["ENCODER"].starts_with("bluback v"));
    }

    #[test]
    fn test_build_metadata_tv_multi_episode() {
        let meta = build_metadata(
            true,
            false,
            Some("Show"),
            Some(1),
            &[
                crate::types::Episode { episode_number: 3, name: "Ep Three".into(), runtime: None },
                crate::types::Episode { episode_number: 4, name: "Ep Four".into(), runtime: None },
            ],
            None, None, None,
            &HashMap::new(),
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Ep Three / Ep Four");
        assert_eq!(meta.tags["EPISODE_SORT"], "3");
    }

    #[test]
    fn test_build_metadata_movie() {
        let meta = build_metadata(
            true,
            true,
            None, None, &[],
            Some("Blade Runner 2049"),
            Some("2017"),
            Some("2017-10-06"),
            &HashMap::new(),
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Blade Runner 2049");
        assert_eq!(meta.tags["DATE_RELEASED"], "2017-10-06");
        assert!(meta.tags["ENCODER"].starts_with("bluback v"));
        assert!(!meta.tags.contains_key("SHOW"));
        assert!(!meta.tags.contains_key("SEASON_NUMBER"));
    }

    #[test]
    fn test_build_metadata_tmdb_skipped() {
        let meta = build_metadata(
            true, false,
            Some("Manual Title"),
            None, &[],
            None, None, None,
            &HashMap::new(),
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Manual Title");
        assert!(meta.tags["ENCODER"].starts_with("bluback v"));
        assert!(!meta.tags.contains_key("SHOW"));
        assert!(!meta.tags.contains_key("DATE_RELEASED"));
    }

    #[test]
    fn test_build_metadata_custom_tags() {
        let mut custom = HashMap::new();
        custom.insert("STUDIO".into(), "HBO".into());
        let meta = build_metadata(
            true, false,
            Some("Show"), Some(1),
            &[crate::types::Episode { episode_number: 1, name: "Pilot".into(), runtime: None }],
            None, None, None,
            &custom,
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["STUDIO"], "HBO");
        assert_eq!(meta.tags["TITLE"], "Pilot");
    }

    #[test]
    fn test_build_metadata_custom_overrides_auto() {
        let mut custom = HashMap::new();
        custom.insert("TITLE".into(), "Custom Title".into());
        let meta = build_metadata(
            true, false,
            Some("Show"), Some(1),
            &[crate::types::Episode { episode_number: 1, name: "Auto Title".into(), runtime: None }],
            None, None, None,
            &custom,
        );
        let meta = meta.unwrap();
        assert_eq!(meta.tags["TITLE"], "Custom Title");
    }

    #[test]
    fn test_build_metadata_disabled() {
        let meta = build_metadata(
            false, false,
            Some("Show"), Some(1),
            &[crate::types::Episode { episode_number: 1, name: "Pilot".into(), runtime: None }],
            None, None, None,
            &HashMap::new(),
        );
        assert!(meta.is_none());
    }

    #[test]
    fn test_build_metadata_no_empty_strings() {
        let meta = build_metadata(
            true, false,
            Some("Show"), Some(1),
            &[crate::types::Episode { episode_number: 1, name: String::new(), runtime: None }],
            None, None, None,
            &HashMap::new(),
        );
        let meta = meta.unwrap();
        // Empty episode name should use show name as TITLE, not empty string
        assert_eq!(meta.tags["TITLE"], "Show");
        for (k, v) in &meta.tags {
            assert!(!v.is_empty(), "Tag {} has empty value", k);
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib workflow::tests::test_build_metadata 2>&1 | tail -10`
Expected: compilation errors — `build_metadata` not found

- [ ] **Step 3: Implement `build_metadata`**

Add to `src/workflow.rs` before the `#[cfg(test)]` block:

```rust
/// Build MKV metadata tags from available context.
/// Returns `None` if metadata is disabled.
pub fn build_metadata(
    enabled: bool,
    movie_mode: bool,
    show_name: Option<&str>,
    season: Option<u32>,
    episodes: &[Episode],
    movie_title: Option<&str>,
    movie_year: Option<&str>,
    date_released: Option<&str>,
    custom_tags: &HashMap<String, String>,
) -> Option<crate::types::MkvMetadata> {
    if !enabled {
        return None;
    }

    let mut tags = HashMap::new();

    let encoder = format!("bluback v{}", env!("CARGO_PKG_VERSION"));
    tags.insert("ENCODER".into(), encoder);

    if movie_mode {
        if let Some(title) = movie_title {
            if !title.is_empty() {
                tags.insert("TITLE".into(), title.to_string());
            }
        }
    } else {
        // TV mode: episode name(s) as TITLE, fall back to show name
        let title = if episodes.len() > 1 {
            let names: Vec<&str> = episodes
                .iter()
                .map(|e| e.name.as_str())
                .filter(|n| !n.is_empty())
                .collect();
            if names.is_empty() { None } else { Some(names.join(" / ")) }
        } else if let Some(ep) = episodes.first() {
            if ep.name.is_empty() { None } else { Some(ep.name.clone()) }
        } else {
            None
        };

        let title = title.or_else(|| show_name.filter(|s| !s.is_empty()).map(String::from));
        if let Some(t) = title {
            tags.insert("TITLE".into(), t);
        }

        if let Some(name) = show_name {
            if !name.is_empty() {
                tags.insert("SHOW".into(), name.to_string());
            }
        }
        if let Some(s) = season {
            tags.insert("SEASON_NUMBER".into(), s.to_string());
        }
        if let Some(ep) = episodes.first() {
            tags.insert("EPISODE_SORT".into(), ep.episode_number.to_string());
        }
    }

    if let Some(date) = date_released {
        if !date.is_empty() {
            tags.insert("DATE_RELEASED".into(), date.to_string());
        }
    }

    // Custom tags override auto-generated ones
    for (k, v) in custom_tags {
        if !v.is_empty() {
            tags.insert(k.clone(), v.clone());
        }
    }

    Some(crate::types::MkvMetadata { tags })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib workflow::tests::test_build_metadata 2>&1 | tail -15`
Expected: all 7 tests pass

- [ ] **Step 5: Commit**

```
feat: add build_metadata() for MKV tag generation
```

---

### Task 4: Add `[metadata]` config section

**Files:**
- Modify: `src/config.rs` (Config struct, KNOWN_KEYS, parsing, serialization, validation, tests)

- [ ] **Step 1: Write failing config tests**

Add to `#[cfg(test)] mod tests` in `src/config.rs`:

```rust
    #[test]
    fn test_parse_metadata_section() {
        let toml_str = r#"
            [metadata]
            enabled = false
            tags = { STUDIO = "HBO", COLLECTION = "My Blu-rays" }
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let meta = config.metadata.unwrap();
        assert_eq!(meta.enabled, Some(false));
        assert_eq!(meta.tags.as_ref().unwrap()["STUDIO"], "HBO");
        assert_eq!(meta.tags.as_ref().unwrap()["COLLECTION"], "My Blu-rays");
    }

    #[test]
    fn test_parse_missing_metadata_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.metadata.is_none());
        // Accessor should still return defaults
        assert!(config.metadata_enabled());
        assert!(config.metadata_tags().is_empty());
    }

    #[test]
    fn test_metadata_config_roundtrip() {
        let toml_str = r#"
            [metadata]
            enabled = false
            tags = { STUDIO = "HBO" }
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let output = config.to_toml_string();
        assert!(output.contains("[metadata]"));
        assert!(output.contains("enabled = false"));
        let reparsed: Config = toml::from_str(&output).unwrap();
        let meta = reparsed.metadata.unwrap();
        assert_eq!(meta.enabled, Some(false));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::test_parse_metadata 2>&1 | tail -10`
Expected: compilation error — no `metadata` field on Config

- [ ] **Step 3: Add `MetadataConfig` struct and field to `Config`**

Add after the `AacsBackend` enum in `src/config.rs`:

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MetadataConfig {
    pub enabled: Option<bool>,
    pub tags: Option<HashMap<String, String>>,
}
```

Add `use std::collections::HashMap;` to the imports at the top of `src/config.rs`.

Add the field to `Config`:

```rust
pub struct Config {
    // ... existing fields ...
    pub log_dir: Option<String>,
    pub max_log_files: Option<u32>,
    pub metadata: Option<MetadataConfig>,
}
```

- [ ] **Step 4: Add accessor methods to `Config`**

Add to the `impl Config` block:

```rust
    pub fn metadata_enabled(&self) -> bool {
        self.metadata
            .as_ref()
            .and_then(|m| m.enabled)
            .unwrap_or(true)
    }

    pub fn metadata_tags(&self) -> HashMap<String, String> {
        self.metadata
            .as_ref()
            .and_then(|m| m.tags.clone())
            .unwrap_or_default()
    }
```

- [ ] **Step 5: Add `metadata` to `KNOWN_KEYS`**

Add to the `KNOWN_KEYS` array:

```rust
    "metadata",
    "metadata.enabled",
    "metadata.tags",
```

- [ ] **Step 6: Update `validate_raw_toml` to handle table keys**

The existing `validate_raw_toml` only checks top-level keys. TOML tables like `[metadata]` parse as a top-level key `"metadata"` with a `toml::Value::Table` value. Since `"metadata"` is already in `KNOWN_KEYS`, the top-level check passes. Sub-keys within the table (like `enabled` and `tags`) are nested and won't trigger false "unknown key" warnings. No changes needed to validation logic for this — the existing approach handles it correctly.

- [ ] **Step 7: Update `to_toml_string` for metadata serialization**

Add before the final `tmdb_api_key` emission in `to_toml_string`:

```rust
        out.push('\n');
        out.push_str("[metadata]\n");
        let meta_enabled = self.metadata.as_ref().and_then(|m| m.enabled);
        emit_bool(&mut out, "enabled", meta_enabled, true);
        if let Some(ref meta) = self.metadata {
            if let Some(ref tags) = meta.tags {
                if !tags.is_empty() {
                    let pairs: Vec<String> = tags
                        .iter()
                        .map(|(k, v)| format!("{} = {:?}", k, v))
                        .collect();
                    out.push_str(&format!("tags = {{ {} }}\n", pairs.join(", ")));
                }
            }
        }
        if self.metadata.as_ref().and_then(|m| m.tags.as_ref()).map_or(true, |t| t.is_empty()) {
            out.push_str("# tags = { }\n");
        }
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test --lib config::tests::test_parse_metadata config::tests::test_metadata_config 2>&1 | tail -15`
Expected: all 3 new tests pass

- [ ] **Step 9: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass (existing + new)

- [ ] **Step 10: Commit**

```
feat: add [metadata] config section with enabled toggle and custom tags
```

---

### Task 5: Add `--no-metadata` CLI flag

**Files:**
- Modify: `src/main.rs` (add arg)

- [ ] **Step 1: Add `--no-metadata` to the `Args` struct**

In `src/main.rs`, add after the `--no-log` flag (around line 139):

```rust
    /// Don't embed metadata tags in output MKV files
    #[arg(long)]
    no_metadata: bool,
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished` with no errors

- [ ] **Step 3: Verify the flag is recognized**

Run: `cargo run -- --help 2>&1 | grep -A1 no-metadata`
Expected: shows `--no-metadata` with description

- [ ] **Step 4: Commit**

```
feat: add --no-metadata CLI flag
```

---

### Task 6: Thread metadata through CLI rip path

**Files:**
- Modify: `src/cli.rs` (build metadata, pass to prepare_remux_options)
- Modify: `src/workflow.rs` (add metadata param to prepare_remux_options)

- [ ] **Step 1: Add `metadata` parameter to `prepare_remux_options`**

Update the function signature and struct literal in `src/workflow.rs`:

```rust
pub fn prepare_remux_options(
    device: &str,
    playlist: &Playlist,
    output: &Path,
    mount_point: Option<&str>,
    stream_selection: StreamSelection,
    cancel: Arc<AtomicBool>,
    reserve_index_space_kb: u32,
    metadata: Option<crate::types::MkvMetadata>,
) -> RemuxOptions {
    let chapters = mount_point
        .and_then(|mount| {
            crate::chapters::extract_chapters(std::path::Path::new(mount), &playlist.num)
        })
        .unwrap_or_default();

    RemuxOptions {
        device: device.to_string(),
        playlist: playlist.num.clone(),
        output: output.to_path_buf(),
        chapters,
        stream_selection,
        cancel,
        reserve_index_space_kb,
        metadata,
    }
}
```

- [ ] **Step 2: Fix existing `prepare_remux_options` tests**

Update the two test calls in `src/workflow.rs` to pass `None` as the last argument:

In `test_prepare_remux_options_no_mount`:
```rust
        let opts = prepare_remux_options(
            "/dev/sr0",
            &playlist,
            Path::new("/tmp/out.mkv"),
            None,
            StreamSelection::All,
            cancel,
            500,
            None,
        );
```

In `test_prepare_remux_options_bad_mount_swallows_error`:
```rust
        let opts = prepare_remux_options(
            "/dev/sr0",
            &playlist,
            Path::new("/tmp/out.mkv"),
            Some("/nonexistent/mount"),
            StreamSelection::All,
            cancel,
            500,
            None,
        );
```

- [ ] **Step 3: Update CLI `rip_selected` to accept and pass metadata**

Change the `rip_selected` function signature in `src/cli.rs` (line 880) to accept metadata parameters:

```rust
fn rip_selected(
    args: &Args,
    config: &crate::config::Config,
    device: &str,
    episodes_pl: &[Playlist],
    selected: &[usize],
    outfiles: &[PathBuf],
    metadata_per_playlist: &[Option<crate::types::MkvMetadata>],
) -> anyhow::Result<()> {
```

Then update the `prepare_remux_options` call inside (around line 962) to pass the metadata:

```rust
        let options = crate::workflow::prepare_remux_options(
            device,
            pl,
            outfile,
            mount_point.as_deref(),
            stream_selection.clone(),
            cancel,
            config.reserve_index_space(),
            metadata_per_playlist[i].clone(),
        );
```

- [ ] **Step 4: Build metadata in CLI `run_interactive` and pass to `rip_selected`**

At the call site in `src/cli.rs` where `rip_selected` is called (around line 321), build the per-playlist metadata before the call. This is where `tmdb_ctx`, `movie_mode`, `label`, `config`, and `args` are all in scope.

Find the `rip_selected` call and add metadata building before it:

```rust
    let metadata_enabled = config.metadata_enabled() && !args.no_metadata;
    let custom_tags = config.metadata_tags();
    let metadata_per_playlist: Vec<Option<crate::types::MkvMetadata>> = selected
        .iter()
        .map(|&idx| {
            let pl = &episodes_pl[idx];
            let episodes = tmdb_ctx
                .episode_assignments
                .get(&pl.num)
                .cloned()
                .unwrap_or_default();
            let date = if movie_mode {
                tmdb_ctx.movie_title.as_ref().and_then(|(_t, y)| {
                    // Try to get full date from selected movie
                    if y.is_empty() { None } else { Some(y.as_str()) }
                })
            } else {
                tmdb_ctx.show_name.as_ref().and_then(|_| {
                    // first_air_date not stored in TmdbContext — leave None
                    None
                })
            };
            crate::workflow::build_metadata(
                metadata_enabled,
                movie_mode,
                tmdb_ctx.show_name.as_deref(),
                tmdb_ctx.season_num,
                &episodes,
                tmdb_ctx.movie_title.as_ref().map(|(t, _)| t.as_str()),
                tmdb_ctx.movie_title.as_ref().map(|(_, y)| y.as_str()),
                date,
                &custom_tags,
            )
        })
        .collect();

    rip_selected(args, config, &device, &episodes_pl, &selected, &outfiles, &metadata_per_playlist)
```

- [ ] **Step 5: Verify it compiles and tests pass**

Run: `cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -5`
Expected: builds and all tests pass

- [ ] **Step 6: Commit**

```
feat: thread MKV metadata through CLI rip path
```

---

### Task 7: Thread metadata through TUI rip path

**Files:**
- Modify: `src/tui/dashboard.rs:420-428` (pass metadata to prepare_remux_options)

- [ ] **Step 1: Build metadata and pass it in `start_next_job_session`**

In `src/tui/dashboard.rs`, in the `start_next_job_session` function (around line 420), before the `prepare_remux_options` call, build the metadata from the session context:

```rust
    let metadata = {
        let metadata_enabled = session.config.metadata_enabled() && !session.no_metadata;
        let custom_tags = session.config.metadata_tags();
        let episodes = &session.rip.jobs[idx].episode;
        let date = if session.tmdb.movie_mode {
            session.tmdb.movie_results
                .get(session.tmdb.selected_movie.unwrap_or(0))
                .and_then(|m| m.release_date.as_deref())
        } else {
            session.tmdb.search_results
                .get(session.tmdb.selected_show.unwrap_or(0))
                .and_then(|s| s.first_air_date.as_deref())
        };
        let movie_title = if session.tmdb.movie_mode {
            session.tmdb.movie_results
                .get(session.tmdb.selected_movie.unwrap_or(0))
                .map(|m| m.title.as_str())
        } else {
            None
        };
        let movie_year = if session.tmdb.movie_mode {
            session.tmdb.movie_results
                .get(session.tmdb.selected_movie.unwrap_or(0))
                .and_then(|m| m.release_date.as_deref())
                .map(|d| if d.len() >= 4 { &d[..4] } else { d })
        } else {
            None
        };
        crate::workflow::build_metadata(
            metadata_enabled,
            session.tmdb.movie_mode,
            Some(&session.tmdb.show_name).filter(|s| !s.is_empty()),
            session.wizard.season_num,
            episodes,
            movie_title,
            movie_year,
            date,
            &custom_tags,
        )
    };

    let options = crate::workflow::prepare_remux_options(
        &device,
        &job_playlist,
        &outfile,
        session.disc.mount_point.as_deref(),
        stream_selection,
        cancel,
        session.config.reserve_index_space(),
        metadata,
    );
```

- [ ] **Step 2: Add `no_metadata` field to `DriveSession`**

In `src/session.rs`, add the field to `DriveSession`:

```rust
    pub overwrite: bool,
    pub no_metadata: bool,
}
```

And in the `DriveSession::new()` constructor, initialize it from the args:

```rust
    no_metadata: args.no_metadata,
```

Find the `new()` function and add the field initialization alongside the other CLI args.

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -5`
Expected: builds and all tests pass

- [ ] **Step 4: Commit**

```
feat: thread MKV metadata through TUI rip path
```

---

### Task 8: Add metadata toggle to settings panel

**Files:**
- Modify: `src/types.rs` (add metadata toggle to SettingsState)

- [ ] **Step 1: Add metadata toggle to `from_config_with_drives`**

In `src/types.rs`, in the `from_config_with_drives` method, add a Metadata separator and toggle after the Logging section items (after the log_level Choice item, around line 643):

```rust
            SettingItem::Separator {
                label: Some("Metadata".into()),
            },
            SettingItem::Toggle {
                label: "Embed Metadata Tags".into(),
                key: "metadata.enabled".into(),
                value: config.metadata_enabled(),
            },
```

- [ ] **Step 2: Update `to_config` to handle the metadata toggle**

In the `to_config` method, add a match arm for the metadata toggle:

```rust
                SettingItem::Toggle { key, value, .. } => match key.as_str() {
                    // ... existing arms ...
                    "metadata.enabled" if !*value => {
                        let meta = config.metadata.get_or_insert_with(Default::default);
                        meta.enabled = Some(false);
                    }
                    _ => {}
                },
```

- [ ] **Step 3: Add env var override for metadata**

Add to the `ENV_MAPPINGS` arrays in both `apply_env_overrides` and `active_env_var_warnings`:

```rust
            ("BLUBACK_METADATA", "metadata.enabled"),
```

- [ ] **Step 4: Update the settings item count test**

In `test_settings_state_from_config_item_count`, the count will increase. Update the assertion to reflect the new separator + toggle (2 more items, but only the toggle is a non-separator):

```rust
        assert_eq!(non_separator_count, 19); // 18 settings + 1 action
```

- [ ] **Step 5: Verify it compiles and tests pass**

Run: `cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -5`
Expected: builds and all tests pass

- [ ] **Step 6: Commit**

```
feat: add metadata toggle to settings panel
```

---

### Task 9: Add config tests for metadata section

**Files:**
- Modify: `src/config.rs` (tests only)

These tests were written in Task 4, Step 1. Verify they all pass along with the existing suite.

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1 | tail -10`
Expected: no warnings

- [ ] **Step 3: Commit (if any clippy fixes needed)**

```
chore: clippy fixes for metadata embedding
```

---

### Task 10: Update CLAUDE.md and TODO.md

**Files:**
- Modify: `CLAUDE.md`
- Modify: `TODO.md`

- [ ] **Step 1: Update CLAUDE.md**

Add `--no-metadata` to the CLI flags table. Add metadata to the "Key Design Decisions" section.

In the CLI flags table, add:

```
      --no-metadata            Don't embed metadata tags in output MKV files
```

In the Key Design Decisions section, add:

```
- **MKV metadata embedding** — Auto-generated tags (TITLE, SHOW, SEASON_NUMBER, EPISODE_SORT, DATE_RELEASED, ENCODER) are embedded during remux via `octx.set_metadata()`. Configurable via `[metadata]` config section (`enabled` bool, `tags` table for custom key-value pairs). Disabled per-run with `--no-metadata`. Custom tags override auto-generated ones on conflict. Empty values are never written.
```

Add `metadata.enabled` and `metadata.tags` references where relevant in the Config section or env var list.

- [ ] **Step 2: Update TODO.md**

Add to the v0.10 section, marking MKV metadata as complete:

Under "Upcoming Milestones", update the v0.10 line to move MKV metadata to a completed sub-item, e.g.:

```markdown
## In Progress: v0.10 — Quality of Life & Automation

- [x] Log files
- [x] MKV metadata embedding (TITLE, SHOW, SEASON_NUMBER, EPISODE_SORT, DATE_RELEASED, ENCODER + custom tags)
- [ ] Pause/resume during ripping
- [ ] Per-stream track titles (TODO: alongside per-stream track selection)
- [ ] Post-rip hooks
- [ ] Rip verification
- [ ] Per-stream track selection
- [ ] Continuous batch mode
- [ ] Disc history / rip database
```

- [ ] **Step 3: Commit**

```
docs: update CLAUDE.md and TODO.md for MKV metadata embedding
```
