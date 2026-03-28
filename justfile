# bluback development justfile

# Default recipe: list available recipes
default:
    @just --list

# --- Build ---

# Debug build
build:
    cargo build

# Release build
release:
    cargo build --release

# Clean build artifacts
clean:
    cargo clean

# --- Test & Lint ---

# Run all tests
test:
    cargo test

# Run a specific test by name
test-one name:
    cargo test -- {{name}}

# Run clippy with warnings as errors
lint:
    cargo clippy -- -D warnings

# Check formatting
fmt-check:
    cargo fmt -- --check

# Apply formatting
fmt:
    cargo fmt

# Run all checks (test + lint + fmt)
check: test lint fmt-check

# --- Run ---

# Run bluback with args
run *args:
    cargo run -- {{args}}

# List playlists on disc
list:
    cargo run -- --list-playlists

# List playlists with verbose stream info
list-verbose:
    cargo run -- --list-playlists -v

# Run environment validation
check-env:
    cargo run -- --check

# Open settings panel
settings:
    cargo run -- --settings

# --- macOS Setup ---

[macos]
setup-macos: _setup-ffmpeg-libbluray _setup-lib-symlinks
    @echo "macOS setup complete. Run 'just check-env' to verify."

# Rebuild FFmpeg with libbluray support (macOS)
[macos]
_setup-ffmpeg-libbluray:
    #!/usr/bin/env bash
    set -euo pipefail
    if ffmpeg -protocols 2>&1 | grep -q '^\s*bluray$'; then
        echo "FFmpeg already has bluray protocol support."
    else
        echo "Patching FFmpeg formula to add libbluray..."
        formula=$(brew formula ffmpeg)
        if ! grep -q 'libbluray' "$formula"; then
            sed -i '' '/depends_on "x265"/a\'"$(printf '\n  depends_on "libbluray"')" "$formula"
            sed -i '' '/--enable-libx265/a\'"$(printf '\n      --enable-libbluray')" "$formula"
        fi
        echo "Rebuilding FFmpeg from source (this takes a few minutes)..."
        HOMEBREW_NO_INSTALL_FROM_API=1 HOMEBREW_NO_AUTO_UPDATE=1 brew reinstall ffmpeg --build-from-source
        echo "Verifying..."
        ffmpeg -protocols 2>&1 | grep -q '^\s*bluray$' && echo "FFmpeg bluray protocol: OK" || echo "ERROR: bluray protocol still missing"
    fi

# Create /usr/local/lib symlinks for libbluray's dlopen (macOS, requires sudo)
[macos]
_setup-lib-symlinks:
    #!/usr/bin/env bash
    set -euo pipefail
    sudo mkdir -p /usr/local/lib
    if [ -f /opt/homebrew/lib/libaacs.dylib ]; then
        sudo ln -sf /opt/homebrew/lib/libaacs.dylib /usr/local/lib/libaacs.dylib
        echo "Symlinked libaacs.dylib"
    else
        echo "libaacs not found — install with: brew install libaacs"
    fi
    if [ -f "/Applications/MakeMKV.app/Contents/lib/libmmbd_new.dylib" ]; then
        sudo ln -sf /Applications/MakeMKV.app/Contents/lib/libmmbd_new.dylib /usr/local/lib/libmmbd.dylib
        echo "Symlinked libmmbd.dylib"
    else
        echo "MakeMKV not found — install with: brew install makemkv (optional, for LibreDrive)"
    fi

# Install all macOS dependencies via Homebrew
[macos]
deps:
    brew install ffmpeg llvm pkg-config libaacs libbluray

# --- Linux Setup ---

# Install dependencies (Fedora)
[linux]
deps:
    sudo dnf install ffmpeg-free-devel clang clang-libs pkg-config

# --- Release ---

# Bump version, commit, tag, and push
bump version:
    #!/usr/bin/env bash
    set -euo pipefail
    sed -i'' -e 's/^version = ".*"/version = "{{version}}"/' Cargo.toml
    cargo check
    git add Cargo.toml Cargo.lock
    git commit -m "chore: bump version to {{version}}"
    git push origin main

# Tag and push a release (triggers CI release workflow)
tag version:
    git tag v{{version}}
    git push origin v{{version}}

# Publish to crates.io
publish:
    cargo publish
