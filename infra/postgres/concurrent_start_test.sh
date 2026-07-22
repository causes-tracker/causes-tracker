#!/usr/bin/env bash
# Integration smoke test: starts several real postgres instances concurrently
# and checks every one comes up on a distinct port with no failures.
# port_retry_test.sh is what proves the retry-on-collision guarantee itself;
# this test guards against a regression that only shows up against a real
# server (e.g. a shared PGDATA path or a startup race under real timing).
# Run with: bazel test //infra/postgres:concurrent_start_test
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

testfixture="$(rlocation _main/infra/postgres/testfixture.sh)"
pg_bin="$(rlocation _main/infra/postgres/postgres_extracted)/bin"

instances=6

# Starts one instance in its own scratch dir, records the assigned port (or
# FAIL) to $2, then stops it. Runs in a subshell so each instance's exported
# PGBIN/PGDATA/PGPORT never leak into siblings running concurrently.
run_instance() {
	local work="$1" outfile="$2"
	(
		# shellcheck source=/dev/null
		source "$testfixture"
		if pg_start_in "$pg_bin" "$work"; then
			echo "$PGPORT" >"$outfile"
			"$PGBIN/pg_ctl" stop -D "$PGDATA" -m immediate -q 2>/dev/null || true
		else
			echo "FAIL" >"$outfile"
		fi
	)
}

root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
pids=()
for i in $(seq 1 "$instances"); do
	mkdir -p "$root/$i"
	run_instance "$root/$i" "$root/$i.port" &
	pids+=($!)
done

failed=0
for pid in "${pids[@]}"; do
	wait "$pid" || failed=1
done

ports=()
for i in $(seq 1 "$instances"); do
	port="$(cat "$root/$i.port")"
	if [[ "$port" == "FAIL" ]]; then
		echo >&2 "ERROR: instance $i failed to start"
		failed=1
		continue
	fi
	ports+=("$port")
done

if [[ "$failed" -ne 0 ]]; then
	exit 1
fi

unique_count="$(printf '%s\n' "${ports[@]}" | sort -u | wc -l)"
if [[ "$unique_count" -ne "$instances" ]]; then
	echo >&2 "ERROR: expected $instances distinct ports, got: ${ports[*]}"
	exit 1
fi

echo "OK: $instances concurrent instances started on distinct ports: ${ports[*]}"
