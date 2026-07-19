#!/usr/bin/env bash
# Rewrite the pinned sha256 in an installer script to match the release
# asset of the pinned version. Idempotent: no change when the pin is
# already correct.
# The digest comes from the GitHub release-asset digest API when present
# (assets uploaded since 2025-06); older assets are downloaded and hashed.
# Usage: update_sha.sh <installer-path>
set -euo pipefail

INSTALLER="$1"

case "$INSTALLER" in
tools/buckle/install.sh)
	REPO=benbrittain/buckle
	VAR=BUCKLE
	ASSET='buckle-x86_64-unknown-linux-gnu.tar.xz'
	;;
tools/nativelink/install.sh)
	REPO=TraceMachina/nativelink
	VAR=NATIVELINK
	ASSET='nativelink-VERSION-x86_64-unknown-linux-musl.tar.gz'
	;;
tools/crane/install.sh)
	REPO=google/go-containerregistry
	VAR=CRANE
	ASSET='go-containerregistry_Linux_x86_64.tar.gz'
	;;
*)
	echo "unknown installer: $INSTALLER" >&2
	exit 1
	;;
esac

VERSION="$(sed -n "s/^${VAR}_VERSION=//p" "$INSTALLER")"
if [[ -z "$VERSION" ]]; then
	echo "no ${VAR}_VERSION in $INSTALLER" >&2
	exit 1
fi
ASSET="${ASSET//VERSION/$VERSION}"

DIGEST="$(gh api "repos/${REPO}/releases/tags/v${VERSION}" \
	--jq ".assets[] | select(.name == \"${ASSET}\") | .digest // empty")"
if [[ -n "$DIGEST" ]]; then
	SHA="${DIGEST#sha256:}"
else
	tmp="$(mktemp)"
	trap 'rm -f "$tmp"' EXIT
	curl -fsSL \
		"https://github.com/${REPO}/releases/download/v${VERSION}/${ASSET}" \
		-o "$tmp"
	SHA="$(sha256sum "$tmp" | awk '{print $1}')"
fi

sed -i "s/^${VAR}_SHA256=.*/${VAR}_SHA256=${SHA}/" "$INSTALLER"
grep -q "^${VAR}_SHA256=${SHA}\$" "$INSTALLER"
