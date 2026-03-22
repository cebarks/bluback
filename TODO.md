
# Bugs
- disc scan after no disc found still shows no disc was found on the drive even after scanning on a newly discovered disc has been found
- "unmounted /dev/sr0" text stuck on screen after state moves on
- file size estimates are way off (~2x)
- eject hint not shown on rip finish screen 
- ripping screen
    - should show current rip at the top or highlight the rip in the list
    - Replace "Done" with "Completed"
- step 3 starting episode selection should be interactive not a field
- why is there a lag between step 5 and 6?

# Features
- add loading screen 
- add visual feedback for loading screens ("rotating" pipe; looping elipses)
- settings overhaul
    - config panel
    - more settings exposed
    - 
- specials/extras support?
    - step 5 -> 6
    - step 1 -> 2
- add pause/resume support during ripping (pause ffmpeg via SIGSTOP/SIGCONT)
- update terminal title with basic status
- show raw disc name in all screens

# Investigate Further
- ffmpeg bindings
- pure Rust MKV integration
    - chapter writing via `mkv-element` crate to replace `mkvpropedit` shell-out (blocked on crate maturity and in-place EBML modification support)
- macos/windows support
