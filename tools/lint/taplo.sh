#!/usr/bin/env bash
# Hermetic taplo wrapper. Resolves the extracted binary via Bazel runfiles.
# All arguments are forwarded to taplo unchanged.
set -euo pipefail

# shellcheck source=/dev/null
source "$(rlocation bazel_tools/tools/bash/runfiles/runfiles.bash)"
exec "$(rlocation _main/tools/lint/taplo_bin)" "$@"
