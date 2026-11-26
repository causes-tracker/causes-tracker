#!/usr/bin/env bash
# Runs the scripted binary and checks its build script reached rustc.
# The build script itself asserts it reached the C compiler.
#
# Invoked as: scripted_test.sh <scripted> <out>
set -euo pipefail

got="$("$1")"
[[ "$got" == "built by rustc "[0-9]* ]] || {
	echo >&2 "FAIL: scripted printed: $got"
	exit 1
}
echo ok >"$2"
