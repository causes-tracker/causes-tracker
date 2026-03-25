#!/usr/bin/env bash
# Runs the bundled psql binary.  Use via: bazel run //infra/postgres:psql -- <args>
set -euo pipefail

# Standard Bazel 3-way runfiles init.
if [[ -f "${RUNFILES_DIR:-/dev/null}/bazel_tools/tools/bash/runfiles/runfiles.bash" ]]; then
  source "${RUNFILES_DIR}/bazel_tools/tools/bash/runfiles/runfiles.bash"
elif [[ -f "${BASH_SOURCE[0]}.runfiles/bazel_tools/tools/bash/runfiles/runfiles.bash" ]]; then
  source "${BASH_SOURCE[0]}.runfiles/bazel_tools/tools/bash/runfiles/runfiles.bash"
elif [[ -f "${RUNFILES_MANIFEST_FILE:-/dev/null}" ]]; then
  source "$(grep -m1 "^bazel_tools/tools/bash/runfiles/runfiles.bash " \
    "$RUNFILES_MANIFEST_FILE" | cut -d ' ' -f2-)"
else
  echo >&2 "ERROR: cannot find Bazel runfiles library"; exit 1
fi

pg_tarball="$(rlocation _main/infra/postgres/postgres.tar.gz)"

# Use a stable extraction dir so repeated runs are instant.
pg_cache="${XDG_CACHE_HOME:-$HOME/.cache}/causes-postgres-bin"
if [[ ! -x "$pg_cache/bin/psql" ]]; then
  mkdir -p "$pg_cache"
  tar -xzf "$pg_tarball" -C "$pg_cache"
fi

exec "$pg_cache/bin/psql" "$@"
