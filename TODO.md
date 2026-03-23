# Bugs
-

# Features
- settings overhaul
    - config panel
    - more settings exposed
- add pause/resume support during ripping (pause ffmpeg via SIGSTOP/SIGCONT)
- update terminal title with basic status
- resume existing partial rip (confirmation on resume or overwrite)

# Investigate Further

- pure Rust MKV/ffprobe integration (overlaps with `~/code/media-tools` use case)
    - ffmpeg bindings
    - chapter writing via `mkv-element` crate to replace `mkvpropedit` shell-out (blocked on crate maturity and in-place EBML modification support)
- macos/windows support
