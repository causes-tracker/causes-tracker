#!/usr/bin/env bash
# Bazel sh_test driver: runs buildifier -mode=check over workspace-relative
# paths received as space-separated args (from $(rootpaths :filegroup)).
set -euo pipefail

# shellcheck source=/dev/null
if [[ -f "${RUNFILES_DIR:-/dev/null}/rules_shell/shell/runfiles/runfiles.bash" ]]; then
	source "${RUNFILES_DIR}/rules_shell/shell/runfiles/runfiles.bash"
elif [[ -f "${RUNFILES_DIR:-/dev/null}/bazel_tools/tools/bash/runfiles/runfiles.bash" ]]; then
	source "${RUNFILES_DIR}/bazel_tools/tools/bash/runfiles/runfiles.bash"
else
	echo >&2 "ERROR: cannot find runfiles.bash"
	exit 1
fi

BUILDIFIER="$(rlocation buildifier_prebuilt+/buildifier/buildifier)"

files=()
for arg in "$@"; do
	for f in $arg; do
		# $(rootpath) for same-package files emits "./foo"; rlocation rejects "./".
		files+=("$(rlocation "_main/${f#./}")")
	done
done

[[ ${#files[@]} -gt 0 ]] || exit 0
exec "$BUILDIFIER" -mode=check "${files[@]}"
