#!/usr/bin/env bash
# Tests install_release.shlib: a stubbed download installs only when its sha256
# matches, and a tgz payload lands as the named executable.
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
shlib="$(rlocation _main/tools/install_release.shlib)"

work="$(mktemp -d)"
bindir="$(mktemp -d)"
stub="$(mktemp -d)"
trap 'rm -rf "$work" "$bindir" "$stub"' EXIT

printf '#!/bin/sh\necho tool-ran\n' >"$work/tool"
chmod +x "$work/tool"
tar czf "$work/pkg.tgz" -C "$work" tool
sha="$(sha256sum "$work/pkg.tgz" | cut -d' ' -f1)"

# curl stub: ignore the URL, copy the fixture to the -o target.
cat >"$stub/curl" <<EOF
#!/bin/sh
out=""
while [ \$# -gt 0 ]; do [ "\$1" = "-o" ] && out="\$2"; shift; done
cp "$work/pkg.tgz" "\$out"
EOF
chmod +x "$stub/curl"
export PATH="$stub:$PATH"

# shellcheck source=/dev/null
. "$shlib"

install_release me/tool "$sha" v1 tool-1.tgz tgz tool "$bindir"
[[ -x "$bindir/tool" ]] || {
	echo "not installed"
	exit 1
}
[[ "$("$bindir/tool")" == "tool-ran" ]] || {
	echo "wrong content"
	exit 1
}

if install_release me/tool deadbeef v1 tool-1.tgz tgz tool2 "$bindir" 2>/dev/null; then
	echo "expected sha mismatch to fail"
	exit 1
fi
[[ -e "$bindir/tool2" ]] && {
	echo "installed despite bad sha"
	exit 1
}

# txz payload, with the binary at a subdirectory path that differs from the
# installed name (buckle's shape: archive holds pkg-dir/buckle, installed as
# buckle).
printf '#!/bin/sh\necho subdir-tool-ran\n' >"$work/subtool"
chmod +x "$work/subtool"
mkdir -p "$work/pkg-dir"
cp "$work/subtool" "$work/pkg-dir/subtool"
tar cJf "$work/pkg.txz" -C "$work" pkg-dir
txz_sha="$(sha256sum "$work/pkg.txz" | cut -d' ' -f1)"

cat >"$stub/curl" <<EOF
#!/bin/sh
out=""
while [ \$# -gt 0 ]; do [ "\$1" = "-o" ] && out="\$2"; shift; done
cp "$work/pkg.txz" "\$out"
EOF
chmod +x "$stub/curl"

install_release me/tool "$txz_sha" v1 tool-1.txz txz renamed-tool "$bindir" \
	pkg-dir/subtool
[[ -x "$bindir/renamed-tool" ]] || {
	echo "txz: not installed"
	exit 1
}
[[ "$("$bindir/renamed-tool")" == "subdir-tool-ran" ]] || {
	echo "txz: wrong content"
	exit 1
}

# A member path absent from the archive must fail the call, not report
# success with nothing installed.
if install_release me/tool "$txz_sha" v1 tool-1.txz txz ghost "$bindir" \
	pkg-dir/no-such-file 2>/dev/null; then
	echo "expected missing member to fail"
	exit 1
fi
[[ -e "$bindir/ghost" ]] && {
	echo "installed despite missing member"
	exit 1
}

# raw payload: the download is the binary itself.
raw_sha="$(sha256sum "$work/tool" | cut -d' ' -f1)"
cat >"$stub/curl" <<EOF
#!/bin/sh
out=""
while [ \$# -gt 0 ]; do [ "\$1" = "-o" ] && out="\$2"; shift; done
cp "$work/tool" "\$out"
EOF
chmod +x "$stub/curl"
install_release me/tool "$raw_sha" v1 tool-1 raw raw-tool "$bindir"
[[ -x "$bindir/raw-tool" && "$("$bindir/raw-tool")" == "tool-ran" ]] || {
	echo "raw: not installed or wrong content"
	exit 1
}

if install_release me/tool "$raw_sha" v1 tool-1 zip zip-tool "$bindir" 2>/dev/null; then
	echo "expected unknown format to fail"
	exit 1
fi
[[ -e "$bindir/zip-tool" ]] && {
	echo "installed despite unknown format"
	exit 1
}

# curl failure must fail the call before the checksum comparison.
cat >"$stub/curl" <<'EOF'
#!/bin/sh
exit 22
EOF
chmod +x "$stub/curl"
if install_release me/tool "$raw_sha" v1 tool-1 raw dl-tool "$bindir" 2>/dev/null; then
	echo "expected download failure to fail"
	exit 1
fi
[[ -e "$bindir/dl-tool" ]] && {
	echo "installed despite download failure"
	exit 1
}

echo "ok"
