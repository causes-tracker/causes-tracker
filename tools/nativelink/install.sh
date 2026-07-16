#!/usr/bin/env bash
# Install the pinned NativeLink binary into $1 (default /usr/local/bin).
set -euo pipefail

BINDIR="${1:-/usr/local/bin}"
NATIVELINK_VERSION=1.6.1
NATIVELINK_SHA256=cd861c1acd8c14f023741d35a310f89527aeadcb681e9818d8e823ee72ae017c

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL \
	"https://github.com/TraceMachina/nativelink/releases/download/v${NATIVELINK_VERSION}/nativelink-${NATIVELINK_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
	-o "$tmp/nativelink.tar.gz"
echo "${NATIVELINK_SHA256}  $tmp/nativelink.tar.gz" | sha256sum -c -
tar xzf "$tmp/nativelink.tar.gz" -C "$tmp"
install -m 0755 "$tmp/nativelink" "$BINDIR/nativelink"
