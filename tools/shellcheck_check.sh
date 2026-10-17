#!/bin/sh
# buck2 action driver: shellcheck the scripts, then write the stamp that
# buck2 caches as the check's output.
set -eu
shellcheck="$1"
out="$2"
shift 2
"$shellcheck" "$@"
: >"$out"
