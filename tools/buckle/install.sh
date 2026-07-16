#!/usr/bin/env bash
# Install the buckle launcher as `buck2` into $1 (default /usr/local/bin).
set -euo pipefail

BINDIR="${1:-/usr/local/bin}"
BUCKLE_VERSION=1.1.0
BUCKLE_SHA256=dad88b264b1139ff12c30b81c5a71b9ddee54b4148e0f45a2708f7d809bd151d

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL \
	"https://github.com/benbrittain/buckle/releases/download/v${BUCKLE_VERSION}/buckle-x86_64-unknown-linux-gnu.tar.xz" \
	-o "$tmp/buckle.tar.xz"
echo "${BUCKLE_SHA256}  $tmp/buckle.tar.xz" | sha256sum -c -
tar xf "$tmp/buckle.tar.xz" -C "$tmp"
install -m 0755 "$tmp/buckle-x86_64-unknown-linux-gnu/buckle" "$BINDIR/buckle"
install -m 0755 "$(dirname "$0")/buck2.sh" "$BINDIR/buck2"
