#!/usr/bin/env bash
# Checks rustc_wrapper.sh against a fake rustc that records how it was called
# and writes the outputs its argsfile names.
#
# Invoked as: rustc_wrapper_test.sh <rustc_wrapper.sh> <out>
set -euo pipefail

wrapper="$1"
out="$2"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export TMPDIR="$work/tmp"
export RECORD="$work/record"
mkdir "$TMPDIR" "$RECORD" "$work/bashbin" "$work/declared" "$work/declared2"

# Records PATH, argv and the argsfile; writes a file into --out-dir and the
# --emit path.
fake="$work/rustc"
cat >"$fake" <<'EOF'
#!/bin/sh
printf '%s\n' "$PATH" >"$RECORD/path"
printf '%s\n' "$@" >"$RECORD/argv"
for a in "$@"; do
	case "$a" in
	@*)
		cp "${a#@}" "$RECORD/args"
		while IFS= read -r line; do
			case "$line" in
			--out-dir=*) mkdir -p "${line#--out-dir=}" && echo extra >"${line#--out-dir=}/extra.txt" ;;
			--emit=*=*) p="${line#--emit=*=}" && echo linked >"$p" ;;
			esac
		done <"${a#@}"
		;;
	esac
done
exit "${FAKE_RC:-0}"
EOF
chmod +x "$fake"

fail() {
	echo >&2 "FAIL: $*"
	exit 1
}

argsfile() {
	printf -- '--crate-name\ngreet\n--emit=link=%s/libgreet.rlib\n--out-dir=%s/extras\n' "$1" "$1" >"$work/args"
}

argsfile "$work/declared"
/bin/sh "$wrapper" "$work/bashbin" "$fake" -v "@$work/args"

case "$(cat "$RECORD/path")" in
"$work/bashbin:/bin:"*) ;;
*) fail "PATH $(cat "$RECORD/path") does not start with the bash directory and /bin" ;;
esac
case "$(cat "$RECORD/argv")" in
"-v
@"*) ;;
*) fail "rustc got argv $(cat "$RECORD/argv"), expected -v and one argsfile" ;;
esac

# rustc saw the flags unchanged except for the two output paths, which point
# into the scratch.
[ "$(sed -n 1,2p "$RECORD/args")" = "--crate-name
greet" ] || fail "non-output flags were rewritten: $(cat "$RECORD/args")"
case "$(sed -n 3p "$RECORD/args")" in
"--emit=link=$TMPDIR/"*) ;;
*) fail "--emit not redirected into the scratch: $(sed -n 3p "$RECORD/args")" ;;
esac
case "$(sed -n 4p "$RECORD/args")" in
"--out-dir=$TMPDIR/"*) ;;
*) fail "--out-dir not redirected into the scratch: $(sed -n 4p "$RECORD/args")" ;;
esac
[ "$(cat "$work/declared/libgreet.rlib")" = linked ] || fail "--emit output not copied to the declared path"
[ "$(cat "$work/declared/extras/extra.txt")" = extra ] || fail "--out-dir contents not copied to the declared path"
[ -z "$(ls -A "$TMPDIR")" ] || fail "scratch left in TMPDIR: $(ls -A "$TMPDIR")"

# rustc's exit status is the script's; nothing is copied and the scratch is
# removed.
argsfile "$work/declared2"
rc=0
FAKE_RC=101 /bin/sh "$wrapper" "$work/bashbin" "$fake" "@$work/args" || rc=$?
[ "$rc" = 101 ] || fail "exit status $rc, expected rustc's 101"
[ -z "$(ls -A "$work/declared2")" ] || fail "outputs copied after a failure: $(ls -A "$work/declared2")"
[ -z "$(ls -A "$TMPDIR")" ] || fail "scratch left after a failure: $(ls -A "$TMPDIR")"

# Without an argsfile the arguments pass through untouched.
/bin/sh "$wrapper" "$work/bashbin" "$fake" --version
[ "$(cat "$RECORD/argv")" = "--version" ] || fail "argv without an argsfile was $(cat "$RECORD/argv")"

echo ok >"$out"
