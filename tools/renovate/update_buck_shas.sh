#!/usr/bin/env bash
# Recompute sha256 pins in a BUCK file for downloads whose URL changed vs the
# base version, and only those.
# An unchanged pin keeps its reviewed sha, so a silently re-published upstream
# artifact fails the build rather than being relocked to the new bytes.
# Usage: update_buck_shas.sh <buck-file> <base-buck-file>
# The base file is the base-branch version of <buck-file>; an empty or absent
# base treats every pin as new.
set -euo pipefail

# The `<name>_url` value in file $1, or empty if the file or name is absent.
url_of() {
	[[ -f "$1" ]] || return 0
	sed -n "s/.*${2}_url = \"\\([^\"]*\\)\".*/\\1/p" "$1"
}

# Names in BUCK $1 whose `<name>_url` differs from base $2.
changed_names() {
	local buck="$1" base="$2" name
	grep -oE '[A-Za-z0-9_]+_url = "[^"]*"' "$buck" | sed -E 's/_url = .*//' |
		while read -r name; do
			[[ "$(url_of "$buck" "$name")" != "$(url_of "$base" "$name")" ]] && echo "$name"
		done
}

main() {
	local buck="${1:?usage: update_buck_shas.sh <buck-file> <base-buck-file>}"
	local base="${2:-}" name url tmp sha
	for name in $(changed_names "$buck" "$base"); do
		url="$(url_of "$buck" "$name")"
		tmp="$(mktemp)"
		curl -fsSL "$url" -o "$tmp"
		sha="$(sha256sum "$tmp" | awk '{print $1}')"
		rm -f "$tmp"
		sed -i "s/\\(${name}_sha256 = \"\\)[0-9a-f]*\\(\"\\)/\\1${sha}\\2/" "$buck"
	done
}

[[ -n "${_UPDATE_BUCK_SHAS_SOURCED:-}" ]] || main "$@"
