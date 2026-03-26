#!/usr/bin/env bash
# Self-test for the PostgreSQL test fixture.
# Verifies that pg_start brings up a reachable PostgreSQL instance.
# Run with: bazel test //infra/postgres:testfixture_test
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

# Verify the instance is reachable.
result="$("$PGBIN/psql" -c 'SELECT 1 AS ok' -t -A 2>&1)"
if [[ "$result" != "1" ]]; then
	echo >&2 "ERROR: SELECT 1 returned: $result"
	exit 1
fi

echo "OK: PostgreSQL is reachable; SELECT 1 returned 1"
