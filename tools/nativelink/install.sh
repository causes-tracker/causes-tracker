#!/usr/bin/env bash
# Install the pinned NativeLink binary into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

NATIVELINK_VERSION=1.6.3
NATIVELINK_SHA256=bbc5ff8cede1af7546c3e60ca11070971384fef2e966c5cbc1aefa44da995ed3
install_release TraceMachina/nativelink "$NATIVELINK_SHA256" \
	"v$NATIVELINK_VERSION" "nativelink-$NATIVELINK_VERSION-x86_64-unknown-linux-musl.tar.gz" \
	tgz nativelink "${1:-/usr/local/bin}"
