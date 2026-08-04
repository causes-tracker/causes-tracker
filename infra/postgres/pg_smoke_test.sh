#!/usr/bin/env bash
# Smoke test for the buck2 PostgreSQL fixture (see pg_fixture.sh).
# Starts a hermetic instance in the action and runs a trivial query, mirroring
# infra/postgres/testfixture_test.sh (the Bazel fixture's self-test).
#
# Invoked as: pg_smoke_test.sh <fixture.sh> <pg_shims> <musl_runtime> <out>
set -euo pipefail

fixture="$1"
export PG_BIN="$2/bin"
export PG_ICU_DATA="$3"
out="$4"

# shellcheck source=pg_fixture.sh
. "$fixture"

pg_start

case "$(trap -p EXIT)" in
*pg_stop*) ;;
*)
	echo >&2 "ERROR: pg_start did not arm pg_stop on EXIT"
	exit 1
	;;
esac

result="$("$PG_BIN/psql" -c 'SELECT 1 AS ok' -t -A)"
if [ "$result" != "1" ]; then
	echo >&2 "ERROR: SELECT 1 returned: $result"
	exit 1
fi

echo "OK: PostgreSQL is reachable; SELECT 1 returned 1" >"$out"
