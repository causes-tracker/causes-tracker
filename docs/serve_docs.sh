#!/usr/bin/env bash
# Builds the docs site and serves it for local browsing.
# Run with: bazel run //docs:serve_docs  →  browse http://localhost:8000
set -euo pipefail

# Standard Bazel 3-way runfiles init.
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

# shellcheck source=site_builder.bash
source "$(rlocation _main/docs/site_builder.bash)"

ZENSICAL=$(rlocation _main/docs/zensical)
MKDOCS_YML=$(rlocation _main/docs/mkdocs.yml)
PROTO_DOCS=$(rlocation _main/proto/proto_docs.md)

# Use the workspace source directly so edits are reflected on re-run.
DESIGNDOCS_SRC="${BUILD_WORKSPACE_DIRECTORY}/designdocs"

PROTO_DOCS="$PROTO_DOCS" build_docs_site

PORT="${SERVE_PORT:-8000}"
echo "Serving docs on http://localhost:${PORT}"
exec python3 -m http.server "$PORT" --directory "$SITE_DIR"
