#!/usr/bin/env bash
# Install the pinned crane binary into $1 (default /usr/local/bin).
set -euo pipefail

BINDIR="${1:-/usr/local/bin}"
CRANE_VERSION=0.21.7
CRANE_SHA256=1a57bc98207fa1c0d04bf760699099e26f8383499bfd55b99c1b919a928a7230

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL \
	"https://github.com/google/go-containerregistry/releases/download/v${CRANE_VERSION}/go-containerregistry_Linux_x86_64.tar.gz" \
	-o "$tmp/crane.tar.gz"
echo "${CRANE_SHA256}  $tmp/crane.tar.gz" | sha256sum -c -
tar xzf "$tmp/crane.tar.gz" -C "$tmp" crane
install -m 0755 "$tmp/crane" "$BINDIR/crane"
