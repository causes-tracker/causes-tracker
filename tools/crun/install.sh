#!/usr/bin/env bash
# Install the pinned crun binary into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

CRUN_VERSION=1.29.1
CRUN_SHA256=0a5ea25cafe618bbfbf1c747871155063619f18025ccdd8ad648c97633f35d57
install_release containers/crun "$CRUN_SHA256" \
	"$CRUN_VERSION" "crun-$CRUN_VERSION-linux-amd64" \
	raw crun "${1:-/usr/local/bin}"
