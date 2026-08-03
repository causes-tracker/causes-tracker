#!/usr/bin/env bash
# Test driver for update_archive_sha.sh.
# Sources the script (with _UPDATE_ARCHIVE_SHA_SOURCED=1 to skip main) and
# checks archives_of (which archives, and their pins, are seen — the parse that
# decides which shas get recomputed) and rewrite_in_block (the mutation).
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

# rewrite_in_block replaces the named archive's pin, leaving the other alone.
new="deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
rewrite_in_block "$tmp/BUCK" "cpython" 'sha256 = "aaaa"' "sha256 = \"${new}\""
trim() { sed 's/^[[:space:]]*//'; }
expect "rewrite updates the target pin" "sha256 = \"${new}\"," "$(grep -m1 'sha256' "$tmp/BUCK" | trim)"
expect "rewrite leaves the other pin" 'sha256 = "bbbb",' "$(grep 'sha256' "$tmp/BUCK" | tail -1 | trim)"

# rewrite_in_block fails loudly when the old value is absent from the named
# block — the mutation did not take.
cat >"$tmp/fresh" <<'EOF'
pinned_file(
    name = "fresh",
    sha256 = "cccc",
)
EOF
rc=0
rewrite_in_block "$tmp/fresh" "fresh" 'sha256 = "notpresent"' 'sha256 = "ffff"' 2>/dev/null || rc=$?
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

# A block with size_bytes emits it as a fourth field; one without still emits three.
cat >"$tmp/BUCK_size" <<'EOF'
cached_http_archive(
    name = "sized",
    sha256 = "eeee",
    size_bytes = 12345,
    urls = ["https://example/s/v1/s.tar.gz"],
)

cached_http_archive(
    name = "unsized",
    sha256 = "ffff",
    urls = ["https://example/u/v1/u.tar.gz"],
)
EOF
expect "archives_of emits size_bytes as a fourth field, omits it when absent" \
	$'sized\teeee\thttps://example/s/v1/s.tar.gz\t12345\nunsized\tffff\thttps://example/u/v1/u.tar.gz' \
	"$(archives_of "$tmp/BUCK_size")"

rewrite_in_block "$tmp/BUCK_size" "sized" 'size_bytes = 12345,' 'size_bytes = 67890,'
expect "rewrite_in_block updates the size" 'size_bytes = 67890,' "$(grep -m1 'size_bytes' "$tmp/BUCK_size" | trim)"
rc=0
rewrite_in_block "$tmp/BUCK_size" "sized" 'size_bytes = 11111,' 'size_bytes = 22222,' 2>/dev/null || rc=$?
expect "rewrite_in_block fails when the old size is absent" "1" "$rc"

# main recomputes size_bytes alongside sha256 when the URL changed. The stub
# curl writes the URL as content, so the new size is the URL's byte length.
cat >"$tmp/base3" <<'EOF'
cached_http_archive(name = "s", sha256 = "SSSS", size_bytes = 1, urls = ["https://ex/s/v1"])
EOF
cat >"$tmp/work3" <<'EOF'
cached_http_archive(name = "s", sha256 = "SSSS", size_bytes = 1, urls = ["https://ex/s/v2"])
EOF
new_url="https://ex/s/v2"
PATH="$stub:$PATH" main "$tmp/work3" "$tmp/base3"
expect "main recomputes size_bytes for the changed archive" "size_bytes = ${#new_url}," \
	"$(sed -n 's/.*\(size_bytes = [0-9]*,\).*/\1/p' "$tmp/work3")"

# Two blocks sharing the same size_bytes, the bumped one second: a rewrite
# keyed on the value alone would hit the first block instead.
cat >"$tmp/base4" <<'EOF'
pinned_file(
    name = "one",
    sha256 = "1111",
    size_bytes = 500,
    url = "https://ex/one/v1",
)
pinned_file(
    name = "two",
    sha256 = "2222",
    size_bytes = 500,
    url = "https://ex/two/v1",
)
EOF
sed 's|https://ex/two/v1|https://ex/two/v2|' "$tmp/base4" >"$tmp/work4"
url_two="https://ex/two/v2"
PATH="$stub:$PATH" main "$tmp/work4" "$tmp/base4"
expect "main updates the bumped second block's size" "size_bytes = ${#url_two}," \
	"$(sed -n '/name = "two"/,/^)/p' "$tmp/work4" | grep 'size_bytes' | trim)"
expect "main leaves the first block's equal size untouched" 'size_bytes = 500,' \
	"$(sed -n '/name = "one"/,/^)/p' "$tmp/work4" | grep 'size_bytes' | trim)"

# Two blocks sharing the same sha256, the bumped one second.
cat >"$tmp/base5" <<'EOF'
pinned_file(
    name = "p",
    sha256 = "9999",
    url = "https://ex/p/v1",
)
pinned_file(
    name = "q",
    sha256 = "9999",
    url = "https://ex/q/v1",
)
EOF
sed 's|https://ex/q/v1|https://ex/q/v2|' "$tmp/base5" >"$tmp/work5"
want_q="$(printf '%s' 'https://ex/q/v2' | sha256sum | awk '{print $1}')"
PATH="$stub:$PATH" main "$tmp/work5" "$tmp/base5"
expect "main updates the bumped second block's sha" "sha256 = \"${want_q}\"," \
	"$(sed -n '/name = "q"/,/^)/p' "$tmp/work5" | grep 'sha256' | trim)"
expect "main leaves the first block's equal sha untouched" 'sha256 = "9999",' \
	"$(sed -n '/name = "p"/,/^)/p' "$tmp/work5" | grep 'sha256' | trim)"

# rewrite_in_block skips triple-quoted string content: the bare ")" inside
# build_file_content must not end the block before the sha line.
cat >"$tmp/MODULE3.bazel" <<'EOF'
http_archive(
    name = "jdk",
    build_file_content = """
filegroup(
    name = "files",
    srcs = glob(["**"]),
)
""",
    sha256 = "abcd",
    url = "https://example/jdk/v4/jdk.tar.gz",
)
EOF
rewrite_in_block "$tmp/MODULE3.bazel" "jdk" 'sha256 = "abcd"' 'sha256 = "ef01"'
expect "rewrite_in_block survives multiline build_file_content" 'sha256 = "ef01",' \
	"$(grep -m1 'sha256' "$tmp/MODULE3.bazel" | trim)"

# A block whose closing ")" line lacks a trailing newline is still parsed.
printf 'pinned_file(\n    name = "tail",\n    sha256 = "0a0a",\n    url = "https://ex/tail/v1",\n)' >"$tmp/noeol_parse"
expect "archives_of emits a block with an unterminated closing line" \
	$'tail\t0a0a\thttps://ex/tail/v1' \
	"$(archives_of "$tmp/noeol_parse")"

# A file whose final line lacks a trailing newline keeps that line; the
# output is normalized to end with a newline.
# The sentinel x preserves the trailing newline through command substitution.
printf 'pinned_file(\n    name = "bare",\n    sha256 = "0808",\n)' >"$tmp/noeol"
rewrite_in_block "$tmp/noeol" "bare" 'sha256 = "0808"' 'sha256 = "0909"'
expect "rewrite_in_block keeps an unterminated final line" \
	$'pinned_file(\n    name = "bare",\n    sha256 = "0909",\n)\nx' \
	"$(
		cat "$tmp/noeol"
		printf x
	)"

# A pinned_file block: singular url, sha256, and size_bytes in one BUCK rule.
cat >"$tmp/BUCK_apk" <<'EOF'
pinned_file(
    name = "libstdc++",
    sha256 = "9999",
    size_bytes = 917068,
    url = "https://dl-cdn.alpinelinux.org/alpine/v3.19/main/x86_64/libstdc++-13.2.1_git20231014-r0.apk",
)
EOF
expect "archives_of reads a pinned_file block" \
	$'libstdc++\t9999\thttps://dl-cdn.alpinelinux.org/alpine/v3.19/main/x86_64/libstdc++-13.2.1_git20231014-r0.apk\t917068' \
	"$(archives_of "$tmp/BUCK_apk")"

echo ""
echo "$PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
