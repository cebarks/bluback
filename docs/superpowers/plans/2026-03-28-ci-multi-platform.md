# CI Multi-Platform Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Consolidate CI into a single workflow with lint + 5-platform build/test matrix.

**Architecture:** Single `ci.yml` with two independent jobs: `lint` (fmt + clippy, once on Ubuntu) and `build-and-test` (5-entry matrix covering Ubuntu x86_64/aarch64, Fedora x86_64/aarch64, macOS aarch64). Fedora runs in `fedora:43` containers on Ubuntu runners.

**Tech Stack:** GitHub Actions, Rust stable, FFmpeg dev libraries

**Spec:** `docs/superpowers/specs/2026-03-28-ci-multi-platform-design.md`

---

### Task 1: Rewrite ci.yml with lint and build-and-test jobs

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Replace ci.yml with the consolidated workflow**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  lint:
    name: Lint
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5

      - name: Install FFmpeg development libraries
        run: >
          sudo apt-get update && sudo apt-get install -y
          libavformat-dev libavcodec-dev libavutil-dev
          libswscale-dev libswresample-dev libavfilter-dev libavdevice-dev
          pkg-config clang libclang-dev

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt

      - uses: Swatinem/rust-cache@v2

      - run: cargo fmt --check
      - run: cargo clippy --locked -- -D warnings

  build-and-test:
    name: Build & Test (${{ matrix.name }})
    runs-on: ${{ matrix.runner }}
    container: ${{ matrix.container || '' }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - name: Ubuntu x86_64
            runner: ubuntu-latest
            target: x86_64-unknown-linux-gnu
          - name: Ubuntu aarch64
            runner: ubuntu-24.04-arm
            target: aarch64-unknown-linux-gnu
          - name: Fedora x86_64
            runner: ubuntu-latest
            target: x86_64-unknown-linux-gnu-fedora
            container: fedora:43
          - name: Fedora aarch64
            runner: ubuntu-24.04-arm
            target: aarch64-unknown-linux-gnu-fedora
            container: fedora:43
          - name: macOS aarch64
            runner: macos-latest
            target: aarch64-apple-darwin
    steps:
      - uses: actions/checkout@v5

      - name: Install dependencies (Ubuntu)
        if: startsWith(matrix.name, 'Ubuntu')
        run: >
          sudo apt-get update && sudo apt-get install -y
          libavformat-dev libavcodec-dev libavutil-dev
          libswscale-dev libswresample-dev libavfilter-dev libavdevice-dev
          pkg-config clang libclang-dev

      - name: Install dependencies (Fedora)
        if: startsWith(matrix.name, 'Fedora')
        run: dnf install -y ffmpeg-free-devel clang clang-devel pkg-config rust cargo

      - name: Install dependencies (macOS)
        if: startsWith(matrix.name, 'macOS')
        run: |
          brew install ffmpeg llvm pkg-config
          echo "/opt/homebrew/opt/llvm/bin" >> $GITHUB_PATH

      - name: Install Rust
        if: ${{ !matrix.container }}
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo
        if: ${{ !matrix.container }}
        uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}

      - run: cargo build --locked
      - run: cargo test --locked
```

- [ ] **Step 2: Review the diff**

Run: `git diff .github/workflows/ci.yml`

Verify:
- Two jobs: `lint` and `build-and-test`
- `lint` has `clippy, rustfmt` components and runs both `fmt --check` and `clippy`
- Matrix has 5 entries with correct runners, containers, and targets
- `fail-fast: false` so all platforms report results even if one fails early
- Fedora steps use `dnf`, skip `dtolnay/rust-toolchain` and `Swatinem/rust-cache`
- macOS adds llvm to `$GITHUB_PATH`
- All checkout actions are `@v5`

---

### Task 2: Delete macos.yml

**Files:**
- Delete: `.github/workflows/macos.yml`

- [ ] **Step 1: Delete the file**

Run: `rm .github/workflows/macos.yml`

- [ ] **Step 2: Verify it's gone**

Run: `ls .github/workflows/`

Expected output shows only `ci.yml` and `release.yml`.

---

### Task 3: Validate and commit

- [ ] **Step 1: Validate workflow syntax (if actionlint is available)**

Run: `actionlint .github/workflows/ci.yml 2>&1 || echo "actionlint not installed, skipping"`

If actionlint reports errors, fix them before proceeding.

- [ ] **Step 2: Suggest commit**

Suggested commit message:
```
ci: consolidate into single workflow with 5-platform matrix

Replace separate ci.yml + macos.yml with a unified workflow.
Lint (fmt + clippy) runs once on Ubuntu. Build and test runs
on Ubuntu x86_64/aarch64, Fedora x86_64/aarch64, and macOS
aarch64. Fedora jobs use container: fedora:43.
```

Files to stage:
```bash
git add .github/workflows/ci.yml
git rm .github/workflows/macos.yml
```

---

### Task 4: Push and verify workflows run

- [ ] **Step 1: Push branch and open PR**

Run:
```bash
git checkout -b ci/multi-platform
git push -u origin ci/multi-platform
gh pr create --title "ci: consolidate into single workflow with 5-platform matrix" --body "$(cat <<'EOF'
## Summary
- Replaces separate `ci.yml` + `macos.yml` with a single unified workflow
- Lint (fmt + clippy) runs once on Ubuntu
- Build & test matrix covers 5 platforms: Ubuntu x86_64/aarch64, Fedora x86_64/aarch64, macOS aarch64
- Fedora jobs use `container: fedora:43` on Ubuntu runners
- All platforms are hard gates

## Test plan
- [ ] All 5 build-and-test matrix entries pass
- [ ] Lint job passes (fmt + clippy)
- [ ] Fedora jobs correctly install deps via dnf and Rust via dnf
- [ ] macOS job finds clang via GITHUB_PATH

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 2: Monitor workflow runs**

Run: `gh run list --limit 5`

Check that all jobs start and the matrix fans out to 5 entries plus 1 lint job (6 total).

- [ ] **Step 3: If any job fails, inspect logs**

Run: `gh run view <run-id> --log-failed`

Common failure modes:
- Fedora: missing package name (check `dnf search ffmpeg`)
- macOS: llvm path wrong (verify `ls /opt/homebrew/opt/llvm/bin/clang`)
- aarch64: runner not available (check GitHub plan supports arm runners)
- Container: `actions/checkout@v5` needs `git` installed (Fedora base image should have it; if not, add `dnf install -y git` before checkout)
