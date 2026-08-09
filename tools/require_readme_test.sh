#!/usr/bin/env bash
# Verifies that every Bazel package has a README.md file.
#
# Usage: tools/require_readme_test.sh
#   Run from the workspace root (same as check.sh).
set -euo pipefail

missing=()
# List tracked BUILD files: jj locally, git in CI (no jj there). Either
# failing aborts (set -e) rather than scanning nothing.
if command -v jj >/dev/null; then
	build_files="$(jj file list --ignore-working-copy 'glob:**/BUILD.bazel' 'glob:**/BUILD')"
else
	build_files="$(git ls-files ':(glob)**/BUILD.bazel' ':(glob)**/BUILD')"
fi
while IFS= read -r build_file; do
	[[ -n "$build_file" ]] || continue
	pkg_dir="$(dirname "$build_file")"
	# Root package — always has README.
	if [[ "$pkg_dir" == "." ]]; then
		continue
	fi
	if [[ ! -f "$pkg_dir/README.md" ]]; then
		missing+=("$pkg_dir")
	fi
done <<<"$(printf '%s\n' "$build_files" | sed 's|^|./|' | sort)"

if ((${#missing[@]} > 0)); then
	echo "FAIL: the following Bazel packages are missing a README.md:" >&2
	for pkg in "${missing[@]}"; do
		echo "  $pkg" >&2
	done
	exit 1
fi

echo "readme ok: all Bazel packages have a README.md"
