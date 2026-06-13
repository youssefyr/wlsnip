#!/usr/bin/env bash
set -euo pipefail

echo "Building wlsnip in release mode..."
cargo build --release

BIN_PATH="target/release/wlsnip"
if [ ! -f "$BIN_PATH" ]; then
    echo "Error: Release binary not found at $BIN_PATH"
    exit 1
fi

INSTALL_DIR="$HOME/.local/bin"
mkdir -p "$INSTALL_DIR"

echo "Installing to $INSTALL_DIR..."
cp "$BIN_PATH" "$INSTALL_DIR/wlsnip"

echo "wlsnip installed successfully to $INSTALL_DIR/wlsnip"
echo "Make sure $INSTALL_DIR is in your PATH."
