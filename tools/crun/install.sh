#!/usr/bin/env bash
# Install the pinned crun binary into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

CRUN_VERSION=1.28
CRUN_SHA256=2aa6b7024a9c9f153895c0d11ae233d3758f54844011c3a039e3e89048d01d42
install_release containers/crun "$CRUN_SHA256" \
	"$CRUN_VERSION" "crun-$CRUN_VERSION-linux-amd64" \
	raw crun "${1:-/usr/local/bin}"
