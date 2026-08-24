#!/usr/bin/env bash
# Install the pinned NativeLink binary into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

NATIVELINK_VERSION=1.6.6
NATIVELINK_SHA256=7ea68447000a0d4f59c948634a6ff5094a3868f8d9961320aab6c0878bc67ab9
install_release TraceMachina/nativelink "$NATIVELINK_SHA256" \
	"v$NATIVELINK_VERSION" "nativelink-$NATIVELINK_VERSION-x86_64-unknown-linux-musl.tar.gz" \
	tgz nativelink "${1:-/usr/local/bin}"
