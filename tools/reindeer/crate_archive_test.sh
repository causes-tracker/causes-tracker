#!/usr/bin/env bash
# Checks the crate sits at the root of a directory named after its target.
#
# Invoked as: crate_archive_test.sh <cfg-if-1.0.5.crate> <out>
set -euo pipefail

[ "$(basename "$1")" = "cfg-if-1.0.5.crate" ] || {
	echo >&2 "FAIL: unpacked into $1"
	exit 1
}
grep -q '^name = "cfg-if"$' "$1/Cargo.toml" || {
	echo >&2 "FAIL: no cfg-if manifest at $1/Cargo.toml"
	exit 1
}
[ -f "$1/src/lib.rs" ] || {
	echo >&2 "FAIL: no $1/src/lib.rs"
	exit 1
}
echo ok >"$2"
