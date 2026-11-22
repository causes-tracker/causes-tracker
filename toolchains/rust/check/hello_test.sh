#!/usr/bin/env bash
# Runs the hello binary and checks the library's greeting came through.
#
# Invoked as: hello_test.sh <hello> <out>
set -euo pipefail

got="$("$1")"
[ "$got" = "hello from rustc" ] || {
	echo >&2 "FAIL: hello printed: $got"
	exit 1
}
echo ok >"$2"
