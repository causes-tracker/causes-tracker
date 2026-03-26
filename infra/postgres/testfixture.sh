#!/usr/bin/env bash
# Bazel test fixture: starts a throwaway PostgreSQL instance.
# Source this file and call pg_start.
# Caller must source the Bazel runfiles library before calling pg_start.
# After pg_start exports: PGBIN, PGDATA, PGHOST, PGPORT,
#                         PGUSER, PGDATABASE, TEST_POSTGRES_URL.

pg_start() {
	local pg_root="${TEST_TMPDIR}/postgres"
	mkdir -p "$pg_root"
	tar -xzf "$(rlocation _main/infra/postgres/postgres.tar.gz)" -C "$pg_root"

	export PGBIN="${pg_root}/bin"
	export PGDATA="${TEST_TMPDIR}/pgdata"
	export PGUSER="postgres"
	export PGDATABASE="postgres"

	# Ask the OS for a free port by binding to 0, then release it.
	local port
	port=$(python3 -c \
		"import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); \
     p=s.getsockname()[1]; s.close(); print(p)")
	export PGPORT="$port"
	export PGHOST="127.0.0.1"
	export TEST_POSTGRES_URL="postgresql://postgres@127.0.0.1:${PGPORT}/postgres"

	# Initialise the data directory (trust auth so no passwords needed).
	"$PGBIN/initdb" -D "$PGDATA" --no-locale --encoding=UTF8 \
		-U postgres --auth=trust >/dev/null

	# Start the server listening on TCP only (no Unix socket needed in tests).
	local pglog="${TEST_TMPDIR}/pg.log"
	"$PGBIN/pg_ctl" start -D "$PGDATA" -l "$pglog" \
		-o "-p ${PGPORT} -h 127.0.0.1 -k ''" \
		--wait

	trap 'pg_stop' EXIT
}

pg_stop() {
	if [[ -n "${PGBIN:-}" && -n "${PGDATA:-}" ]]; then
		"$PGBIN/pg_ctl" stop -D "$PGDATA" -m fast -q 2>/dev/null || true
	fi
}
