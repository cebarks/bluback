# Bugs
-

# Features
- add pause/resume support during ripping (AtomicBool-based pause in remux loop)
- resume existing partial rip (confirmation on resume or overwrite)
- auto-detect supported drive read speeds for settings dropdown (requires SCSI/MMC GET PERFORMANCE or MODE SENSE; unreliable through USB bridges)

# Investigate Further

- ~~pure Rust MKV/ffprobe integration~~ Done: migrated to `ffmpeg-the-third` library bindings
    - ~~ffmpeg bindings~~ Done: all probe/remux via FFmpeg API
    - ~~chapter writing via `mkv-element` crate~~ Done: chapters injected via AVChapter during remux
- macos/windows support
