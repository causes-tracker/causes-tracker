#!/usr/bin/env bash
# Builds the docs with Zensical and deploys to GitHub Pages via ghp-import.
# Run with: bazel run //docs:deploy_docs
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
GHP_IMPORT=$(rlocation _main/docs/ghp_import)
MKDOCS_YML=$(rlocation _main/docs/mkdocs.yml)

DESIGNDOCS_SRC="${BUILD_WORKSPACE_DIRECTORY}/designdocs"

build_docs_site

# ghp-import needs git credentials from the workspace.
cd "${BUILD_WORKSPACE_DIRECTORY}"
"$GHP_IMPORT" --force --push "$SITE_DIR"
