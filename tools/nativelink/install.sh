#!/usr/bin/env bash
# Install the pinned NativeLink binary into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

NATIVELINK_VERSION=1.7.6
NATIVELINK_SHA256=63e266a513c23b5d6e24c0fc52cd547af7e8a70c2de79a9360cb309292f69d70
install_release TraceMachina/nativelink "$NATIVELINK_SHA256" \
	"v$NATIVELINK_VERSION" "nativelink-$NATIVELINK_VERSION-x86_64-unknown-linux-musl.tar.gz" \
	tgz nativelink "${1:-/usr/local/bin}"
