# Bugs
-

# Features
- add pause/resume support during ripping (pause ffmpeg via SIGSTOP/SIGCONT)
- resume existing partial rip (confirmation on resume or overwrite)
- auto-detect supported drive read speeds for settings dropdown (requires SCSI/MMC GET PERFORMANCE or MODE SENSE; unreliable through USB bridges)

# Investigate Further

- pure Rust MKV/ffprobe integration (overlaps with `~/code/media-tools` use case)
    - ffmpeg bindings
    - chapter writing via `mkv-element` crate to replace `mkvpropedit` shell-out (blocked on crate maturity and in-place EBML modification support)
- macos/windows support
