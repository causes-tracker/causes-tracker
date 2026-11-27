#!/usr/bin/env bash
# Resolves a dependency-free package with the staged cargo.
#
# Invoked as: cargo_test.sh <rust_tools> <out>
set -euo pipefail

tools="$(cd "$1" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/package/src"
: >"$work/package/src/lib.rs"
cat >"$work/package/Cargo.toml" <<'EOF'
[package]
name = "resolved"
version = "0.0.0"
edition = "2024"
EOF

export CARGO_HOME="$work/home"
export RUSTC="$tools/bin/rustc"
got="$("$tools/bin/cargo" metadata --offline --format-version 1 \
	--manifest-path "$work/package/Cargo.toml")"
[[ "$got" == *'"name":"resolved"'* ]] || {
	echo >&2 "FAIL: cargo metadata printed: $got"
	exit 1
}
echo ok >"$2"
