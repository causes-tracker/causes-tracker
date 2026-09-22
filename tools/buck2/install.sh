#!/usr/bin/env bash
# Install the pinned buck2 binary (as buck2-bin) and its launcher (as buck2)
# into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

BINDIR="${1:-/usr/local/bin}"
BUCK2_VERSION=2026-09-15
BUCK2_SHA256=631eb84a8925d19146e0f5d594b9f2d0b1f0864e63f24e0d5914a75a6a0a4dea
install_release facebook/buck2 "$BUCK2_SHA256" \
	"$BUCK2_VERSION" "buck2-x86_64-unknown-linux-musl.zst" \
	zst buck2-bin "$BINDIR"
install -m 0755 "$(dirname "$0")/buck2.sh" "$BINDIR/buck2"
