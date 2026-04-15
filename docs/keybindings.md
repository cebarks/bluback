# TUI Keybindings

## Screen Flow

**TV mode:** Scanning → TMDb Search → Season → Playlist Manager → Confirm → Ripping → Done

**Movie mode:** Scanning → TMDb Search → Playlist Manager → Confirm → Ripping → Done

## Global (all screens)

| Key | Action |
|-----|--------|
| `Ctrl+S` | Open settings panel (overlay) |
| `Ctrl+H` | Open history overlay |
| `Ctrl+R` | Rescan disc and restart wizard (confirms during ripping) |
| `Ctrl+E` | Eject disc |
| `Ctrl+Left/Right` | Switch active drive tab (multi-drive) |
| `Ctrl+L` | Link TMDb context from another session (multi-drive) |
| `Ctrl+N` | Start new session on available drive (multi-drive) |
| `Ctrl+C` | Quit immediately |
| `q` | Quit (except during text input or ripping) |

When an overlay (settings or history) is open, all global keys except `Ctrl+C` are blocked — input routes to the overlay.

## TMDb Search

| Key | Action |
|-----|--------|
| `Enter` | Search (in input) / Select (in results) |
| `Up/Down` | Navigate between input and results |
| `Tab` | Toggle Movie/TV mode |
| `Esc` | Skip TMDb |

## Season (TV mode)

| Key | Action |
|-----|--------|
| `Enter` | Fetch episodes / Confirm and proceed |
| `Up/Down` | Scroll episode list |
| `Esc` | Go back to TMDb Search |

## Playlist Manager

| Key | Action |
|-----|--------|
| `Space` | Toggle playlist selection |
| `e` | Edit episode assignment inline (format: `3`, `3-4`, or `3,5`) |
| `s` | Toggle special marking (TV mode only) |
| `r` | Reset current row's assignment |
| `R` | Reset all episode assignments |
| `t` | Expand/collapse track list (video/audio/subtitle streams) |
| `f` | Show/hide filtered (short) playlists |
| `A` | Accept all auto-detected suggestions (medium+ confidence) |
| `Enter` | Confirm and proceed |
| `Esc` | Go back |

## Ripping Dashboard

| Key | Action |
|-----|--------|
| `q` | Abort (with confirmation) |

## Done Screen

| Key | Action |
|-----|--------|
| `Enter` | Rescan disc and restart wizard |
| Any other key | Exit |

The Done screen auto-detects disc insertion and shows a popup prompt.

## Settings Panel (overlay)

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate settings (skips separators) |
| `Enter/Space` | Toggle (bool), cycle (choice), enter edit (text/number), save (action) |
| `Left/Right` | Cycle choice backward/forward |
| `Esc` | Cancel edit (if editing), otherwise close panel |
| `Ctrl+S` | Save to config file |

## History Overlay (Ctrl+H)

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate session list |
| `Enter` | Toggle detail view (show/hide files) |
| `d` | Delete selected session (with confirmation) |
| `D` | Clear all sessions (with confirmation) |
| `y/n` | Confirm/cancel when prompted |
| `Esc` | Close detail view, or close overlay |
