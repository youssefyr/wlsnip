#!/usr/bin/env bash
set -euo pipefail

# Screenshot script for Wayland compositors using wlsnip
# Usage:
#   snip.sh [region|full|annotate] [--slurp]

SCREENSHOT_DIR="${HOME}/Pictures/Screenshots"
mkdir -p "$SCREENSHOT_DIR"

timestamp="$(date +%Y%m%d-%H%M%S)"
filepath="${SCREENSHOT_DIR}/screenshot-${timestamp}.png"

SLURP_FLAG=""
for arg in "$@"; do
    case "$arg" in
        --slurp) SLURP_FLAG="--slurp" ;;
    esac
done

mode="${1:-region}"

case "$mode" in
    region)
        wlsnip region $SLURP_FLAG --clipboard --notify -o "$filepath"
        ;;

    full)
        wlsnip full $SLURP_FLAG --clipboard --notify -o "$filepath"
        ;;

    annotate)
        # wlsnip natively supports opening the capture in satty
        wlsnip region $SLURP_FLAG --annotate --clipboard --notify -o "$filepath"
        ;;

    -h|--help|help)
        echo "Usage: snip.sh [region|full|annotate] [--slurp]"
        ;;

    *)
        echo "Usage: snip.sh [region|full|annotate] [--slurp]"
        exit 2
        ;;
esac
