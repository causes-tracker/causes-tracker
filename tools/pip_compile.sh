#!/usr/bin/env bash
# Regenerate requirements_lock.txt from requirements.in.
# --exclude-newer enforces a 7-day cooldown on package versions, matching
# Renovate's minimumReleaseAge gate for direct deps (see renovate.json).
set -euo pipefail

UV="$(readlink -f -- "${1:?uv binary path required as argv[1]}")"
cd "${BUILD_WORKSPACE_DIRECTORY:?must be run via 'bazel run'}"

exec "$UV" pip compile \
	--quiet \
	--generate-hashes \
	--exclude-newer='1 week' \
	--python-version=3.12 \
	--output-file=requirements_lock.txt \
	requirements.in
