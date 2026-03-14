#!/bin/bash
set -euo pipefail

REPO="syarihu/local-knowledge-cli"
BIN_DIR="${HOME}/.local/bin"
CONFIG_DIR="${HOME}/.config/lk"

echo "=== local-knowledge-cli (lk) setup ==="
echo ""

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}-${ARCH}" in
    Darwin-arm64)  TARGET="lk-aarch64-apple-darwin" ;;
    Darwin-x86_64) TARGET="lk-x86_64-apple-darwin" ;;
    Linux-aarch64) TARGET="lk-aarch64-unknown-linux-gnu" ;;
    Linux-x86_64)  TARGET="lk-x86_64-unknown-linux-gnu" ;;
    *)
        echo "Error: Unsupported platform: ${OS}-${ARCH}"
        exit 1
        ;;
esac

echo "Platform: ${OS} ${ARCH} (${TARGET})"

# Determine version to install
VERSION="${1:-latest}"
if [ "$VERSION" = "latest" ]; then
    echo "Fetching latest release..."
    BASE_URL="https://github.com/${REPO}/releases/latest/download"
else
    BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
fi

# Create secure temporary directory (mktemp provides unpredictable names)
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

# Download binary archive
echo "Downloading ${TARGET}.tar.gz..."
curl -fSL "${BASE_URL}/${TARGET}.tar.gz" -o "$TMPDIR/${TARGET}.tar.gz"

# Download and verify checksum
echo "Verifying checksum..."
if curl -fsSL "${BASE_URL}/checksums.txt" -o "$TMPDIR/checksums.txt" 2>/dev/null; then
    EXPECTED_HASH=$(grep "${TARGET}.tar.gz" "$TMPDIR/checksums.txt" | awk '{print $1}')
    if [ -z "$EXPECTED_HASH" ]; then
        echo "Error: Checksum for ${TARGET}.tar.gz not found in checksums.txt"
        exit 1
    fi
    ACTUAL_HASH=$(shasum -a 256 "$TMPDIR/${TARGET}.tar.gz" | awk '{print $1}')
    if [ "$EXPECTED_HASH" != "$ACTUAL_HASH" ]; then
        echo "Error: Checksum mismatch!"
        echo "  Expected: $EXPECTED_HASH"
        echo "  Actual:   $ACTUAL_HASH"
        exit 1
    fi
    echo "Checksum verified."
else
    echo "Warning: checksums.txt not available, skipping verification."
fi

# Extract and install binary
tar xzf "$TMPDIR/${TARGET}.tar.gz" -C "$TMPDIR"
mkdir -p "$BIN_DIR"
rm -f "$BIN_DIR/lk"
mv "$TMPDIR/lk" "$BIN_DIR/lk"
chmod +x "$BIN_DIR/lk"
echo "Installed binary: $BIN_DIR/lk"

# Install Claude commands via the binary itself (commands are embedded)
echo ""
echo "Installing Claude commands..."
"$BIN_DIR/lk" install-commands 2>/dev/null || {
    echo "  Note: install-commands not available in this version, skipping."
}

# Save config
INSTALLED_VERSION="$("$BIN_DIR/lk" --version 2>/dev/null | awk '{print $2}' || echo "unknown")"
mkdir -p "$CONFIG_DIR"
cat > "$CONFIG_DIR/config.json" <<EOF
{
  "install_dir": "",
  "installed_at": "$(date -u +%Y-%m-%dT%H:%M:%S)",
  "version": "${INSTALLED_VERSION}",
  "repo": "${REPO}"
}
EOF
echo ""
echo "Saved config to $CONFIG_DIR/config.json"

# Check PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo ""
    echo "WARNING: $BIN_DIR is not in your PATH."
    echo "Add the following to your shell profile (~/.zshrc or ~/.bashrc):"
    echo ""
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

echo ""
echo "Setup complete! (lk ${INSTALLED_VERSION})"
echo ""
echo "Next steps:"
echo "  1. cd <your-project>"
echo "  2. lk init"
echo ""
