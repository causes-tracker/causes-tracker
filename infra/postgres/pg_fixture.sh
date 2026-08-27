#!/bin/sh
# buck2 action fixture: starts a throwaway PostgreSQL instance inside the
# action's sandbox.
# Source this file, call pg_start, run binaries through pg_run; call pg_stop
# (or let the caller's own trap do it) before the action exits.
#
# postgres is invoked through an explicit musl loader with --library-path
# rather than its embedded /lib/ld-musl-x86_64.so.1 interpreter, so it does not
# depend on the worker image carrying a loader at that path.
#
# Caller must export before sourcing:
#   PG_DIST          - extracted postgres tree (has bin/, lib/)
#   PG_MUSL_RUNTIME  - the staged musl runtime dir: the loader, libc, and the
#                      libraries postgres links
#   PG_WORKDIR       - a scratch dir for pgdata and pg.log; created if absent
#
# pg_start exports: PGBIN, PGDATA, PGHOST (a unix socket dir), PGPORT, PGUSER,
# PGDATABASE.

# buck2 hands these in as paths relative to the action's cwd, but the
# postmaster chdir()s into PGDATA at startup; anything a running backend reads
# later (ICU_DATA) must survive that chdir, so resolve them up front.
PG_DIST="$(cd "$PG_DIST" && pwd)"
PG_MUSL_RUNTIME="$(cd "$PG_MUSL_RUNTIME" && pwd)"
mkdir -p "$PG_WORKDIR"
PG_WORKDIR="$(cd "$PG_WORKDIR" && pwd)"

# initdb and the postmaster exec further postgres processes that inherit the
# environment but not a top-level --library-path, so LD_LIBRARY_PATH also has
# to carry the runtime for the dynamic linker those child execs go through.
export LD_LIBRARY_PATH="$PG_DIST/lib:$PG_MUSL_RUNTIME"

# Alpine's icu-libs ships a data-less libicudata.so stub; the collation tables
# are icu-data-en's icudt74l.dat in the runtime.
# ICU_DATA is the directory to load it from - without it initdb's bootstrap
# fails creating the default ICU collation entry even under the libc locale
# provider.
export ICU_DATA="$PG_MUSL_RUNTIME"

pg_run() {
	"$PG_MUSL_RUNTIME/ld-musl-x86_64.so.1" \
		--library-path "$LD_LIBRARY_PATH" \
		"$@"
}

pg_start() {
	export PGBIN="$PG_DIST/bin"
	export PGDATA="$PG_WORKDIR/pgdata"
	export PGUSER="postgres"
	export PGDATABASE="postgres"
	# Unix socket paths cap at 107 bytes; $PG_WORKDIR (under buck-out's cache
	# path) routinely blows past that, so the socket dir lives under /tmp,
	# PID-keyed to stay unique across concurrent actions on the same worker.
	export PGHOST="/tmp/pgsock.$$"
	mkdir -p "$PGHOST"

	# PGPORT names the socket file (.s.PGSQL.<port>); the server listens on the
	# unix socket only.
	export PGPORT="5432"

	# --locale-provider=libc: ICU's default collation provider fails "could
	# not open collator for locale und" (the stub libicudata, see ICU_DATA
	# above); libc collation needs no ICU data.
	pg_run "$PGBIN/initdb" -D "$PGDATA" --no-locale --encoding=UTF8 \
		--locale-provider=libc -U postgres --auth=trust >/dev/null

	# mmap dynamic shared memory so nothing lands in /dev/shm.
	pg_run "$PGBIN/pg_ctl" start -D "$PGDATA" -l "$PG_WORKDIR/pg.log" \
		-o "-h '' -p $PGPORT -k $PGHOST -c dynamic_shared_memory_type=mmap" \
		--wait || {
		cat "$PG_WORKDIR/pg.log" >&2
		return 1
	}
}

pg_stop() {
	if [ -n "${PGBIN:-}" ] && [ -n "${PGDATA:-}" ]; then
		# pg_ctl stop waits for shutdown, so a clean exit means it is down.
		# Guard on status so a repeated trap is a no-op.
		if pg_run "$PGBIN/pg_ctl" status -D "$PGDATA" >/dev/null 2>&1; then
			pg_run "$PGBIN/pg_ctl" stop -D "$PGDATA" -m immediate -s
		fi
	fi
	# /tmp is shared by concurrent actions; PGHOST is PID-keyed but still needs
	# cleaning up so it doesn't accumulate.
	if [ -n "${PGHOST:-}" ]; then
		rm -rf "$PGHOST"
	fi
}
