#!/usr/bin/env bash
# Rewrite the pinned sha256 in an installer script to match the release
# asset of the pinned version. Idempotent: no change when the pin is
# already correct.
# The digest comes from the GitHub release-asset digest API when present
# (assets uploaded since 2025-06); older assets are downloaded and hashed.
# Usage: update_sha.sh <installer-path> [installer-file]
set -euo pipefail

# update_sha <installer-path> [installer-file]
#
# installer-path selects the repo/asset-naming case below; installer-file
# (defaulting to installer-path) is the file read and rewritten — tests point
# it at a scratch copy so the real installer trees stay untouched.
update_sha() {
	local installer="$1" file="${2:-$1}"
	local repo var asset tag_prefix

	case "$installer" in
	tools/buck2/install.sh)
		repo=facebook/buck2
		var=BUCK2
		asset='buck2-x86_64-unknown-linux-musl.zst'
		tag_prefix=
		;;
	tools/nativelink/install.sh)
		repo=TraceMachina/nativelink
		var=NATIVELINK
		asset='nativelink-VERSION-x86_64-unknown-linux-musl.tar.gz'
		tag_prefix=v
		;;
	tools/crun/install.sh)
		repo=containers/crun
		var=CRUN
		asset='crun-VERSION-linux-amd64'
		tag_prefix=
		;;
	*)
		echo "unknown installer: $installer" >&2
		return 1
		;;
	esac

	local version
	version="$(sed -n "s/^${var}_VERSION=//p" "$file")"
	if [[ -z "$version" ]]; then
		echo "no ${var}_VERSION in $file" >&2
		return 1
	fi
	asset="${asset//VERSION/$version}"
	local tag="${tag_prefix}${version}"

	local digest sha
	digest="$(gh api "repos/${repo}/releases/tags/${tag}" \
		--jq ".assets[] | select(.name == \"${asset}\") | .digest // empty")"
	if [[ -n "$digest" ]]; then
		sha="${digest#sha256:}"
	else
		local tmp
		tmp="$(mktemp)"
		trap 'rm -f "$tmp"' RETURN
		curl -fsSL \
			"https://github.com/${repo}/releases/download/${tag}/${asset}" \
			-o "$tmp"
		sha="$(sha256sum "$tmp" | awk '{print $1}')"
	fi

	sed -i "s/^${var}_SHA256=.*/${var}_SHA256=${sha}/" "$file"
	grep -q "^${var}_SHA256=${sha}\$" "$file"
}

if [[ -z "${_UPDATE_SHA_SOURCED:-}" ]]; then
	update_sha "$1"
fi
