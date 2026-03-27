# macOS Installation Guide

## Prerequisites

### Install Homebrew

If you don't have Homebrew installed:

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

### Install Runtime Dependencies

```bash
brew install ffmpeg libaacs libbluray
```

### Set Up AACS Keys

bluback requires AACS decryption keys to read encrypted Blu-ray discs.

1. Download the KEYDB.cfg from the [FindVUK Online Database](http://fvonline-db.bplaced.net/)
2. Create the config directory: `mkdir -p ~/.config/aacs`
3. Place the file: `mv ~/Downloads/KEYDB.cfg ~/.config/aacs/`

## Installation

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

Expected output includes:
- `[PASS] FFmpeg libraries`
- `[PASS] libbluray`
- `[PASS] libaacs`
- `[PASS] KEYDB.cfg`
- `[PASS] diskutil`

## Finding Your Blu-ray Drive

macOS uses `/dev/diskN` paths for block devices:

```bash
diskutil list
```

Look for your optical drive, typically `/dev/disk2` or `/dev/disk3`.

## Usage

```bash
# Auto-detect drive (recommended)
bluback

# Specify drive explicitly
bluback -d /dev/disk2 -o ~/Movies

# Check what's on the disc
bluback --list-playlists
```

## Troubleshooting

### "No optical drives detected"

- Ensure your Blu-ray drive is connected and powered on
- Run `diskutil list` to verify the drive is recognized
- Try specifying the device explicitly with `-d /dev/diskN`

### "Failed to mount"

- macOS usually auto-mounts optical discs to `/Volumes/<LABEL>`
- bluback detects and uses existing mounts
- If mount fails, try ejecting and re-inserting the disc

### "AACS decryption failed"

- Verify `~/.config/aacs/KEYDB.cfg` exists and is up-to-date
- For newer discs (MKBv72+), you may need the per-disc VUK in KEYDB.cfg
- Consider using MakeMKV's libmmbd backend: `bluback --aacs-backend libmmbd`

### Build errors with clang

- Ensure llvm is installed: `brew install llvm`
- Add to PATH: `export PATH="/opt/homebrew/opt/llvm/bin:$PATH"`
- Verify: `which clang` should point to Homebrew's clang
