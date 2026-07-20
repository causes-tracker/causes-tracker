#!/usr/bin/env bash
# Test driver for the diff-gating logic in update_buck_shas.sh.
# Sources the script (with _UPDATE_BUCK_SHAS_SOURCED=1 to skip main) and checks
# that `changed_names` selects exactly the pins whose URL differs from the base
# — the property that keeps an unchanged pin's reviewed sha from being relocked.
set -uo pipefail

if [[ -f "${RUNFILES_DIR:-/dev/null}/bazel_tools/tools/bash/runfiles/runfiles.bash" ]]; then
	# shellcheck source=/dev/null
	source "${RUNFILES_DIR}/bazel_tools/tools/bash/runfiles/runfiles.bash"
elif [[ -f "${BASH_SOURCE[0]}.runfiles/bazel_tools/tools/bash/runfiles/runfiles.bash" ]]; then
	# shellcheck source=/dev/null
	source "${BASH_SOURCE[0]}.runfiles/bazel_tools/tools/bash/runfiles/runfiles.bash"
elif [[ -f "${RUNFILES_MANIFEST_FILE:-/dev/null}" ]]; then
	# shellcheck source=/dev/null
	source "$(grep -m1 "^bazel_tools/tools/bash/runfiles/runfiles.bash " \
		"$RUNFILES_MANIFEST_FILE" | cut -d ' ' -f2-)"
else
	echo >&2 "ERROR: cannot find Bazel runfiles library"
	exit 1
fi

SCRIPT="$(rlocation _main/tools/renovate/update_buck_shas.sh)"
if [[ ! -f "$SCRIPT" ]]; then
	echo "ERROR: cannot locate update_buck_shas.sh via runfiles" >&2
	exit 1
fi

# shellcheck source=/dev/null
_UPDATE_BUCK_SHAS_SOURCED=1 source "$SCRIPT"

PASS=0
FAIL=0
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

expect() {
	local name="$1" want="$2" got="$3"
	if [[ "$got" == "$want" ]]; then
		echo "PASS: $name"
		((PASS++)) || true
	else
		echo "FAIL: $name — want [$want], got [$got]"
		((FAIL++)) || true
	fi
}

names() { changed_names "$1" "$2" | sort | tr '\n' ' '; }

cat >"$tmp/base" <<'EOF'
foo_url = "https://example/foo/v1/a.tgz"
foo_sha256 = "aaaa"
bar_url = "https://example/bar/v9/b.tgz"
bar_sha256 = "bbbb"
EOF

# One URL bumped, the other untouched.
cat >"$tmp/work" <<'EOF'
foo_url = "https://example/foo/v2/a.tgz"
foo_sha256 = "aaaa"
bar_url = "https://example/bar/v9/b.tgz"
bar_sha256 = "bbbb"
EOF
expect "only the changed url is selected" "foo " "$(names "$tmp/work" "$tmp/base")"

# A pin absent from the base is new, so selected.
cp "$tmp/work" "$tmp/work2"
{
	echo 'baz_url = "https://example/baz/v1/c.tgz"'
	echo 'baz_sha256 = "cccc"'
} >>"$tmp/work2"
expect "a new pin is selected" "baz foo " "$(names "$tmp/work2" "$tmp/base")"

# Identical to base: nothing selected, so an unchanged pin is never re-fetched
# and cannot be relocked to a silently-mutated upstream artifact.
expect "identical file selects nothing" "" "$(names "$tmp/base" "$tmp/base")"

# Absent base: every pin is new.
expect "absent base selects every pin" "bar foo " "$(names "$tmp/base" "$tmp/nope")"

echo ""
echo "$PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
