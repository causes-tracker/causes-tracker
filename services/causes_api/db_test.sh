#!/usr/bin/env bash
# Database integration tests for causes_api.
# Starts a hermetic PostgreSQL instance via the Bazel test fixture,
# then runs the compiled Rust test binary with DATABASE_URL set.
# $1 — path to the causes_api_test binary (supplied by sh_test args).
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

# shellcheck source=/dev/null
source "$(rlocation _main/infra/postgres/testfixture.sh)"
pg_start

test_binary="${1:?usage: db_test.sh <path-to-causes_api_test>}"
DATABASE_URL="$TEST_POSTGRES_URL" "$test_binary" "${@:2}"
