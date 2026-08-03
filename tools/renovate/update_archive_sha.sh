#!/usr/bin/env bash
# Recompute pinned archive sha256s — and, for buck2 archives, their size_bytes —
# in a BUCK file or MODULE.bazel for the archives whose URL changed vs the base
# version, and only those.
# An unchanged URL keeps its first-seen sha, so a silently re-published upstream
# artifact fails the build rather than being relocked to the new bytes.
# Renovate rewrites a version in the URL but cannot compute the sha256
# (renovatebot/renovate#22183); this closes that gap on the bump branch.
# Usage: update_archive_sha.sh <pin-file> <base-pin-file>
set -euo pipefail

# Emit "name<TAB>sha256<TAB>url[<TAB>size_bytes]" for each archive in file $1 — a
# rule block that carries both a `sha256 = "..."` and a single-element
# `urls = ["..."]` (BUCK cached_http_archive) or a singular `url = "..."`
# (MODULE.bazel http_archive/http_file).
# The trailing size_bytes field is emitted only when the block declares one.
archives_of() {
	[[ -f "$1" ]] || return 0
	local line name="" sha="" url="" size="" quotes in_string=0 dsha durl dname
	while IFS= read -r line; do
		# Lines inside (or delimiting) a triple-quoted string are literal
		# content, not fields or block ends.
		quotes="${line//[^\"]/}"
		if [[ "$line" == *'"""'* ]]; then
			[[ "$quotes" == '"""' ]] && in_string=$((1 - in_string))
			continue
		fi
		if [[ "$in_string" == 1 ]]; then
			continue
		fi
		# A one-line dict pin ({"sha256": ..., "url": ...}, e.g. an apk package
		# list entry) is self-contained; its stable key is the package name,
		# the url basename up to its version.
		if [[ "$line" == *'"sha256": "'* && "$line" == *'"url": "'* ]]; then
			dsha="$(sed -n 's/.*"sha256": "\([^"]*\)".*/\1/p' <<<"$line")"
			durl="$(sed -n 's/.*"url": "\([^"]*\)".*/\1/p' <<<"$line")"
			dname="${durl##*/}"
			dname="${dname%%-[0-9]*}"
			printf '%s\t%s\t%s\n' "$dname" "$dsha" "$durl"
			continue
		fi
		[[ "$line" == *'name = "'* ]] && name="$(sed -n 's/.*name = "\([^"]*\)".*/\1/p' <<<"$line")"
		[[ "$line" == *'sha256 = "'* ]] && sha="$(sed -n 's/.*sha256 = "\([^"]*\)".*/\1/p' <<<"$line")"
		[[ "$line" == *'urls = ["'* ]] && url="$(sed -n 's/.*urls = \["\([^"]*\)"\].*/\1/p' <<<"$line")"
		[[ "$line" == *'url = "'* ]] && url="$(sed -n 's/.*url = "\([^"]*\)".*/\1/p' <<<"$line")"
		[[ "$line" == *'size_bytes = '* ]] && size="$(sed -n 's/.*size_bytes = \([0-9]*\).*/\1/p' <<<"$line")"
		if [[ "$line" =~ \)[[:space:]]*$ ]]; then
			if [[ -n "$sha" && -n "$url" ]]; then
				if [[ -n "$size" ]]; then
					printf '%s\t%s\t%s\t%s\n' "$name" "$sha" "$url" "$size"
				else
					printf '%s\t%s\t%s\n' "$name" "$sha" "$url"
				fi
			fi
			name=""
			sha=""
			url=""
			size=""
		fi
	done <"$1"
}

# In pin file $1, replace the (unique) sha256 value $2 with $3; fail if $3 is absent.
rewrite_sha() {
	sed -i "s/${2}/${3}/" "$1"
	grep -q "$3" "$1"
}

# In pin file $1, replace size_bytes value $2 with $3; fail if $3 is absent.
rewrite_size() {
	sed -i "s/size_bytes = ${2},/size_bytes = ${3},/" "$1"
	grep -q "size_bytes = ${3}," "$1"
}

main() {
	local pinfile="${1:?usage: update_archive_sha.sh <pin-file> <base-pin-file>}"
	local base="${2:-}" name sha url size newsha newsize tmp
	declare -A base_url=()
	while IFS=$'\t' read -r name sha url size; do base_url["$name"]="$url"; done < <(archives_of "$base")
	while IFS=$'\t' read -r name sha url size; do
		[[ "$url" == "${base_url[$name]:-}" ]] && continue
		tmp="$(mktemp)"
		curl -fsSL "$url" -o "$tmp"
		newsha="$(sha256sum "$tmp" | awk '{print $1}')"
		newsize="$(stat -c%s "$tmp")"
		rm -f "$tmp"
		rewrite_sha "$pinfile" "$sha" "$newsha"
		if [[ -n "$size" ]]; then
			rewrite_size "$pinfile" "$size" "$newsize"
		fi
	done < <(archives_of "$pinfile")
}

[[ -n "${_UPDATE_ARCHIVE_SHA_SOURCED:-}" ]] || main "$@"
