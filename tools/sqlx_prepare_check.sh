#!/usr/bin/env bash
# Golden diff for captured sqlx metadata.
# Fails if the captured `.sqlx` TreeArtifact differs from the committed one.
#
# $1 — rlocation path to the captured `.sqlx` TreeArtifact
# $2 — package path of the crate (its committed `.sqlx/` lives at
#      $TEST_WORKSPACE/$2/.sqlx)
# $3 — name of the `bazel run` target that regenerates the committed files
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

captured="$(rlocation "${1:?usage: sqlx_prepare_check.sh <captured-rlocation> <pkg> <update-target>}")"
pkg="${2:?usage: sqlx_prepare_check.sh <captured-rlocation> <pkg> <update-target>}"
update="${3:?usage: sqlx_prepare_check.sh <captured-rlocation> <pkg> <update-target>}"
committed="$(rlocation "${TEST_WORKSPACE}/${pkg}/.sqlx")"

if [[ ! -d "$captured" ]]; then
	echo >&2 "ERROR: captured directory not found: $captured"
	exit 1
fi
if [[ ! -d "$committed" ]]; then
	echo >&2 "ERROR: committed .sqlx directory not found: $committed"
	exit 1
fi

if ! diff -r "$committed" "$captured"; then
	echo >&2 ""
	echo >&2 "Committed .sqlx is stale versus the live-schema capture."
	echo >&2 "Regenerate it from //${pkg}:${update} and re-commit."
	exit 1
fi
