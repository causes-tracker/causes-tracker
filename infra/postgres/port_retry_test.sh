#!/usr/bin/env bash
# Unit test for pg_start_in's port-retry loop, using fake initdb/pg_ctl
# doubles (testdata/fake_pg/) so the retry/bail-out logic is verified
# deterministically, without a real postgres server or real network ports.
# Run with: bazel test //infra/postgres:port_retry_test
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

fake_pg_bin="$(dirname "$(rlocation _main/infra/postgres/testdata/fake_pg/pg_ctl)")"

# Runs pg_start_in against the fake bin with $1 as the newline-separated
# outcome script, in a fresh scratch dir. Sets $rc and $attempts.
run_scenario() {
	local outcomes="$1"
	local work
	work="$(mktemp -d)"
	export FAKE_PG_CTL_OUTCOMES="$work/outcomes"
	export FAKE_PG_CTL_COUNTER="$work/counter"
	printf '%s\n' "$outcomes" >"$FAKE_PG_CTL_OUTCOMES"

	rc=0
	pg_start_in "$fake_pg_bin" "$work/instance" >"$work/stdout" 2>"$work/stderr" || rc=$?
	attempts="$(cat "$FAKE_PG_CTL_COUNTER" 2>/dev/null || echo 0)"
}

# Scenario 1: first candidate binds — succeeds in one attempt.
run_scenario "ok"
if [[ "$rc" -ne 0 || "$attempts" -ne 1 ]]; then
	echo >&2 "FAIL: expected success in 1 attempt, got rc=$rc attempts=$attempts"
	exit 1
fi
echo "OK: succeeds on first available port"

# Scenario 2: one port collision, then success — proves the retry loop
# treats a bind collision as retryable and recovers.
run_scenario $'collision\nok'
if [[ "$rc" -ne 0 || "$attempts" -ne 2 ]]; then
	echo >&2 "FAIL: expected success after 1 retry, got rc=$rc attempts=$attempts"
	exit 1
fi
echo "OK: retries past a port collision and succeeds"

# Scenario 3: a non-collision startup failure must not be retried — it is a
# real error, and masking it behind port-retry logic would hide misconfigured
# postgres flags as flaky port exhaustion.
run_scenario "other"
if [[ "$rc" -eq 0 || "$attempts" -ne 1 ]]; then
	echo >&2 "FAIL: expected immediate failure on non-port error, got rc=$rc attempts=$attempts"
	exit 1
fi
echo "OK: fails fast on a non-port startup error, without retrying"

# Scenario 4: only port collisions, forever — bounded by max_attempts (10)
# rather than looping indefinitely.
run_scenario "$(for _ in $(seq 1 20); do echo collision; done)"
if [[ "$rc" -eq 0 || "$attempts" -ne 10 ]]; then
	echo >&2 "FAIL: expected bail-out after 10 attempts, got rc=$rc attempts=$attempts"
	exit 1
fi
echo "OK: bounds retries to 10 attempts when every port collides"

# Scenario 5: a collision, then a genuine error — the collision message from
# attempt 1 must not leak into attempt 2's check (pg_ctl -l appends to the
# same log), so this must fail on attempt 2 rather than being misread as
# another collision and retried.
run_scenario $'collision\nother'
if [[ "$rc" -eq 0 || "$attempts" -ne 2 ]]; then
	echo >&2 "FAIL: expected failure on attempt 2, got rc=$rc attempts=$attempts"
	exit 1
fi
echo "OK: a real error after a collision is not masked by the stale log entry"

echo "OK: all port-retry scenarios passed"
