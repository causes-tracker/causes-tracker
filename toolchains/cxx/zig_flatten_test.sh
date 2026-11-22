#!/usr/bin/env bash
# Checks zig_flatten.sh against a fake zig that records how it was called.
#
# Invoked as: zig_flatten_test.sh <zig_flatten.sh> <out>
set -euo pipefail

flatten="$1"
out="$2"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export TMPDIR="$work/tmp"
export RECORD="$work/record"
mkdir "$TMPDIR" "$RECORD" "$work/out"

# Records its argv, the argsfile it was handed and its cache directories.
fake="$work/zig"
cat >"$fake" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" >"$RECORD/argv"
printf '%s\n' "$ZIG_GLOBAL_CACHE_DIR" "$ZIG_LOCAL_CACHE_DIR" >"$RECORD/env"
case "$2" in
@*) cat "${2#@}" >"$RECORD/args" ;;
esac
exit "${FAKE_RC:-0}"
EOF
chmod +x "$fake"

fail() {
	echo >&2 "FAIL: $*"
	exit 1
}

# One argsfile that @-includes another; the top one ends without a newline,
# as buck2's do, and names the -o output; the included one carries a
# shell-quoted line as buck2's cxx argsfiles do, which zig's tokenizer reads
# as one token.
printf -- "-Wall\\n-I\\ninclude\\n'-DSUB=one two'\\n" >"$work/sub"
printf -- '-target\nx86_64-linux-musl\n@%s\nsrc.c\n-o\n%s' "$work/sub" "$work/out/src.o" >"$work/top"

/bin/sh "$flatten" "$fake" cc -v "@$work/top" "-DGREETING=hello there" "#not-a-comment" '-DMSG="hi"' "-DDIR=a\\" '-DRAW=a\b'

argv="$(cat "$RECORD/argv")"
case "$argv" in
"cc
@"*) ;;
*) fail "zig got argv $argv, expected cc and one argsfile" ;;
esac

# Quoted as zig's tokenizer reads them: a backslash run before a quote is
# doubled and the quote escaped; a backslash elsewhere stays single.
expected="$(printf -- '-v\n-target\nx86_64-linux-musl\n-Wall\n-I\ninclude\n'"'"'-DSUB=one two'"'"'\nsrc.c\n-o\n%s\n"-DGREETING=hello there"\n"#not-a-comment"\n"-DMSG=\\"hi\\""\n"-DDIR=a\\\\"\n"-DRAW=a\\b"' "$work/out/src.o")"
args="$(cat "$RECORD/args")"
[ "$args" = "$expected" ] || fail "flattened args were:
$args
expected:
$expected"

global="$(sed -n 1p "$RECORD/env")"
local_cache="$(sed -n 2p "$RECORD/env")"
case "$global" in
"$TMPDIR"/*) ;;
*) fail "global cache $global is not under TMPDIR" ;;
esac
case "$local_cache" in
"$work/out/."*) ;;
*) fail "local cache $local_cache is not beside the -o output" ;;
esac
[ -z "$(ls -A "$TMPDIR")" ] || fail "scratch left in TMPDIR: $(ls -A "$TMPDIR")"
[ -z "$(ls -A "$work/out")" ] || fail "cache left beside the output: $(ls -A "$work/out")"

# Without -o the local cache stays in the scratch.
/bin/sh "$flatten" "$fake" cc --version
local_cache="$(sed -n 2p "$RECORD/env")"
case "$local_cache" in
"$TMPDIR"/*) ;;
*) fail "local cache $local_cache is not under TMPDIR without -o" ;;
esac

# zig's exit status is the script's, and both caches are still removed.
rc=0
FAKE_RC=3 /bin/sh "$flatten" "$fake" cc -c src.c -o "$work/out/src.o" || rc=$?
[ "$rc" = 3 ] || fail "exit status $rc, expected zig's 3"
[ -z "$(ls -A "$TMPDIR")" ] || fail "scratch left after a failure: $(ls -A "$TMPDIR")"
[ -z "$(ls -A "$work/out")" ] || fail "cache left beside the output after a failure: $(ls -A "$work/out")"

# An argument the response file cannot carry is refused before zig runs.
rm "$RECORD/argv"
rc=0
/bin/sh "$flatten" "$fake" cc "-DQUOTE='" 2>/dev/null || rc=$?
[ "$rc" = 1 ] || fail "exit status $rc for a quoted argument, expected 1"
[ ! -e "$RECORD/argv" ] || fail "zig ran despite a refused argument"

echo ok >"$out"
