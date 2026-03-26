#!/usr/bin/env bash
# Hermetic rustfmt wrapper.
# Usage: bazel run //tools:rustfmt -- <rustfmt args>
#   e.g. bazel run //tools:rustfmt -- --check services/causes_api/src/main.rs
#        bazel run //tools:rustfmt -- services/causes_api/src/**/*.rs
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

rustfmt_bin="$(rlocation rust_host_tools/bin/rustfmt)"
cd "${BUILD_WORKSPACE_DIRECTORY}"
exec "$rustfmt_bin" "$@"
