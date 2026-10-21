#!/usr/bin/env bash
# Install the pinned NativeLink binary into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

NATIVELINK_VERSION=1.7.7
NATIVELINK_SHA256=6e8f964c5d39434410ea6f5cc6251c0834bf6afd075e184538425016567848e9
install_release TraceMachina/nativelink "$NATIVELINK_SHA256" \
	"v$NATIVELINK_VERSION" "nativelink-$NATIVELINK_VERSION-x86_64-unknown-linux-musl.tar.gz" \
	tgz nativelink "${1:-/usr/local/bin}"
