#!/usr/bin/env bash
# Test driver for update_archive_sha.sh.
# Sources the script (with _UPDATE_ARCHIVE_SHA_SOURCED=1 to skip main) and
# checks archives_of (which archives, and their pins, are seen — the parse that
# decides which shas get recomputed) and rewrite_sha (the mutation).
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

SCRIPT="$(rlocation _main/tools/renovate/update_archive_sha.sh)"
if [[ ! -f "$SCRIPT" ]]; then
	echo "ERROR: cannot locate update_archive_sha.sh via runfiles" >&2
	exit 1
fi

# shellcheck source=/dev/null
_UPDATE_ARCHIVE_SHA_SOURCED=1 source "$SCRIPT"

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

# A file with two archives and one unrelated rule between them.
cat >"$tmp/BUCK" <<'EOF'
cached_http_archive(
    name = "cpython",
    sha256 = "aaaa",
    urls = ["https://example/py/v1/cpython.tar.gz"],
)

filegroup(
    name = "misc",
    srcs = ["x"],
)

cached_http_archive(
    name = "toolchain",
    sha256 = "bbbb",
    urls = ["https://example/tc/v9/tc.tar.gz"],
)
EOF

expect "archives_of finds both pins, skips the non-archive rule" \
	$'cpython\taaaa\thttps://example/py/v1/cpython.tar.gz\ntoolchain\tbbbb\thttps://example/tc/v9/tc.tar.gz' \
	"$(archives_of "$tmp/BUCK")"
expect "archives_of on absent file -> empty" "" "$(archives_of "$tmp/nope")"

# MODULE.bazel http_archive blocks use a singular `url = "..."`.
cat >"$tmp/MODULE.bazel" <<'EOF'
http_archive(
    name = "crane_linux_amd64",
    build_file_content = """exports_files(["crane"])""",
    sha256 = "cccc",
    url = "https://example/cr/v2/crane.tar.gz",
)
EOF

expect "archives_of reads MODULE.bazel singular url" \
	$'crane_linux_amd64\tcccc\thttps://example/cr/v2/crane.tar.gz' \
	"$(archives_of "$tmp/MODULE.bazel")"

# A multiline build_file_content closes a nested call on its own line; that
# bare ")" must not end the block.
cat >"$tmp/MODULE2.bazel" <<'EOF'
http_archive(
    name = "jre",
    build_file_content = """
filegroup(
    name = "files",
    srcs = glob(["**"]),
)
""",
    sha256 = "dddd",
    url = "https://example/jre/v3/jre.tar.gz",
)
EOF

expect "archives_of survives multiline build_file_content" \
	$'jre\tdddd\thttps://example/jre/v3/jre.tar.gz' \
	"$(archives_of "$tmp/MODULE2.bazel")"

# rewrite_sha replaces one archive's pin by value, leaving the other alone.
new="deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
rewrite_sha "$tmp/BUCK" "aaaa" "$new"
trim() { sed 's/^[[:space:]]*//'; }
expect "rewrite updates the target pin" "sha256 = \"${new}\"," "$(grep -m1 'sha256' "$tmp/BUCK" | trim)"
expect "rewrite leaves the other pin" 'sha256 = "bbbb",' "$(grep 'sha256' "$tmp/BUCK" | tail -1 | trim)"

# rewrite_sha fails loudly when the old value is absent and the new one isn't
# already there — the mutation did not take.
printf 'sha256 = "cccc",\n' >"$tmp/fresh"
rc=0
rewrite_sha "$tmp/fresh" "notpresent" "ffff" 2>/dev/null || rc=$?
expect "rewrite fails when the old sha is absent" "1" "$rc"

# main re-fetches (and rewrites) only archives whose URL changed vs base.
# Stub curl to write the URL as the "content", so sha256sum of that is
# deterministic — no network.
stub="$tmp/stub"
mkdir -p "$stub"
cat >"$stub/curl" <<'SH'
#!/bin/sh
out=""; url=""
while [ $# -gt 0 ]; do
	case "$1" in
	-o) out="$2"; shift 2 ;;
	-*) shift ;;
	*) url="$1"; shift ;;
	esac
done
printf '%s' "$url" >"$out"
SH
chmod +x "$stub/curl"

cat >"$tmp/base2" <<'EOF'
cached_http_archive(name = "a", sha256 = "AAAA", urls = ["https://ex/a/v1"])
cached_http_archive(name = "b", sha256 = "BBBB", urls = ["https://ex/b/v1"])
EOF
cat >"$tmp/work2" <<'EOF'
cached_http_archive(name = "a", sha256 = "AAAA", urls = ["https://ex/a/v2"])
cached_http_archive(name = "b", sha256 = "BBBB", urls = ["https://ex/b/v1"])
EOF
want_a="$(printf '%s' 'https://ex/a/v2' | sha256sum | awk '{print $1}')"
PATH="$stub:$PATH" main "$tmp/work2" "$tmp/base2"
expect "main recomputes the changed archive" "sha256 = \"${want_a}\"" "$(sed -n 's/.*\(sha256 = "[0-9a-f]*"\).*/\1/p' "$tmp/work2" | head -1)"
expect "main leaves the unchanged archive" 'sha256 = "BBBB"' "$(sed -n 's/.*\(sha256 = "[^"]*"\).*/\1/p' "$tmp/work2" | tail -1)"

echo ""
echo "$PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
