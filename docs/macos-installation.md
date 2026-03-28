# macOS Installation Guide

## Prerequisites

### Install Homebrew

If you don't have Homebrew installed:

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

### Install Runtime Dependencies

```bash
brew install libaacs libbluray
```

### Rebuild FFmpeg with libbluray support

Homebrew's default FFmpeg does **not** include Blu-ray protocol support. You must patch the formula and rebuild from source:

```bash
# Edit the FFmpeg formula
brew edit ffmpeg
```

Add these two lines to the formula:
1. Add `depends_on "libbluray"` in the dependencies section (near the other `depends_on` lines)
2. Add `--enable-libbluray` in the configure args section (near the other `--enable-lib*` lines)

Then rebuild:

```bash
HOMEBREW_NO_INSTALL_FROM_API=1 HOMEBREW_NO_AUTO_UPDATE=1 brew reinstall ffmpeg --build-from-source
```

Verify the bluray protocol is available:

```bash
ffmpeg -protocols 2>&1 | grep bluray
# Should output: bluray
```

### Library symlinks

libbluray loads libaacs at runtime via `dlopen()`, but macOS's default search path doesn't include Homebrew's `/opt/homebrew/lib/`. You need symlinks in `/usr/local/lib/`:

```bash
sudo mkdir -p /usr/local/lib
sudo ln -sf /opt/homebrew/lib/libaacs.dylib /usr/local/lib/libaacs.dylib
```

### Set up AACS keys

bluback requires AACS decryption keys to read encrypted Blu-ray discs.

1. Download the KEYDB.cfg from the [FindVUK Online Database](http://fvonline-db.bplaced.net/)
2. Create the config directory: `mkdir -p ~/.config/aacs`
3. Place the file: `mv ~/Downloads/KEYDB.cfg ~/.config/aacs/`

### MakeMKV / libmmbd setup (optional, for LibreDrive)

Some discs (MKBv72+ without per-disc VUKs in KEYDB.cfg) require MakeMKV's libmmbd backend for decryption. This is also needed for drives with LibreDrive firmware.

```bash
# Install MakeMKV
brew install makemkv

# Symlink libmmbd where libbluray can find it
sudo ln -sf /Applications/MakeMKV.app/Contents/lib/libmmbd_new.dylib /usr/local/lib/libmmbd.dylib
```

MakeMKV requires a license. Set the current beta key (check [the forum](https://forum.makemkv.com/forum/viewtopic.php?f=5&t=1053) for the latest):

```bash
mkdir -p ~/.MakeMKV
echo 'app_Key = "your-beta-key-here"' > ~/.MakeMKV/settings.conf
```

Then use libmmbd with bluback:

```bash
bluback --aacs-backend libmmbd
```

## Installation

### From GitHub Releases

Download the latest macOS binary from the [releases page](https://github.com/cebarks/bluback/releases):

```bash
# Download (replace VERSION with actual version)
curl -LO https://github.com/cebarks/bluback/releases/download/vVERSION/bluback-aarch64-apple-darwin.tar.gz
tar xzf bluback-aarch64-apple-darwin.tar.gz
sudo mv bluback /usr/local/bin/
```

### From Source

Requires Xcode Command Line Tools and Homebrew.

```bash
# Install build dependencies
brew install ffmpeg llvm pkg-config

# Add llvm to PATH (add to ~/.zshrc for persistence)
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"

# Clone and build
git clone https://github.com/cebarks/bluback.git
cd bluback
cargo build --release

# Binary at target/release/bluback
```

## Verify Installation

```bash
bluback --check
```

Expected output:

```
[PASS] FFmpeg libraries
[PASS] libbluray
[PASS] libaacs
[PASS] KEYDB.cfg
[PASS] libmmbd          (if MakeMKV installed)
[PASS] makemkvcon        (if MakeMKV installed)
[PASS] diskutil
[PASS] Optical drives    (if drive connected)
```

## Finding Your Blu-ray Drive

macOS uses `/dev/diskN` paths for block devices:

```bash
diskutil list
```

Look for your optical drive — it will show the disc label and typically be `/dev/disk2` or higher. You can also use `drutil status` to see the current optical drive.

## Usage

```bash
# Auto-detect drive (recommended)
bluback

# Specify drive explicitly
bluback -d /dev/disk2 -o ~/Movies

# Check what's on the disc
bluback --list-playlists

# Use libmmbd for AACS decryption
bluback --aacs-backend libmmbd
```

## Troubleshooting

### "Protocol not found" or "bluray: Protocol not found"

FFmpeg was not compiled with libbluray support. See [Rebuild FFmpeg with libbluray support](#rebuild-ffmpeg-with-libbluray-support) above.

### "No optical drives detected"

- Ensure your Blu-ray drive is connected and powered on
- Run `diskutil list` or `drutil status` to verify the drive is recognized
- Try specifying the device explicitly with `-d /dev/diskN`

### "No usable AACS libraries found"

libbluray can't find libaacs or libmmbd via `dlopen()`. Create the symlinks:

```bash
sudo ln -sf /opt/homebrew/lib/libaacs.dylib /usr/local/lib/libaacs.dylib
# If using libmmbd:
sudo ln -sf /Applications/MakeMKV.app/Contents/lib/libmmbd_new.dylib /usr/local/lib/libmmbd.dylib
```

### "AACS authentication failed" / "Input/output error"

- Verify `~/.config/aacs/KEYDB.cfg` exists and is up-to-date
- For newer discs (MKBv72+), you may need a per-disc VUK in KEYDB.cfg
- Try the libmmbd backend: `bluback --aacs-backend libmmbd`
- If using libmmbd, ensure MakeMKV is registered (beta key or purchased)

### "Failed to mount"

- macOS usually auto-mounts optical discs to `/Volumes/<LABEL>`
- bluback detects and uses existing mounts
- If mount fails, try ejecting and re-inserting the disc

### Build errors with clang

- Ensure llvm is installed: `brew install llvm`
- Add to PATH: `export PATH="/opt/homebrew/opt/llvm/bin:$PATH"`
- Verify: `which clang` should point to Homebrew's clang
