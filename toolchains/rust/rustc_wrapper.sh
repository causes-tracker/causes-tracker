#!/bin/sh
# Runs rustc with its outputs redirected to a scratch directory and copied
# onto the declared paths afterwards: rustc moves each output into place with
# a rename, which fails with EXDEV on the worker, where the declared paths are
# on another mount than rustc's temporaries.
# PATH gets the staged bash for the prelude's `#!/usr/bin/env bash` linker
# wrapper, which rustc runs, and /bin for the busybox utilities that wrapper's
# zig_flatten.sh runs.
#
# Invoked as: rustc_wrapper.sh <bash-bin-dir> <rustc> <args...>
# The prelude's rustc driver passes the compile flags as one @argsfile.
set -eu

export PATH="$1:/bin${PATH:+:$PATH}"
shift

argsfile=""
for arg in "$@"; do
	case "$arg" in
	@*) argsfile="${arg#@}" ;;
	esac
done
if [ -z "$argsfile" ]; then
	exec "$@"
fi

scratch="$(mktemp -d "$TMPDIR/rustc_wrapper.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
rewritten="$scratch/args"
declared_out_dir=""
declared_emits=""
: >"$rewritten"

# Points --out-dir and every --emit path into the scratch, remembering the
# declared paths.
while IFS= read -r line || [ -n "$line" ]; do
	case "$line" in
	--out-dir=*)
		declared_out_dir="${line#--out-dir=}"
		mkdir -p "$scratch/out-dir"
		printf -- '--out-dir=%s/out-dir\n' "$scratch" >>"$rewritten"
		;;
	--emit=*=*)
		kind="${line#--emit=}"
		kind="${kind%%=*}"
		declared="${line#--emit=*=}"
		declared_emits="$declared_emits
$declared"
		printf -- '--emit=%s=%s/emit-%s\n' "$kind" "$scratch" "${declared##*/}" >>"$rewritten"
		;;
	*) printf '%s\n' "$line" >>"$rewritten" ;;
	esac
done <"$argsfile"

# Rebuilds argv with the rewritten argsfile in place of the original.
n=$#
for arg in "$@"; do
	case "$arg" in
	@*) set -- "$@" "@$rewritten" ;;
	*) set -- "$@" "$arg" ;;
	esac
done
shift "$n"

rc=0
"$@" || rc=$?
if [ "$rc" != 0 ]; then
	exit "$rc"
fi

if [ -n "$declared_out_dir" ]; then
	mkdir -p "$declared_out_dir"
	cp -a "$scratch/out-dir/." "$declared_out_dir/"
fi
printf '%s\n' "$declared_emits" | while IFS= read -r declared; do
	[ -n "$declared" ] || continue
	cp "$scratch/emit-${declared##*/}" "$declared"
done
