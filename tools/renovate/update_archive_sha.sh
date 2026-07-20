#!/usr/bin/env bash
# Recompute cached_http_archive sha256 pins in a BUCK file for the archives
# whose URL changed vs the base version, and only those.
# An unchanged URL keeps its first-seen sha, so a silently re-published upstream
# artifact fails the build rather than being relocked to the new bytes.
# Renovate rewrites a version in the URL but cannot compute the sha256
# (renovatebot/renovate#22183); this closes that gap on the bump branch.
# Usage: update_archive_sha.sh <buck-file> <base-buck-file>
set -euo pipefail

# Emit "name<TAB>sha256<TAB>url" for each archive in file $1 — a rule block that
# carries both a `sha256 = "..."` and a single-element `urls = ["..."]`.
archives_of() {
	[[ -f "$1" ]] || return 0
	local line name="" sha="" url=""
	while IFS= read -r line; do
		[[ "$line" == *'name = "'* ]] && name="$(sed -n 's/.*name = "\([^"]*\)".*/\1/p' <<<"$line")"
		[[ "$line" == *'sha256 = "'* ]] && sha="$(sed -n 's/.*sha256 = "\([^"]*\)".*/\1/p' <<<"$line")"
		[[ "$line" == *'urls = ["'* ]] && url="$(sed -n 's/.*urls = \["\([^"]*\)"\].*/\1/p' <<<"$line")"
		if [[ "$line" == *')'* ]]; then
			[[ -n "$sha" && -n "$url" ]] && printf '%s\t%s\t%s\n' "$name" "$sha" "$url"
			name=""
			sha=""
			url=""
		fi
	done <"$1"
}

# In BUCK $1, replace the (unique) sha256 value $2 with $3; fail if $3 is absent.
rewrite_sha() {
	sed -i "s/${2}/${3}/" "$1"
	grep -q "$3" "$1"
}

main() {
	local buck="${1:?usage: update_archive_sha.sh <buck-file> <base-buck-file>}"
	local base="${2:-}" name sha url newsha tmp
	declare -A base_url=()
	while IFS=$'\t' read -r name sha url; do base_url["$name"]="$url"; done < <(archives_of "$base")
	while IFS=$'\t' read -r name sha url; do
		[[ "$url" == "${base_url[$name]:-}" ]] && continue
		tmp="$(mktemp)"
		curl -fsSL "$url" -o "$tmp"
		newsha="$(sha256sum "$tmp" | awk '{print $1}')"
		rm -f "$tmp"
		rewrite_sha "$buck" "$sha" "$newsha"
	done < <(archives_of "$buck")
}

[[ -n "${_UPDATE_ARCHIVE_SHA_SOURCED:-}" ]] || main "$@"
