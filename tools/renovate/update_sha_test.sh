#!/usr/bin/env bash
# Test driver for update_sha.sh.
# Sources the script (with _UPDATE_SHA_SOURCED=1 to skip the top-level call)
# and exercises update_sha's tag derivation: most tools tag releases "vX.Y.Z",
# crun tags releases "X.Y.Z" with no "v" prefix, and the two must not be
# confused when querying the release API or building the download URL.
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

SCRIPT="$(rlocation _main/tools/renovate/update_sha.sh)"
if [[ ! -f "$SCRIPT" ]]; then
	echo "ERROR: cannot locate update_sha.sh via runfiles" >&2
	exit 1
fi

# shellcheck source=/dev/null
_UPDATE_SHA_SOURCED=1 source "$SCRIPT"
# Sourcing imports the script's -e; drop it so a failing case reports FAIL
# through the harness instead of aborting the run.
set +e

PASS=0
FAIL=0
tmp="$(mktemp -d)"
stub="$tmp/stub"
mkdir -p "$stub"
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

# gh stub: only answers the exact release-tag path each case expects, so a
# wrong tag (e.g. a stray "v" prefix) makes the lookup fail loudly instead of
# silently falling through to the curl path.
cat >"$stub/gh" <<'SH'
#!/bin/sh
path="$2"
case "$path" in
"repos/TraceMachina/nativelink/releases/tags/v1.6.1")
	echo "sha256:1111111111111111111111111111111111111111111111111111111111aaaa"
	;;
"repos/containers/crun/releases/tags/1.20")
	;; # no digest field on this asset -> fall through to curl
*)
	echo "gh stub: unexpected path $path" >&2
	exit 1
	;;
esac
SH
chmod +x "$stub/gh"

# curl stub: only answers the exact (unprefixed) crun download URL; any other
# URL (e.g. one with a stray "v") makes the fetch fail loudly.
cat >"$stub/curl" <<'SH'
#!/bin/sh
out="" url=""
while [ $# -gt 0 ]; do
	case "$1" in
	-o) out="$2"; shift 2 ;;
	-*) shift ;;
	*) url="$1"; shift ;;
	esac
done
case "$url" in
"https://github.com/containers/crun/releases/download/1.20/crun-1.20-linux-amd64")
	printf 'fake-crun-binary' >"$out"
	;;
*)
	echo "curl stub: unexpected url $url" >&2
	exit 1
	;;
esac
SH
chmod +x "$stub/curl"

export PATH="$stub:$PATH"

# nativelink: tag is "v${VERSION}" and the digest comes straight from gh.
nl="$tmp/nativelink-install.sh"
printf 'NATIVELINK_VERSION=1.6.1\nNATIVELINK_SHA256=stale\n' >"$nl"
update_sha "tools/nativelink/install.sh" "$nl"
expect "nativelink: digest path rewrites the pinned sha" \
	"NATIVELINK_SHA256=1111111111111111111111111111111111111111111111111111111111aaaa" \
	"$(grep NATIVELINK_SHA256 "$nl")"

# crun: tag has no "v" prefix, in both the release-lookup path and the
# download URL used on fallback.
crun="$tmp/crun-install.sh"
printf 'CRUN_VERSION=1.20\nCRUN_SHA256=stale\n' >"$crun"
update_sha "tools/crun/install.sh" "$crun"
want_sha="$(printf 'fake-crun-binary' | sha256sum | awk '{print $1}')"
expect "crun: unprefixed tag used for both lookup and download" \
	"CRUN_SHA256=${want_sha}" \
	"$(grep CRUN_SHA256 "$crun")"

# Unknown installer is rejected.
rc=0
update_sha "tools/nope/install.sh" "$tmp/nope" 2>/dev/null || rc=$?
expect "unknown installer fails" "1" "$rc"

echo ""
echo "$PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
