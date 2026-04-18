#!/usr/bin/env bash
# Bazel sh_test driver: runs `taplo format --check` over workspace-relative
# paths received as space-separated args.
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

TAPLO="$(rlocation _main/tools/lint/taplo_bin)"

files=()
for arg in "$@"; do
	for f in $arg; do
		files+=("$(rlocation "_main/${f#./}")")
	done
done

[[ ${#files[@]} -gt 0 ]] || exit 0
exec "$TAPLO" format --check "${files[@]}"
