
# Bugs
- disc scan after no disc found still shows no disc was found on the drive even after scanning on a newly discovered disc has been found
- "unmounted /dev/sr0" text stuck on screen after state moves on
- ripping screen
    - should show current rip at the top of the list (currently just bolded)
- step 3 starting episode selection should be interactive not a field
- why is there a lag between step 5 and 6?
- can't skip detected select playlists in playlist select
- playlist edit should be a mode switch with easy editting of all listed playlists
- step 5 and step 4 should be a single interface
-

# Features
- add loading screen 
- add visual feedback for loading screens ("rotating" pipe; looping elipses)
- settings overhaul
    - config panel
    - more settings exposed
    - minimum playlist length should be an option
- specials/extras support?
    - step 5 -> 6
    - step 1 -> 2
- add pause/resume support during ripping (pause ffmpeg via SIGSTOP/SIGCONT)
- update terminal title with basic status
- show raw disc name in all screens
- auto-detect new disk on finish screen
- resume existing partial rip (confirmation on resume or overwrite)

# Investigate Further
- ffmpeg bindings
- pure Rust MKV integration
    - chapter writing via `mkv-element` crate to replace `mkvpropedit` shell-out (blocked on crate maturity and in-place EBML modification support)
- macos/windows support
