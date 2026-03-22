# CI/Builds Design

## Overview

Add GitHub Actions CI and release workflows to bluback, modeled after the dotm project's workflow structure.

## CI Workflow (`.github/workflows/ci.yml`)

**Triggers:** push to `main`, PRs against `main`

**Jobs (run in parallel):**

### test
- Checkout, install stable Rust toolchain, enable cargo cache
- `cargo test --locked`

### lint
- Checkout, install stable Rust toolchain with clippy component, enable cargo cache
- `cargo clippy --locked -- -D warnings`

**Actions used:**
- `actions/checkout@v4`
- `dtolnay/rust-toolchain@stable`
- `Swatinem/rust-cache@v2`

**Environment:** `CARGO_TERM_COLOR: always`

## Release Workflow (`.github/workflows/release.yml`)

**Triggers:** push of tags matching `v[0-9]+.*`

**Permissions:** `contents: write` (needed to create GitHub Releases)

**Environment:** `CARGO_TERM_COLOR: always`

### Job 1: gate

Runs tests and clippy as a quality gate before building release artifacts.

Steps:
1. Checkout, install stable Rust toolchain with clippy, enable cargo cache
2. Validate that the git tag version matches `Cargo.toml` version (fail if mismatch)
3. `cargo test --locked`
4. `cargo clippy --locked -- -D warnings`

### Job 2: build (depends on gate)

Build release binaries for Linux targets using a matrix strategy.

**Matrix:**

| Target | Runner |
|---|---|
| `x86_64-unknown-linux-gnu` | `ubuntu-latest` |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` |

Steps:
1. Checkout, install stable Rust toolchain, enable cargo cache (keyed by target)
2. `cargo build --release --locked`
3. Package: `tar czf bluback-<target>.tar.gz` containing the `bluback` binary, `LICENSE`, and `README.md`
4. Upload artifact via `actions/upload-artifact@v4`

### Job 3: publish (depends on build)

Create a GitHub Release with the built artifacts.

Steps:
1. Download all artifacts via `actions/download-artifact@v4` with `merge-multiple: true`
2. Create GitHub Release via `softprops/action-gh-release@v2` with auto-generated release notes and `artifacts/*.tar.gz` attached

## Release Process

To cut a release:
1. Bump version in `Cargo.toml`
2. Run `cargo check` to update `Cargo.lock` to match the new version
3. Commit both files: `git commit -am "release v0.X.Y"`
4. Tag: `git tag v0.X.Y`
5. Push: `git push && git push --tags`

The gate job validates the tag matches `Cargo.toml`, then builds and publishes automatically.

## Future Considerations

- macOS targets (`aarch64-apple-darwin`) can be added to the build matrix later
- crates.io publishing can be added as a step in the publish job if desired
