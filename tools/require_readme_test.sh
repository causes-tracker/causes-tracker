#!/usr/bin/env bash
# Verifies that every Bazel package has a README.md file.
#
# Usage: tools/require_readme_test.sh
#   Run from the workspace root (same as coverage.sh).
set -euo pipefail

missing=()
while IFS= read -r build_file; do
	pkg_dir="$(dirname "$build_file")"
	# Root package — always has README.
	if [[ "$pkg_dir" == "." ]]; then
		continue
	fi
	if [[ ! -f "$pkg_dir/README.md" ]]; then
		missing+=("$pkg_dir")
	fi
done < <(find . -name BUILD.bazel -o -name BUILD | sort)

if ((${#missing[@]} > 0)); then
	echo "FAIL: the following Bazel packages are missing a README.md:" >&2
	for pkg in "${missing[@]}"; do
		echo "  $pkg" >&2
	done
	exit 1
fi

echo "readme ok: all Bazel packages have a README.md"
