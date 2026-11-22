#!/usr/bin/env bash
# buck2 action fixture: starts a throwaway PostgreSQL instance inside the
# action's sandbox, with pgdata and the server log under $TMPDIR.
# Source this file and call pg_start; it arms pg_stop on EXIT, INT and TERM.
#
# Caller must export before sourcing:
#   PG_BIN       - directory of the postgres programs (initdb, pg_ctl, psql)
#   PG_ICU_DATA  - directory holding the ICU data file
#
# pg_start exports: PGDATA, PGHOST (a unix socket dir), PGPORT, PGUSER,
# PGDATABASE.

# buck2 hands this in as a path relative to the action's cwd, but the
# postmaster chdir()s into PGDATA at startup and backends read ICU_DATA after
# that, so resolve it up front.
# Without it initdb fails after bootstrap, at `UPDATE pg_collation SET
# collversion` for the unicode collation.
ICU_DATA="$(cd "$PG_ICU_DATA" && pwd)"
export ICU_DATA

pg_start() {
	export PGDATA="$TMPDIR/pgdata"
	export PGUSER="postgres"
	export PGDATABASE="postgres"
	# Unix socket paths cap at 107 bytes, which a path under $TMPDIR exceeds;
	# /tmp is private to the action, so the socket dir lives there.
	PGHOST="$(mktemp -d /tmp/pgsock.XXXXXX)"
	export PGHOST

	# PGPORT names the socket file (.s.PGSQL.<port>); the server listens on the
	# unix socket only.
	export PGPORT="5432"

	"$PG_BIN/initdb" -D "$PGDATA" --no-locale --encoding=UTF8 -U postgres \
		--auth=trust >/dev/null

	# mmap dynamic shared memory so nothing lands in /dev/shm.
	"$PG_BIN/pg_ctl" start -D "$PGDATA" -l "$TMPDIR/pg.log" \
		-o "-h '' -p $PGPORT -k $PGHOST -c dynamic_shared_memory_type=mmap" \
		--wait || {
		cat "$TMPDIR/pg.log" >&2
		return 1
	}

	trap pg_stop EXIT INT TERM
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
