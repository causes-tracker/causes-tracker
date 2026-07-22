#!/usr/bin/env bash
# Install the buckle launcher as `buck2` into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

BINDIR="${1:-/usr/local/bin}"
BUCKLE_VERSION=1.1.0
BUCKLE_SHA256=dad88b264b1139ff12c30b81c5a71b9ddee54b4148e0f45a2708f7d809bd151d
install_release benbrittain/buckle "$BUCKLE_SHA256" \
	"v$BUCKLE_VERSION" "buckle-x86_64-unknown-linux-gnu.tar.xz" \
	txz buckle "$BINDIR" "buckle-x86_64-unknown-linux-gnu/buckle"
install -m 0755 "$(dirname "$0")/buck2.sh" "$BINDIR/buck2"
