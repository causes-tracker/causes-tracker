#!/usr/bin/env bash
# Install the pinned crun binary into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

CRUN_VERSION=1.30.1
CRUN_SHA256=86d1e6a0e76945975d3aebfab39cbc6a26eea15f1c3fc66b6776d19e5dc346a0
install_release containers/crun "$CRUN_SHA256" \
	"$CRUN_VERSION" "crun-$CRUN_VERSION-linux-amd64" \
	raw crun "${1:-/usr/local/bin}"
