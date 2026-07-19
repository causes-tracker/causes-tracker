#!/usr/bin/env bash
# Install the pinned crane binary into $1 (default /usr/local/bin).
set -euo pipefail

BINDIR="${1:-/usr/local/bin}"
CRANE_VERSION=0.21.7
CRANE_SHA256=c14340087103ba9dadf61d45acd20675490fd0ccbd56ac7901fc1b502137f44b

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL \
	"https://github.com/google/go-containerregistry/releases/download/v${CRANE_VERSION}/go-containerregistry_Linux_x86_64.tar.gz" \
	-o "$tmp/crane.tar.gz"
echo "${CRANE_SHA256}  $tmp/crane.tar.gz" | sha256sum -c -
tar xzf "$tmp/crane.tar.gz" -C "$tmp" crane
install -m 0755 "$tmp/crane" "$BINDIR/crane"
