#!/usr/bin/env bash
# Install the pinned crun binary into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

CRUN_VERSION=1.30.1
CRUN_SHA256=8093b6d104408d6c8dfdd3436e06dbf345074587e0848c1e262a17b96847d6fb
install_release containers/crun "$CRUN_SHA256" \
	"$CRUN_VERSION" "crun-$CRUN_VERSION-linux-amd64" \
	raw crun "${1:-/usr/local/bin}"
