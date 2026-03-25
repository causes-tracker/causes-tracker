#!/usr/bin/env bash
# Verifies that the hermetic rust toolchain wrappers resolve and report
# the expected versions.
set -euo pipefail

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

# Rust 1.87.0 release commit hash — appears in rustc and rustfmt --version output.
EXPECTED_RUST_VERSION="1.87.0"
EXPECTED_COMMIT="17067e9ac"

cargo_bin="$(rlocation rust_host_tools/bin/cargo)"
rustc_bin="$(rlocation rust_host_tools/bin/rustc)"
rustfmt_bin="$(rlocation rust_host_tools/bin/rustfmt)"

fail() { echo "FAIL: $*" >&2; exit 1; }

cargo_ver="$("$cargo_bin" --version)"
[[ "$cargo_ver" == *"$EXPECTED_RUST_VERSION"* ]] \
  || fail "cargo version mismatch: got '$cargo_ver', want $EXPECTED_RUST_VERSION"
echo "PASS: $cargo_ver"

rustc_ver="$("$rustc_bin" --version)"
[[ "$rustc_ver" == *"$EXPECTED_RUST_VERSION"* ]] \
  || fail "rustc version mismatch: got '$rustc_ver', want $EXPECTED_RUST_VERSION"
echo "PASS: $rustc_ver"

# rustfmt has its own version number; verify it's from the 1.87.0 toolchain
# by matching the release commit hash that appears in its --version output.
rustfmt_ver="$("$rustfmt_bin" --version)"
[[ "$rustfmt_ver" == *"$EXPECTED_COMMIT"* ]] \
  || fail "rustfmt commit mismatch: got '$rustfmt_ver', want commit $EXPECTED_COMMIT"
echo "PASS: $rustfmt_ver"
