#!/usr/bin/env bash
# The launcher must refuse to run in a checkout carrying bazel-* convenience
# symlinks, which make the buck2 file watcher silently stop seeing edits
# (mechanism in buck2.sh).
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

set -e

BUCK2_SH="$(rlocation _main/tools/buck2/buck2.sh)"
if [[ ! -f "$BUCK2_SH" ]]; then
	echo "ERROR: cannot locate tools/buck2/buck2.sh via runfiles" >&2
	exit 1
fi

fail() {
	echo "FAIL: $1" >&2
	exit 1
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# The stub logs its argv per call; "buckd running" keeps the launcher out of
# its bootstrap path in the pass case.
mkdir -p "$tmp/bin"
cat >"$tmp/bin/buck2-bin" <<EOF
#!/bin/sh
echo "\$@" >>"$tmp/buck2-bin.log"
if [ "\$1" = "status" ]; then echo "buckd running"; fi
exit 0
EOF
chmod 0755 "$tmp/bin/buck2-bin"
export PATH="$tmp/bin:$PATH"

root="$tmp/repo"
mkdir -p "$root"
: >"$root/.buckroot"

# A bazel-* symlink must refuse before any daemon interaction.
ln -s /nonexistent "$root/bazel-repo"
out="$(cd "$root" && "$BUCK2_SH" version 2>&1)" && fail "guard did not refuse"
grep -q 'bazel-repo' <<<"$out" || fail "refusal does not name the symlink: $out"
[[ ! -e "$tmp/buck2-bin.log" ]] || fail "daemon was contacted despite refusal"

# Without the symlink the launcher hands over to the real binary; a plain
# bazel-* directory (no symlink) must not trigger the guard.
rm "$root/bazel-repo"
mkdir "$root/bazel-out"
(cd "$root" && "$BUCK2_SH" version >/dev/null 2>&1) || fail "clean root refused"
grep -q '^version$' "$tmp/buck2-bin.log" 2>/dev/null ||
	fail "launcher never handed 'version' to buck2-bin"

echo ok
