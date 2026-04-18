#!/usr/bin/env bash
# Bazel sh_test driver: runs rustfmt --check over arg files.
# Args are workspace-relative paths ($(rootpath) from the macro).
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

RUSTFMT="$(rlocation rust_host_tools/bin/rustfmt)"
CONFIG_DIR="$(dirname "$(rlocation _main/rustfmt.toml)")"

if [[ $# -eq 0 ]]; then
	exit 0
fi

files=()
for arg in "$@"; do
	f="$(rlocation "_main/$arg")" || {
		echo >&2 "ERROR: cannot resolve $arg via rlocation"
		exit 1
	}
	files+=("$f")
done

exec "$RUSTFMT" --check --config-path="$CONFIG_DIR" "${files[@]}"
