#!/usr/bin/env bash
# Install the pinned crun binary into $1 (default /usr/local/bin).
set -euo pipefail
. "$(dirname "$0")/../install_release.shlib"

CRUN_VERSION=1.29
CRUN_SHA256=4d64bc4c49366f1288b9ace27717486d325031ab07abfbe4a6d0e2c50146d998
install_release containers/crun "$CRUN_SHA256" \
	"$CRUN_VERSION" "crun-$CRUN_VERSION-linux-amd64" \
	raw crun "${1:-/usr/local/bin}"
