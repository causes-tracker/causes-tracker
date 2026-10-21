#!/bin/sh
# buck2 action fixture: starts a throwaway PostgreSQL instance inside the
# action's sandbox.
# Source this file, call pg_start; call pg_stop (or let the caller's own trap
# do it) before the action exits.
#
# Caller must export before sourcing:
#   PG_BIN           - directory of the postgres programs (initdb, pg_ctl, psql)
#   PG_MUSL_RUNTIME  - the staged musl runtime dir, holding the ICU data
#   PG_WORKDIR       - a scratch dir for pgdata and pg.log; created if absent
#
# pg_start exports: PGDATA, PGHOST (a unix socket dir), PGPORT, PGUSER,
# PGDATABASE.

# buck2 hands this in as a path relative to the action's cwd, but the
# postmaster chdir()s into PGDATA at startup and backends read ICU_DATA after
# that, so resolve it up front.
PG_MUSL_RUNTIME="$(cd "$PG_MUSL_RUNTIME" && pwd)"
mkdir -p "$PG_WORKDIR"

# Alpine's icu-libs ships a data-less libicudata.so stub; the collation data
# is in the runtime.
# ICU_DATA is the directory to load it from - without it initdb fails after
# bootstrap, at `UPDATE pg_collation SET collversion` for the unicode
# collation.
export ICU_DATA="$PG_MUSL_RUNTIME"

pg_start() {
	export PGDATA="$PG_WORKDIR/pgdata"
	export PGUSER="postgres"
	export PGDATABASE="postgres"
	# Unix socket paths cap at 107 bytes; $PG_WORKDIR (under the action's
	# scratch path) routinely blows past that, so the socket dir lives under /tmp.
	PGHOST="$(mktemp -d /tmp/pgsock.XXXXXX)"
	export PGHOST

	# PGPORT names the socket file (.s.PGSQL.<port>); the server listens on the
	# unix socket only.
	export PGPORT="5432"

	"$PG_BIN/initdb" -D "$PGDATA" --no-locale --encoding=UTF8 -U postgres \
		--auth=trust >/dev/null

	# mmap dynamic shared memory so nothing lands in /dev/shm.
	"$PG_BIN/pg_ctl" start -D "$PGDATA" -l "$PG_WORKDIR/pg.log" \
		-o "-h '' -p $PGPORT -k $PGHOST -c dynamic_shared_memory_type=mmap" \
		--wait || {
		cat "$PG_WORKDIR/pg.log" >&2
		return 1
	}
}

pg_stop() {
	if [ -n "${PGDATA:-}" ]; then
		# pg_ctl stop waits for shutdown, so a clean exit means it is down.
		# Guard on status so a repeated trap is a no-op.
		if "$PG_BIN/pg_ctl" status -D "$PGDATA" >/dev/null 2>&1; then
			"$PG_BIN/pg_ctl" stop -D "$PGDATA" -m immediate -s
		fi
	fi
}
