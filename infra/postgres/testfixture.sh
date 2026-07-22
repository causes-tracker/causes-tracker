#!/usr/bin/env bash
# Bazel test fixture: starts a throwaway PostgreSQL instance.
#
# sh_test/sh_binary consumers (runfiles + TEST_TMPDIR available): source this
# file and call pg_start.  Caller must source the Bazel runfiles library
# first.  After pg_start exports: PGBIN, PGDATA, PGHOST, PGPORT,
#                                 PGUSER, PGDATABASE, TEST_POSTGRES_URL.
#
# Build-action consumers (no runfiles, no TEST_TMPDIR): source this file by
# its plain input path and call pg_start_in "$pg_bin_dir" "$writable_dir"
# directly; it exports the same variables without touching rlocation or
# TEST_TMPDIR.

# Starts postgres rooted at $2 (a writable directory) using the install at
# $1 (a PGBIN dir). Tries candidate ports until one binds: a bind collision
# is the only failure treated as retryable, so a genuine startup error still
# fails on the first attempt with the postgres log attached.
pg_start_in() {
	PGBIN="$1"
	PGDATA="$2/pgdata"
	export PGBIN PGDATA
	export PGUSER="postgres"
	export PGDATABASE="postgres"

	# Initialise the data directory (trust auth so no passwords needed).
	"$PGBIN/initdb" -D "$PGDATA" --no-locale --encoding=UTF8 \
		-U postgres --auth=trust >/dev/null

	# Start the server listening on TCP only (no Unix socket needed in tests,
	# which also sidesteps sandbox tmpdir path-length limits on socket paths).
	# Use mmap for dynamic shared memory so nothing lands in /dev/shm.
	# Ephemeral test instances that crash leave POSIX segments behind in
	# /dev/shm; with mmap the segments live inside PGDATA and vanish when the
	# caller's directory is cleaned up.
	local pglog="$2/pg.log"
	local candidate attempt max_attempts=10
	for attempt in $(seq 1 "$max_attempts"); do
		# RANDOM only ranges 0-32767, so candidates cover 20000-52767.
		candidate=$((20000 + RANDOM))
		# Truncated per attempt: pg_ctl -l appends, and a stale collision
		# message from an earlier attempt must not shadow this attempt's
		# real outcome in the check below.
		: >"$pglog"
		# LC_ALL=C pins postgres's log wording to English so the collision
		# check below matches regardless of the invoking environment's locale.
		if LC_ALL=C "$PGBIN/pg_ctl" start -D "$PGDATA" -l "$pglog" \
			-o "-p ${candidate} -h 127.0.0.1 -k '' -c dynamic_shared_memory_type=mmap" \
			--wait 2>/dev/null; then
			export PGPORT="$candidate"
			export PGHOST="127.0.0.1"
			export TEST_POSTGRES_URL="postgresql://postgres@127.0.0.1:${PGPORT}/postgres"
			return 0
		fi
		if ! grep -q "Address already in use" "$pglog" 2>/dev/null; then
			echo >&2 "ERROR: postgres failed to start (not a port collision):"
			cat "$pglog" >&2
			return 1
		fi
	done
	echo >&2 "ERROR: no free port found in ${max_attempts} attempts"
	cat "$pglog" >&2
	return 1
}

pg_start() {
	pg_start_in "$(rlocation _main/infra/postgres/postgres_extracted)/bin" "${TEST_TMPDIR}"
	trap 'pg_stop' EXIT INT TERM
}

pg_stop() {
	if [[ -n "${PGBIN:-}" && -n "${PGDATA:-}" ]]; then
		"$PGBIN/pg_ctl" stop -D "$PGDATA" -m immediate -q 2>/dev/null || true
	fi
}
