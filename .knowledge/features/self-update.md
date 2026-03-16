---
keywords: [update, self-update, github-releases, checksum]
category: features
---

# Self-Update Mechanism

## Entry: Update Flow
keywords: [fetch_latest_tag, verify_checksum, update]

The `cmd_update()` function in `src/cmd/update.rs` handles self-updating. It detects homebrew installations via `is_homebrew_install()` and runs `brew upgrade` instead of manual download. For non-homebrew installs, it downloads the latest release from GitHub, verifies the SHA256 checksum, and installs to `~/.local/bin/lk`. Both paths share post-update logic: install embedded Claude commands, update config, and run DB migration. The version displayed is read from the newly installed binary to show the correct new version.

## Entry: Platform Detection
keywords: [detect_target, platform, cross-platform]

The `detect_target()` function in `src/cmd/update.rs` maps OS and architecture to release artifact names. Supported platforms: macOS aarch64 (Apple Silicon), macOS x86_64 (Intel), Linux aarch64 (ARM64), and Linux x86_64. The artifact naming convention is `lk-{target}.tar.gz`.
