#!/usr/bin/env bash
# Flags `bazel run|build` whose stdout is captured by $(...) without
# `--quiet` between `bazel` and the subcommand.
#
# Usage: tools/require_bazel_quiet_test.sh
#   Run from the workspace root (same as check.sh).
set -uo pipefail

violations_file="$(mktemp)"
trap 'rm -f "$violations_file"' EXIT

# List tracked files: jj locally, git in CI (no jj there). Abort on a
# listing failure so a broken enumeration can't pass by scanning nothing.
if command -v jj >/dev/null; then
	files="$(jj file list --ignore-working-copy 'glob:**/*.sh' 'glob:**/*.md')"
else
	files="$(git ls-files ':(glob)**/*.sh' ':(glob)**/*.md')"
fi || {
	echo "FAIL: could not list tracked files" >&2
	exit 1
}

while IFS= read -r file; do
	[[ -n "$file" ]] || continue
	awk -v file="$file" '
		file ~ /\.sh$/ && /^[[:space:]]*#/ { next }
		/\$\([^)]*bazel/ {
			if ($0 !~ /bazel[[:space:]]+(--[^[:space:]]+[[:space:]]+)*(run|build)[[:space:]]/) next
			if ($0 ~ /bazel[[:space:]]+(--[^[:space:]]+[[:space:]]+)*--quiet/) next
			print file ":" NR ": output-captured bazel run/build missing --quiet"
			print "    " $0
		}
	' "$file"
done <<<"$(printf '%s\n' "$files" | sort)" >"$violations_file"

if [[ -s "$violations_file" ]]; then
	echo "FAIL: bazel run/build commands missing --quiet:" >&2
	cat "$violations_file" >&2
	echo "" >&2
	echo "Fix: add --quiet between 'bazel' and the subcommand" >&2
	echo "(e.g. \`bazel --quiet run //infra:tofu -- output -raw foo\`)." >&2
	exit 1
fi

echo "bazel-quiet ok: all output-captured bazel run/build commands use --quiet"
