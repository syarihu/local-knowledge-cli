---
keywords: [ci, cd, github-actions, release, cross-compile]
category: infrastructure
---

# CI/CD Pipeline

## Entry: Release Workflow
keywords: [release.yml, build, matrix, checksum]

The `.github/workflows/release.yml` triggers on `v*` tags and has 2 jobs. The `build` job uses a 5-platform matrix (macOS aarch64/x86_64, Linux aarch64/x86_64, Windows x86_64) to cross-compile with `cargo build --release`. Linux aarch64 uses `gcc-aarch64-linux-gnu` as a cross-compiler. Unix platforms produce `lk-{target}.tar.gz` artifacts, Windows produces a `.zip`. The `release` job downloads all artifacts, generates `checksums.txt` with SHA256 hashes, and creates a GitHub Release via `softprops/action-gh-release`.

## Entry: Setup Script
keywords: [setup.sh, install, curl, checksum-verify]

The `setup.sh` (109 lines) installer detects the platform, downloads the binary from GitHub Releases, verifies the SHA256 checksum, installs to `~/.local/bin/lk`, runs `lk install-commands`, saves config to `~/.config/lk/config.json`, and warns if `~/.local/bin` is not in PATH. Supports specifying a version argument or defaults to latest.
