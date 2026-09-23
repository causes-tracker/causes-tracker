#!/usr/bin/env bash
# Install the pinned NativeLink binary into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

NATIVELINK_VERSION=1.7.0
NATIVELINK_SHA256=cb6c2da6f3f51023a45e5b85d3dc3971cd103007a160e93933356fdc0dfc2682
install_release TraceMachina/nativelink "$NATIVELINK_SHA256" \
	"v$NATIVELINK_VERSION" "nativelink-$NATIVELINK_VERSION-x86_64-unknown-linux-musl.tar.gz" \
	tgz nativelink "${1:-/usr/local/bin}"
