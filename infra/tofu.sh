#!/usr/bin/env bash
# Thin wrapper: runs OpenTofu from a specified root module directory.
# Usage via Bazel: bazel run //infra:tofu -- <module-dir> <tofu args>
# Example:         bazel run //infra:tofu -- infra/terraform plan
set -euo pipefail

# Standard Bazel 3-way runfiles init.
if [[ -f "${RUNFILES_DIR:-/dev/null}/bazel_tools/tools/bash/runfiles/runfiles.bash" ]]; then
	# shellcheck source=/dev/null
	source "${RUNFILES_DIR}/bazel_tools/tools/bash/runfiles/runfiles.bash"
elif [[ -f "${BASH_SOURCE[0]}.runfiles/bazel_tools/tools/bash/runfiles/runfiles.bash" ]]; then
	# shellcheck source=/dev/null
	source "${BASH_SOURCE[0]}.runfiles/bazel_tools/tools/bash/runfiles/runfiles.bash"
elif [[ -f "${RUNFILES_MANIFEST_FILE:-/dev/null}" ]]; then
	# shellcheck source=/dev/null
	source "$(grep -m1 "^bazel_tools/tools/bash/runfiles/runfiles.bash " \
		"$RUNFILES_MANIFEST_FILE" | cut -d ' ' -f2-)"
else
	echo >&2 "ERROR: cannot find Bazel runfiles library"
	exit 1
fi

tofu_bin="$(rlocation opentofu_linux_amd64/tofu)"

if [[ $# -eq 0 ]]; then
	echo >&2 "Usage: bazel run //infra:tofu -- <module-dir> <tofu-args...>"
	echo >&2 "Example: bazel run //infra:tofu -- infra/terraform plan"
	exit 1
fi
root_module="$1"
shift

cd "${BUILD_WORKSPACE_DIRECTORY}/${root_module}"
exec "$tofu_bin" "$@"
