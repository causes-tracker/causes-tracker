#!/usr/bin/env bash
set -euo pipefail
# Capture before echo so a git failure aborts (echo would exit 0).
sha="$(git rev-parse HEAD)"
echo "COMMIT_SHA $sha"
echo "REPO_URL https://github.com/causes-tracker/causes-tracker.git"
