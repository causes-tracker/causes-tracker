#!/usr/bin/env bash
# Builds the devcontainer via the devcontainers CLI, which exercises
# devcontainer.json (features, image build) — not just the Dockerfile alone.
set -euo pipefail

# Bazel 3-way runfiles init
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

DEVCONTAINER=$(rlocation _main/.devcontainer/devcontainer_cli_/devcontainer_cli)

# Runfiles for local (non-sandboxed) tests are symlinks to the real workspace
# files. Resolving the Dockerfile symlink gives us the workspace root.
DOCKERFILE=$(rlocation _main/.devcontainer/Dockerfile)
WORKSPACE=$(readlink -f "$(dirname "$(dirname "$DOCKERFILE")")")

# The docker buildx 'docker' driver can report "driver not connecting" when
# the daemon socket isn't yet accessible (common in DooD on first use).
# Initialising a builder with the docker-container driver avoids this.
if ! docker buildx inspect devcontainer-test-builder &>/dev/null; then
	docker buildx create --name devcontainer-test-builder --driver docker-container --use --bootstrap
else
	docker buildx use devcontainer-test-builder
fi

IMAGE_TAG="devcontainer-build-test-$$"
trap 'docker image rm -f "$IMAGE_TAG" 2>/dev/null || true' EXIT

"$DEVCONTAINER" build \
	--workspace-folder "$WORKSPACE" \
	--image-name "$IMAGE_TAG"
