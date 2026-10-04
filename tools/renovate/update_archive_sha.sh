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
	local line name="" sha="" url="" size="" quotes in_string=0
	while IFS= read -r line || [[ -n "$line" ]]; do
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

# In pin file $1, inside the archive block named $2, replace the first
# occurrence of $3 on each line of the block containing $3 with $4.
# Fails if no line changed.
# Skips triple-quoted string content the same way archives_of does.
rewrite_in_block() {
	local file="$1" block="$2" old="$3" new="$4"
	local line quotes in_string=0 name="" changed=0 out
	out="$(mktemp)"
	while IFS= read -r line || [[ -n "$line" ]]; do
		quotes="${line//[^\"]/}"
		if [[ "$line" == *'"""'* ]]; then
			[[ "$quotes" == '"""' ]] && in_string=$((1 - in_string))
		elif [[ "$in_string" == 0 ]]; then
			[[ "$line" == *'name = "'* ]] && name="$(sed -n 's/.*name = "\([^"]*\)".*/\1/p' <<<"$line")"
			if [[ "$name" == "$block" && "$line" == *"$old"* ]]; then
				line="${line/"$old"/"$new"}"
				changed=1
			fi
			[[ "$line" =~ \)[[:space:]]*$ ]] && name=""
		fi
		printf '%s\n' "$line" >>"$out"
	done <"$file"
	cat "$out" >"$file"
	rm -f "$out"
	[[ "$changed" == 1 ]]
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
		rewrite_in_block "$pinfile" "$name" "sha256 = \"${sha}\"" "sha256 = \"${newsha}\""
		if [[ -n "$size" ]]; then
			rewrite_in_block "$pinfile" "$name" "size_bytes = ${size}," "size_bytes = ${newsize},"
		fi
	done < <(archives_of "$pinfile")
}

[[ -n "${_UPDATE_ARCHIVE_SHA_SOURCED:-}" ]] || main "$@"
