# CI Multi-Platform Coverage Design

**Date:** 2026-03-28
**Goal:** Build confidence — ensure bluback compiles and tests pass on all supported platforms.

## Scope

Replace the current split CI (`ci.yml` + `macos.yml`) with a single consolidated `ci.yml` covering 5 platform targets. `release.yml` is not modified.

## Workflow Structure

Single `ci.yml`, triggered on push to `main` and PRs to `main`. Two independent jobs (no dependency between them — both run in parallel):

### Job 1: `lint`

- **Runner:** `ubuntu-latest`
- **Steps:** Install FFmpeg dev libs → Install Rust stable (with `clippy` and `rustfmt` components) → `cargo fmt --check` → `cargo clippy --locked -- -D warnings`
- **Rationale:** Formatting and clippy are platform-independent for this codebase (no meaningful `#[cfg(target_os)]` conditional compilation). Running once saves CI minutes.

### Job 2: `build-and-test`

5-entry matrix:

| Name | Runner | Container | Dependency Install | Rust Install |
|------|--------|-----------|--------------------|--------------|
| Ubuntu x86_64 | `ubuntu-latest` | — | `apt-get` | `dtolnay/rust-toolchain@stable` |
| Ubuntu aarch64 | `ubuntu-24.04-arm` | — | `apt-get` | `dtolnay/rust-toolchain@stable` |
| Fedora x86_64 | `ubuntu-latest` | `fedora:43` | `dnf install` | `dnf install rust cargo` |
| Fedora aarch64 | `ubuntu-24.04-arm` | `fedora:43` | `dnf install` | `dnf install rust cargo` |
| macOS aarch64 | `macos-latest` | — | `brew install` | `dtolnay/rust-toolchain@stable` |

**Steps per matrix entry:** Install deps → Install Rust → `cargo build --locked` → `cargo test --locked`

All 5 targets are hard gates — all must pass for a PR to merge.

## Platform Details

### Ubuntu (x86_64, aarch64)

```
apt-get install -y libavformat-dev libavcodec-dev libavutil-dev \
  libswscale-dev libswresample-dev libavfilter-dev libavdevice-dev \
  pkg-config clang libclang-dev
```

### Fedora (x86_64, aarch64)

```
dnf install -y ffmpeg-free-devel clang clang-devel pkg-config rust cargo
```

Uses `ffmpeg-free-devel` from base repos (codec-limited, but sufficient for build/test since bluback only does lossless remux). RPMFusion is not needed.

Container jobs (`container: fedora:43`) cannot use `dtolnay/rust-toolchain` or `Swatinem/rust-cache` — these require the host runner environment. Rust is installed via dnf instead.

### macOS (aarch64)

```
brew install ffmpeg llvm pkg-config
```

Adds `/opt/homebrew/opt/llvm/bin` to `$GITHUB_PATH` for clang/bindgen.

## Caching

- **Ubuntu/macOS:** `Swatinem/rust-cache@v2` with `key: ${{ matrix.target }}` for per-platform cache isolation.
- **Fedora:** No cargo cache (container job incompatibility). Clean builds each run (~3-4 min, acceptable).

## Action Versions

All jobs use consistent versions:
- `actions/checkout@v5`
- `dtolnay/rust-toolchain@stable` (non-container jobs)
- `Swatinem/rust-cache@v2` (non-container jobs)

## Files Changed

- **Modified:** `.github/workflows/ci.yml` — rewritten with lint + build-and-test matrix
- **Deleted:** `.github/workflows/macos.yml` — fully replaced by macOS matrix entry

## Design Decisions

- **No MSRV testing:** Only `stable` is tested. Users building from source on old Rust can file issues.
- **No `continue-on-error`:** All platforms are hard gates.
- **No job dependency between lint and build-and-test:** Parallel execution gives fastest wall-clock feedback.
- **Clippy in lint only:** Platform-specific clippy differences are unlikely given the codebase's minimal `cfg` usage. Can be promoted to the matrix later if needed.
- **Fedora uses base repo FFmpeg:** `ffmpeg-free-devel` is sufficient for compilation and tests. RPMFusion would add complexity for no CI benefit.
