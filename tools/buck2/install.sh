#!/usr/bin/env bash
# Install the pinned buck2 binary (as buck2-bin) and its launcher (as buck2)
# into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

BINDIR="${1:-/usr/local/bin}"
BUCK2_VERSION=2026-07-01
BUCK2_SHA256=c9fe39745fbe014a2d05b6b17369cc9be56838a9463455938b0f987826473a88
install_release facebook/buck2 "$BUCK2_SHA256" \
	"$BUCK2_VERSION" "buck2-x86_64-unknown-linux-musl.zst" \
	zst buck2-bin "$BINDIR"
install -m 0755 "$(dirname "$0")/buck2.sh" "$BINDIR/buck2"
