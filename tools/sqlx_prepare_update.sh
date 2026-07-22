#!/usr/bin/env bash
# Copy a freshly captured `.sqlx` TreeArtifact back into the source tree.
#
# $1 — rootpath of the captured `.sqlx` TreeArtifact
# $2 — package path of the crate (its committed `.sqlx/` lives at
#      $BUILD_WORKSPACE_DIRECTORY/$2/.sqlx)
set -euo pipefail

captured="${1:?usage: sqlx_prepare_update.sh <captured-rootpath> <pkg>}"
pkg="${2:?usage: sqlx_prepare_update.sh <captured-rootpath> <pkg>}"
dest="${BUILD_WORKSPACE_DIRECTORY:?must be invoked via bazel run}/${pkg}/.sqlx"

# Stage into a sibling dir and swap, so a failed copy cannot leave the
# committed .sqlx/ emptied. Bazel materializes TreeArtifact files executable;
# install -m strips that so the committed files keep plain-file modes.
staged="${dest}.new"
rm -rf "$staged"
mkdir -p "$staged"
install -m 0644 "$captured"/query-*.json "$staged/"
rm -rf "$dest"
mv "$staged" "$dest"
echo "Updated ${dest} ($(find "$dest" -name 'query-*.json' | wc -l) files)"
