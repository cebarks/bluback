# AVIO Buffer Tuning

**Date:** 2026-04-16
**Status:** Approved
**Version target:** v0.12 (or next minor)

## Problem

FFmpeg's default AVIO buffer is 32 KB for both input and output contexts. For Blu-ray remux through a USB drive (especially with ASMedia bridge chips), this causes excessive per-transaction overhead:

- ~156 buffer refills/sec at 5 MB/s throughput
- Each refill triggers: FFmpeg protocol handler -> libbluray -> libaacs decryption -> kernel -> USB bridge SCSI command
- ASMedia USB-SATA bridges have measurable per-SCSI-command latency (command setup, bridge translation)
- AACS decrypts in 6144-byte aligned units; function call overhead per `bd_read()` compounds
- Observed: ~2.6x rip speed on a drive capable of 6x+

For output, many small `write()` syscalls add unnecessary context-switch and VFS overhead. This is especially noticeable on NFS targets where each syscall can trigger an RPC round-trip.

## Solution

Bypass `ffmpeg-the-third`'s convenience open functions and call the underlying FFI functions directly, inserting a buffer resize step between the open and probe/write phases.

### Input (disc read)

1. `avformat_alloc_context()` - allocate raw format context
2. `avformat_open_input()` with `bluray:{device}` URL + playlist dictionary - creates AVIO with default 32 KB buffer. **Must remain inside `MakemkvconGuard::track_open()`** to preserve makemkvcon lifecycle management.
3. **`resize_avio_buffer()`** - resize to configured `read_buffer_kb` before any probing occurs
4. `avformat_find_stream_info()` - probes with the larger buffer. **On failure, must call `avformat_close_input()` to clean up** (mirrors upstream `ffmpeg-the-third` behavior).
5. **Re-verify `pb->buffer_size`** - `avformat_find_stream_info` can seek internally, which may trigger FFmpeg's internal buffer resize via `orig_buffer_size` (not accessible from public API), reverting to 32 KB. If reverted, resize again. Defensive, zero-cost.
6. `Input::wrap()` - re-enter `ffmpeg-the-third` safe types

Separating open from probe (which `format::input_with_dictionary` combines) lets us resize the buffer at a clean state boundary: the protocol is open, but no demuxer data has been buffered yet.

### Output (file write)

1. `avformat_alloc_output_context2()` - allocate output format context
2. `avio_open2()` - open output file, creates AVIO with default 32 KB buffer
3. **`resize_avio_buffer()`** - resize to configured `write_buffer_kb` before any writes. **On failure, must call `avio_close()` + `avformat_free_context()` to clean up.**
4. `Output::wrap()` - re-enter `ffmpeg-the-third` safe types

### Buffer resize helper

Reimplements FFmpeg's internal (non-public) `ffio_set_buf_size`:

```rust
unsafe fn resize_avio_buffer(pb: *mut AVIOContext, new_size: usize, is_write: bool) -> Result<(), MediaError>
```

- **Atomic operation**: allocates new buffer with `av_malloc(new_size)` FIRST. Only if allocation succeeds, frees old buffer with `av_free(old_buffer)`. If `av_malloc` returns null, leaves original buffer intact and returns error.
- Sets `buffer`, `buffer_size` to new values
- Resets pointer state:
  - Read mode: `buf_ptr = buffer`, `buf_end = buffer` (empty buffer, next read refills from protocol)
  - Write mode: `buf_ptr = buffer`, `buf_end = buffer + size` (full capacity available for writes)
- Sets `buf_ptr_max = buffer`
- `checksum_ptr` is not reset — bluray input and file output protocols do not use checksumming, so it remains null/unused
- Cleanup safety: `avformat_close_input` (input) and `avio_close` (output) call `av_free` on the buffer during drop, which correctly frees our `av_malloc`-allocated replacement

### Config

Two new fields in `Config`:

| Field | Type | Default | Rationale |
|-------|------|---------|-----------|
| `read_buffer_kb` | `Option<u32>` | 2048 (2 MB) | Reduces disc read refills from ~156/sec to ~2.5/sec at 5 MB/s. Aligns with typical I-frame/GOP sizes (500 KB - 2 MB). Diminishing returns above 4 MB. |
| `write_buffer_kb` | `Option<u32>` | 1024 (1 MB) | Reduces `write()` syscalls from ~156/sec to ~5/sec. Aligns with NFS default wsize. Trivial memory cost. |

- Added to `KNOWN_KEYS` for unknown-key validation
- Added to `with_comments()` for config file serialization (commented-out defaults)
- Validation: warn if < 32 (below FFmpeg default) or > 4096 (4 MB, diminishing returns)
- Config-only, no CLI flags (power-user tuning knob)

### RemuxOptions

Two new fields:

```rust
pub read_buffer_kb: u32,
pub write_buffer_kb: u32,
```

Populated from config in `workflow::prepare_remux_options()`.

## Files changed

| File | Change |
|------|--------|
| `src/media/remux.rs` | New `open_input()`, `open_output()`, `resize_avio_buffer()` functions; `remux()` uses them instead of `format::input_with_dictionary` / `format::output` |
| `src/config.rs` | New fields, constants, accessor methods, `KNOWN_KEYS`, `with_comments()`, validation |
| `src/workflow.rs` | Pass `read_buffer_kb` / `write_buffer_kb` to `RemuxOptions` |
| `src/media/remux.rs` (tests) | Config parsing, validation, serialization roundtrip |
| `src/config.rs` (tests) | Buffer size config tests |

## Testing

- Config: parsing, defaults, validation (too small / too large warnings), `with_comments()` roundtrip
- Remux: `open_input` / `open_output` can't be unit-tested without a real device, but the `resize_avio_buffer` logic is straightforward FFI pointer manipulation covered by the existing integration path
- Manual: rip a disc with default (2048/1024) vs original (32/32) and compare reported speed

## Out of scope

- **`probe.rs` buffer tuning**: `src/media/probe.rs` also opens bluray inputs via `format::input_with_dictionary()`. Probe reads are small (headers/stream info only), so the benefit of larger buffers would be marginal. Can be added later if needed.
- **`min_packet_size` interaction**: `AVIOContext.min_packet_size` can affect write buffer flush timing. The Matroska muxer does not set this, so it has no effect on MKV output.

## Risks

- **`orig_buffer_size` not accessible**: FFmpeg 8.x moved this to an internal `FFIOContext` wrapper struct (not public API). `avformat_find_stream_info` can seek internally, which may trigger an FFmpeg-internal buffer resize that reverts to the original 32 KB. Mitigated by the post-probe re-verify step (input step 5). During the packet loop, remux is sequential — no seeks occur, so the field is not consulted again.
- **AACS error path**: If `avformat_open_input` fails, it frees the format context (documented behavior: "a user-supplied AVFormatContext will be freed on failure"). Our code must not double-free.
- **Resize failure after open**: If `resize_avio_buffer` fails (allocation failure), the original buffer remains intact (atomic allocate-before-free). The context can be cleaned up normally.
