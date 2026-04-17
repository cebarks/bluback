# AVIO Buffer Tuning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace FFmpeg's default 32 KB AVIO buffers with configurable sizes (2 MB read, 1 MB write) to improve Blu-ray remux throughput through USB drives.

**Architecture:** Bypass `ffmpeg-the-third`'s combined open+probe convenience functions by calling FFI functions directly, inserting a buffer resize step between open and probe/write. A `resize_avio_buffer` helper reimplements FFmpeg's internal `ffio_set_buf_size`. Two new config fields (`read_buffer_kb`, `write_buffer_kb`) control sizes.

**Tech Stack:** Rust, `ffmpeg-the-third` (v4.1.0) / `ffmpeg-sys-the-third` FFI bindings, TOML config

**Spec:** `docs/superpowers/specs/2026-04-16-avio-buffer-tuning-design.md`

---

### Task 1: Add config fields, defaults, accessors, and validation

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write failing tests for config parsing and defaults**

Add these tests to the existing `#[cfg(test)] mod tests` block at the bottom of `src/config.rs`:

```rust
#[test]
fn test_read_buffer_kb_default() {
    let config: Config = toml::from_str("").unwrap();
    assert_eq!(config.read_buffer_kb(), DEFAULT_READ_BUFFER_KB);
}

#[test]
fn test_write_buffer_kb_default() {
    let config: Config = toml::from_str("").unwrap();
    assert_eq!(config.write_buffer_kb(), DEFAULT_WRITE_BUFFER_KB);
}

#[test]
fn test_parse_buffer_sizes() {
    let config: Config = toml::from_str("read_buffer_kb = 4096\nwrite_buffer_kb = 512").unwrap();
    assert_eq!(config.read_buffer_kb(), 4096);
    assert_eq!(config.write_buffer_kb(), 512);
}

#[test]
fn test_validate_buffer_too_small_warns() {
    let config = Config {
        read_buffer_kb: Some(16),
        write_buffer_kb: Some(8),
        ..Config::default()
    };
    let warnings = validate_config(&config);
    assert!(warnings.iter().any(|w| w.contains("read_buffer_kb")));
    assert!(warnings.iter().any(|w| w.contains("write_buffer_kb")));
}

#[test]
fn test_validate_buffer_too_large_warns() {
    let config = Config {
        read_buffer_kb: Some(8192),
        write_buffer_kb: Some(5000),
        ..Config::default()
    };
    let warnings = validate_config(&config);
    assert!(warnings.iter().any(|w| w.contains("read_buffer_kb")));
    assert!(warnings.iter().any(|w| w.contains("write_buffer_kb")));
}

#[test]
fn test_validate_buffer_at_bounds_no_warnings() {
    let config = Config {
        read_buffer_kb: Some(32),
        write_buffer_kb: Some(4096),
        ..Config::default()
    };
    let warnings = validate_config(&config);
    assert!(!warnings.iter().any(|w| w.contains("buffer")));
}

#[test]
fn test_buffer_known_keys() {
    assert!(KNOWN_KEYS.contains(&"read_buffer_kb"));
    assert!(KNOWN_KEYS.contains(&"write_buffer_kb"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::test_read_buffer_kb_default config::tests::test_write_buffer_kb_default config::tests::test_parse_buffer_sizes config::tests::test_validate_buffer_too_small_warns config::tests::test_validate_buffer_too_large_warns config::tests::test_validate_buffer_at_bounds_no_warnings config::tests::test_buffer_known_keys 2>&1 | tail -20`
Expected: compilation errors — `DEFAULT_READ_BUFFER_KB`, `read_buffer_kb` field, and `read_buffer_kb()` method don't exist yet.

- [ ] **Step 3: Add constants, fields, accessors, KNOWN_KEYS, with_comments, and validation**

In `src/config.rs`:

1. Add constants after `DEFAULT_RESERVE_INDEX_SPACE`:

```rust
pub const DEFAULT_READ_BUFFER_KB: u32 = 2048;
pub const DEFAULT_WRITE_BUFFER_KB: u32 = 1024;
```

2. Add fields to `Config` struct after `reserve_index_space`:

```rust
pub read_buffer_kb: Option<u32>,
pub write_buffer_kb: Option<u32>,
```

3. Add accessor methods in the `impl Config` block (near `reserve_index_space()`):

```rust
pub fn read_buffer_kb(&self) -> u32 {
    self.read_buffer_kb.unwrap_or(DEFAULT_READ_BUFFER_KB)
}

pub fn write_buffer_kb(&self) -> u32 {
    self.write_buffer_kb.unwrap_or(DEFAULT_WRITE_BUFFER_KB)
}
```

4. Add to `KNOWN_KEYS` array after `"reserve_index_space"`:

```rust
"read_buffer_kb",
"write_buffer_kb",
```

5. Add to `with_comments()` method, after the `reserve_index_space` `emit_u32` call:

```rust
emit_u32(
    &mut out,
    "read_buffer_kb",
    self.read_buffer_kb,
    DEFAULT_READ_BUFFER_KB,
);
emit_u32(
    &mut out,
    "write_buffer_kb",
    self.write_buffer_kb,
    DEFAULT_WRITE_BUFFER_KB,
);
```

6. Add validation in `validate_config()`, after the `reserve_index_space` check:

```rust
if let Some(r) = config.read_buffer_kb {
    if r < 32 {
        warnings.push(format!(
            "read_buffer_kb = {} KB is below FFmpeg's default (32 KB)",
            r
        ));
    } else if r > 4096 {
        warnings.push(format!(
            "read_buffer_kb = {} KB exceeds recommended maximum (4096 KB)",
            r
        ));
    }
}
if let Some(w) = config.write_buffer_kb {
    if w < 32 {
        warnings.push(format!(
            "write_buffer_kb = {} KB is below FFmpeg's default (32 KB)",
            w
        ));
    } else if w > 4096 {
        warnings.push(format!(
            "write_buffer_kb = {} KB exceeds recommended maximum (4096 KB)",
            w
        ));
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config::tests 2>&1 | tail -5`
Expected: all config tests pass.

- [ ] **Step 5: Verify with_comments roundtrip test still passes**

Run: `cargo test --lib config::tests::test_with_comments_roundtrip 2>&1 | tail -5`
Expected: PASS (the roundtrip test should handle the new commented-out defaults).

- [ ] **Step 6: Commit**

```bash
git add src/config.rs
git commit -m "feat: add read_buffer_kb and write_buffer_kb config options

Configurable AVIO buffer sizes for FFmpeg input (disc read) and output
(file write) contexts. Defaults: 2 MB read, 1 MB write."
```

---

### Task 2: Add buffer fields to RemuxOptions and wire through callers

**Files:**
- Modify: `src/media/remux.rs` (struct only)
- Modify: `src/workflow.rs`
- Modify: `src/cli.rs`
- Modify: `src/tui/dashboard.rs`

- [ ] **Step 1: Add fields to RemuxOptions**

In `src/media/remux.rs`, add two fields to the `RemuxOptions` struct after `metadata`:

```rust
/// Size in KB of the AVIO read buffer for the input (disc) context.
pub read_buffer_kb: u32,
/// Size in KB of the AVIO write buffer for the output (file) context.
pub write_buffer_kb: u32,
```

- [ ] **Step 2: Update prepare_remux_options to accept and pass through buffer sizes**

In `src/workflow.rs`, add `read_buffer_kb: u32` and `write_buffer_kb: u32` parameters to `prepare_remux_options()` and include them in the returned `RemuxOptions`:

Change the function signature to:

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
    read_buffer_kb: u32,
    write_buffer_kb: u32,
) -> RemuxOptions {
```

And add to the returned struct:

```rust
    RemuxOptions {
        device: device.to_string(),
        playlist: playlist.num.clone(),
        output: output.to_path_buf(),
        chapters,
        stream_selection,
        cancel,
        reserve_index_space_kb,
        metadata,
        read_buffer_kb,
        write_buffer_kb,
    }
```

- [ ] **Step 3: Update CLI caller**

In `src/cli.rs` at the `prepare_remux_options` call site (around line 1714), add the two new arguments:

```rust
        let options = crate::workflow::prepare_remux_options(
            device,
            pl,
            outfile,
            mount_point.as_deref(),
            stream_selection,
            cancel,
            config.reserve_index_space(),
            metadata_per_playlist[i].clone(),
            config.read_buffer_kb(),
            config.write_buffer_kb(),
        );
```

- [ ] **Step 4: Update TUI dashboard caller**

In `src/tui/dashboard.rs` at the `prepare_remux_options` call site (around line 871), add the two new arguments:

```rust
    let options = crate::workflow::prepare_remux_options(
        &device,
        &job_playlist,
        &outfile,
        session.disc.mount_point.as_deref(),
        stream_selection,
        cancel,
        session.config.reserve_index_space(),
        metadata,
        session.config.read_buffer_kb(),
        session.config.write_buffer_kb(),
    );
```

- [ ] **Step 5: Update workflow test call sites**

In `src/workflow.rs`, update the two `prepare_remux_options` calls in the test module (around lines 364 and 392) to add the new arguments. Add after the `metadata` argument:

```rust
            crate::config::DEFAULT_READ_BUFFER_KB,
            crate::config::DEFAULT_WRITE_BUFFER_KB,
```

- [ ] **Step 6: Verify compilation**

Run: `cargo build 2>&1 | tail -10`
Expected: compiles without errors. The new fields are passed through but not yet used by `remux()`.

- [ ] **Step 7: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/media/remux.rs src/workflow.rs src/cli.rs src/tui/dashboard.rs
git commit -m "feat: wire read_buffer_kb/write_buffer_kb through RemuxOptions

New fields flow from config -> prepare_remux_options -> RemuxOptions.
Not yet consumed by remux(); wiring only."
```

---

### Task 3: Implement resize_avio_buffer helper

**Files:**
- Modify: `src/media/remux.rs`

- [ ] **Step 1: Add the resize_avio_buffer function**

Add this function in `src/media/remux.rs` after the `inject_chapters` function and before the `remux` function. This reimplements FFmpeg's internal `ffio_set_buf_size` (not part of the public API):

```rust
/// Resize the AVIO buffer on an already-opened format context.
///
/// Reimplements FFmpeg's internal `ffio_set_buf_size` (not part of the public API).
/// Allocation is atomic: the new buffer is allocated before freeing the old one.
/// If allocation fails, the original buffer is left intact.
///
/// # Safety
/// `pb` must be a valid, non-null `AVIOContext` pointer from an open format context.
unsafe fn resize_avio_buffer(
    pb: *mut ffmpeg::ffi::AVIOContext,
    new_size: usize,
    is_write: bool,
) -> Result<(), MediaError> {
    let new_buf = ffmpeg::ffi::av_malloc(new_size) as *mut u8;
    if new_buf.is_null() {
        return Err(MediaError::RemuxFailed(format!(
            "Failed to allocate {} KB AVIO buffer",
            new_size / 1024
        )));
    }

    ffmpeg::ffi::av_free((*pb).buffer as *mut libc::c_void);

    (*pb).buffer = new_buf;
    (*pb).buffer_size = new_size as libc::c_int;
    (*pb).buf_ptr = new_buf;
    (*pb).buf_ptr_max = new_buf;

    if is_write {
        (*pb).buf_end = new_buf.add(new_size);
    } else {
        (*pb).buf_end = new_buf;
    }

    Ok(())
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles. Function is defined but not yet called (allow dead_code warning is fine for now).

- [ ] **Step 3: Commit**

```bash
git add src/media/remux.rs
git commit -m "feat: add resize_avio_buffer FFI helper

Reimplements FFmpeg's internal ffio_set_buf_size with atomic
allocate-before-free semantics."
```

---

### Task 4: Implement open_input with buffer resize

**Files:**
- Modify: `src/media/remux.rs`

- [ ] **Step 1: Add necessary FFI imports**

At the top of `src/media/remux.rs`, add `use std::ffi::CString;` and `use std::ptr;` to the existing imports if not already present.

- [ ] **Step 2: Add the open_input function**

Add this function after `resize_avio_buffer` and before `remux`:

```rust
/// Open a bluray input with a custom AVIO read buffer size.
///
/// Decomposes `ffmpeg-the-third`'s `format::input_with_dictionary` into separate
/// open + probe steps, inserting a buffer resize between them.
fn open_input(
    url: &str,
    playlist: &str,
    read_buffer_kb: u32,
    mkv_guard: &mut crate::aacs::MakemkvconGuard,
) -> Result<format::context::Input, MediaError> {
    super::ensure_init();

    let c_url = CString::new(url).map_err(|_| {
        MediaError::RemuxFailed("Input URL contains null byte".into())
    })?;

    let mut opts = Dictionary::new();
    opts.set("playlist", playlist);
    let mut raw_opts = opts.disown();

    unsafe {
        let mut ps = ffmpeg::ffi::avformat_alloc_context();
        if ps.is_null() {
            Dictionary::own(raw_opts);
            return Err(MediaError::RemuxFailed(
                "Failed to allocate format context".into(),
            ));
        }

        // Open input inside MakemkvconGuard to track any makemkvcon processes spawned.
        let open_result = mkv_guard.track_open(|| {
            ffmpeg::ffi::avformat_open_input(
                &mut ps,
                c_url.as_ptr(),
                ptr::null(),
                &mut raw_opts,
            )
        });

        Dictionary::own(raw_opts);

        if open_result < 0 {
            // avformat_open_input frees ps on failure — do not double-free.
            let err = ffmpeg::Error::from(open_result);
            if let Some(aacs_err) = classify_aacs_error(&err) {
                return Err(aacs_err);
            }
            return Err(MediaError::Ffmpeg(err));
        }

        // Resize AVIO read buffer before probing.
        let buf_size = (read_buffer_kb as usize) * 1024;
        let pb = (*ps).pb;
        if !pb.is_null() && buf_size != (*pb).buffer_size as usize {
            if let Err(e) = resize_avio_buffer(pb, buf_size, false) {
                ffmpeg::ffi::avformat_close_input(&mut ps);
                return Err(e);
            }
        }

        // Probe streams.
        let probe_result = ffmpeg::ffi::avformat_find_stream_info(ps, ptr::null_mut());
        if probe_result < 0 {
            ffmpeg::ffi::avformat_close_input(&mut ps);
            return Err(MediaError::Ffmpeg(ffmpeg::Error::from(probe_result)));
        }

        // Post-probe re-verify: avformat_find_stream_info can seek internally,
        // which may trigger FFmpeg's internal buffer resize via orig_buffer_size
        // (not accessible from public API), reverting to the default 32 KB.
        let pb = (*ps).pb;
        if !pb.is_null() && ((*pb).buffer_size as usize) < buf_size {
            log::debug!(
                "AVIO buffer reverted to {} KB after probe, resizing to {} KB",
                (*pb).buffer_size / 1024,
                read_buffer_kb
            );
            if let Err(e) = resize_avio_buffer(pb, buf_size, false) {
                ffmpeg::ffi::avformat_close_input(&mut ps);
                return Err(e);
            }
        }

        log::info!(
            "Input AVIO buffer: {} KB",
            read_buffer_kb
        );

        Ok(format::context::Input::wrap(ps))
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add src/media/remux.rs
git commit -m "feat: add open_input with custom AVIO buffer size

Decomposes format::input_with_dictionary into open + resize + probe
to insert a buffer resize at a clean state boundary."
```

---

### Task 5: Implement open_output with buffer resize

**Files:**
- Modify: `src/media/remux.rs`

- [ ] **Step 1: Add the open_output function**

Add this function after `open_input` and before `remux`:

```rust
/// Open an output file with a custom AVIO write buffer size.
///
/// Decomposes `ffmpeg-the-third`'s `format::output` into separate
/// context alloc + avio_open steps, inserting a buffer resize before any writes.
fn open_output(path: &str, write_buffer_kb: u32) -> Result<format::context::Output, MediaError> {
    let c_path = CString::new(path).map_err(|_| {
        MediaError::RemuxFailed("Output path contains null byte".into())
    })?;

    unsafe {
        let mut ps = ptr::null_mut();
        let alloc_result = ffmpeg::ffi::avformat_alloc_output_context2(
            &mut ps,
            ptr::null(),
            ptr::null(),
            c_path.as_ptr(),
        );
        if alloc_result < 0 || ps.is_null() {
            return Err(MediaError::Ffmpeg(ffmpeg::Error::from(alloc_result)));
        }

        let avio_result = ffmpeg::ffi::avio_open(
            &mut (*ps).pb,
            c_path.as_ptr(),
            ffmpeg::ffi::AVIO_FLAG_WRITE,
        );
        if avio_result < 0 {
            ffmpeg::ffi::avformat_free_context(ps);
            return Err(MediaError::Ffmpeg(ffmpeg::Error::from(avio_result)));
        }

        // Resize AVIO write buffer before any writes.
        let buf_size = (write_buffer_kb as usize) * 1024;
        let pb = (*ps).pb;
        if !pb.is_null() && buf_size != (*pb).buffer_size as usize {
            if let Err(e) = resize_avio_buffer(pb, buf_size, true) {
                ffmpeg::ffi::avio_close((*ps).pb);
                ffmpeg::ffi::avformat_free_context(ps);
                return Err(e);
            }
        }

        log::info!("Output AVIO buffer: {} KB", write_buffer_kb);

        Ok(format::context::Output::wrap(ps))
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/media/remux.rs
git commit -m "feat: add open_output with custom AVIO buffer size

Decomposes format::output into alloc + avio_open + resize to insert
a write buffer resize before any data is written."
```

---

### Task 6: Wire open_input and open_output into remux()

**Files:**
- Modify: `src/media/remux.rs`

- [ ] **Step 1: Replace the input open in remux()**

In the `remux()` function, replace lines 152-167 (the current input open block):

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

With:

```rust
    // Open input with custom AVIO read buffer size.
    // MakemkvconGuard tracks makemkvcon processes spawned during the open.
    let mut _mkv_guard = crate::aacs::MakemkvconGuard::new();
    let input_url = format!("bluray:{}", options.device);

    let mut ictx = open_input(
        &input_url,
        &options.playlist,
        options.read_buffer_kb,
        &mut _mkv_guard,
    )?;
```

- [ ] **Step 2: Replace the output open in remux()**

Replace line 185 (the current output open):

```rust
    let mut octx = format::output(output_path)?;
```

With:

```rust
    let mut octx = open_output(output_path, options.write_buffer_kb)?;
```

- [ ] **Step 3: Clean up unused imports**

The `format::input_with_dictionary` and `format::output` imports are no longer directly called in `remux()`. Check if they're used elsewhere in the file. If not, the `format` import in the use statement `use ffmpeg_the_third::{self as ffmpeg, format, ...}` is still needed (for `format::context::Input`, `format::context::Output` in function signatures). No change needed — `format` is still used.

However, `Dictionary` is no longer used directly in `remux()` (it's now used inside `open_input`). Check if `Dictionary` is still imported — yes, it's in the use statement and `open_input` uses it. No change needed.

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles without errors.

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests pass (existing remux unit tests don't exercise the open path — they test stream selection, chapters, etc.).

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1 | tail -10`
Expected: no warnings.

- [ ] **Step 7: Run formatter**

Run: `rustup run stable cargo fmt`
Expected: no changes or clean formatting applied.

- [ ] **Step 8: Commit**

```bash
git add src/media/remux.rs
git commit -m "feat: use custom AVIO buffer sizes in remux

remux() now uses open_input/open_output with configurable buffer sizes
instead of ffmpeg-the-third's default 32 KB convenience functions.

Input default: 2 MB (read_buffer_kb = 2048)
Output default: 1 MB (write_buffer_kb = 1024)"
```

---

### Task 7: Final verification

**Files:** None (verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1 | tail -10`
Expected: no warnings.

- [ ] **Step 3: Run formatter**

Run: `rustup run stable cargo fmt -- --check 2>&1`
Expected: no formatting issues.

- [ ] **Step 4: Verify config file output**

Run: `cargo test --lib config::tests::test_with_comments 2>&1 | tail -5`
Expected: PASS — config serialization includes the new buffer options with commented-out defaults.
