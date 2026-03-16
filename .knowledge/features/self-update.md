---
keywords: [update, self-update, github-releases, checksum]
category: features
---

# Self-Update Mechanism

## Entry: Update Flow
keywords: [fetch_latest_tag, verify_checksum, update]

The `cmd_update()` function in `src/main.rs` downloads the latest release from GitHub. Version discovery uses `gh release view` with a curl redirect fallback (`fetch_latest_tag()`). The binary is downloaded to a temp directory (via tempfile crate), verified with SHA256 checksum (`verify_checksum()`), extracted, and installed to `~/.local/bin/lk`. After installation, embedded Claude commands are automatically re-installed. The version displayed in "Update complete!" is read from the newly installed binary (not the running process) to show the correct new version.

## Entry: Platform Detection
keywords: [detect_target, platform, cross-platform]

The `detect_target()` function in `src/main.rs` maps OS and architecture to release artifact names. Supported platforms: macOS aarch64 (Apple Silicon), macOS x86_64 (Intel), Linux aarch64 (ARM64), and Linux x86_64. The artifact naming convention is `lk-{target}.tar.gz`.
