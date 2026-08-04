#!/bin/sh
# Smoke test for the buck2 PostgreSQL fixture (see pg_fixture.sh).
# Starts a hermetic instance in the action and runs a trivial query, mirroring
# infra/postgres/testfixture_test.sh (the Bazel fixture's self-test).
#
# Invoked as: pg_smoke_test.sh <fixture.sh> <pg_bin> <musl_runtime> <out>
set -eu

fixture="$1"
export PG_BIN="$2"
export PG_MUSL_RUNTIME="$3"
out="$4"

export PG_WORKDIR="$TMPDIR/pg_smoke"

# shellcheck source=pg_fixture.sh
. "$fixture"

trap pg_stop EXIT INT TERM
pg_start

result="$("$PG_BIN/psql" -c 'SELECT 1 AS ok' -t -A)"
if [ "$result" != "1" ]; then
	echo >&2 "ERROR: SELECT 1 returned: $result"
	exit 1
fi

echo "OK: PostgreSQL is reachable; SELECT 1 returned 1" >"$out"
