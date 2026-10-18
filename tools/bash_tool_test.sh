#!/usr/bin/env bash
# The bash running this script is the runtime's, not a host bash: $BASH is the
# launched program's path (/proc is unreadable in the worker sandbox).
# Usage: bash_tool_test.sh <out>
set -euo pipefail

[[ "$BASH" == */musl_runtime/bin/bash ]] || {
	echo "error: interpreter is $BASH" >&2
	exit 1
}
: >"$1"
