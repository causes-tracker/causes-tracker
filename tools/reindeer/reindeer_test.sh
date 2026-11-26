#!/usr/bin/env bash
# Runs the pinned reindeer and checks it lists its buckify subcommand.
#
# Invoked as: reindeer_test.sh <reindeer> <out>
set -euo pipefail

got="$("$1" --help)"
grep -q '^  buckify ' <<<"$got" || {
	echo >&2 "FAIL: reindeer --help printed: $got"
	exit 1
}
echo ok >"$2"
