#!/bin/sh
# Runs a zig subcommand with buck2's argsfiles flattened into one.
# zig rejects a @response-file that references another @response-file
# (NestedResponseFile); buck2's cxx rules pass one top argsfile that
# @-includes per-concern sub-argsfiles.
#
# Invoked as: zig_flatten.sh <zig> <verb> <args...>
set -eu

zig="$1"
verb="$2"
shift 2

# zig has no HOME on the worker (AppDataDirUnavailable); its caches live in
# the action's private $TMPDIR.
scratch="$(mktemp -d "$TMPDIR/zig_flatten.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
export ZIG_GLOBAL_CACHE_DIR="$scratch/global"
export ZIG_LOCAL_CACHE_DIR="$scratch/local"

flat="$scratch/args"
: >"$flat"
outdir=""
want_out=0
nl="
"

# Notes the directory of the argument following -o.
track_out() {
	if [ "$want_out" = 1 ]; then
		outdir="$(dirname "$1")"
		want_out=0
	elif [ "$1" = "-o" ]; then
		want_out=1
	fi
}

# Appends one argv argument as the response file's tokenizer reads it:
# whitespace splits a line, a leading # starts a comment, either quote
# toggles quoting, and a backslash run before a quote is halved with the
# quote escaped when the run is odd.
# A single quote cannot be produced (its escape yields a double quote), so an
# argument containing one, or a newline, is refused.
emit_arg() {
	case "$1" in
	*\'* | *"$nl"*)
		echo >&2 "zig_flatten.sh: argument cannot be written to a response file: $1"
		exit 1
		;;
	*[[:space:]\"\\]* | \#*)
		printf '%s' "$1" | sed 's/\(\\*\)"/\1\1\\"/g; s/\(\\*\)$/\1\1/; s/^/"/; s/$/"/' >>"$flat"
		echo >>"$flat"
		;;
	*) printf '%s\n' "$1" >>"$flat" ;;
	esac
	track_out "$1"
}

# Appends the lines of response file "$1" verbatim, recursing into nested
# @refs.
# `|| [ -n "$line" ]` keeps a final line without a trailing newline, which
# buck2's argsfiles routinely lack.
emit_file() {
	while IFS= read -r line || [ -n "$line" ]; do
		case "$line" in
		@*) emit_file "${line#@}" ;;
		*)
			printf '%s\n' "$line" >>"$flat"
			track_out "$line"
			;;
		esac
	done <"$1"
}

for arg in "$@"; do
	case "$arg" in
	@*) emit_file "${arg#@}" ;;
	*) emit_arg "$arg" ;;
	esac
done

# zig installs its output by renaming it out of the local cache, which fails
# with EXDEV when the output is on another mount than the scratch; the local
# cache goes beside the output instead.
if [ -n "$outdir" ]; then
	ZIG_LOCAL_CACHE_DIR="$(mktemp -d "$outdir/.zig_cache.XXXXXX")"
	trap 'rm -rf "$scratch" "$ZIG_LOCAL_CACHE_DIR"' EXIT
fi

"$zig" "$verb" "@$flat"
