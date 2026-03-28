#!/usr/bin/env bash
# Hermetic pymarkdown wrapper for Bazel lint tests.
# Runs pymarkdown scan on all arguments, using the config file at $1.
set -euo pipefail

# Bootstrap the Bazel runfiles library via RUNFILES_DIR (set by both
# sh_binary wrappers and the sh_test runner).  Try rules_shell first;
# fall back to the legacy bazel_tools location.
# shellcheck source=/dev/null
if [[ -f "${RUNFILES_DIR:-/dev/null}/rules_shell/shell/runfiles/runfiles.bash" ]]; then
	source "${RUNFILES_DIR}/rules_shell/shell/runfiles/runfiles.bash"
elif [[ -f "${RUNFILES_DIR:-/dev/null}/bazel_tools/tools/bash/runfiles/runfiles.bash" ]]; then
	source "${RUNFILES_DIR}/bazel_tools/tools/bash/runfiles/runfiles.bash"
else
	echo >&2 "ERROR: cannot find runfiles.bash"
	exit 1
fi

PYMARKDOWN="$(rlocation _main/tools/lint/pymarkdown)"
CONFIG="$(rlocation _main/.pymarkdown.json)"

exec "$PYMARKDOWN" --config "$CONFIG" scan "$@"
