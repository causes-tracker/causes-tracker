#!/usr/bin/env bash
# End-to-end test: runs //docs:serve_docs and makes HTTP requests against it.
# Run with: bazel test //docs:serve_test
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

# serve_docs reads designdocs from BUILD_WORKSPACE_DIRECTORY.
# Stage them from the test's runfiles so the binary can be run hermetically.
DESIGNDOCS_RUNFILES=$(rlocation _main/designdocs/README.md)
WORKSPACE="$TEST_TMPDIR/workspace"
mkdir -p "$WORKSPACE/designdocs"
cp -rL "$(dirname "$DESIGNDOCS_RUNFILES")/." "$WORKSPACE/designdocs/"

# Pick a free port.
PORT=$(python3 -c \
	"import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); \
   p=s.getsockname()[1]; s.close(); print(p)")

SERVE_DOCS=$(rlocation _main/docs/serve_docs)

BUILD_WORKSPACE_DIRECTORY="$WORKSPACE" SERVE_PORT="$PORT" "$SERVE_DOCS" \
	>/dev/null 2>&1 &
SERVER_PID=$!
trap 'kill "$SERVER_PID" 2>/dev/null || true' EXIT

# Wait up to 10 s for the server to accept connections.
for i in $(seq 1 20); do
	if curl -sf "http://127.0.0.1:${PORT}/" >/dev/null 2>&1; then
		break
	fi
	sleep 0.5
	if [[ "$i" -eq 20 ]]; then
		echo >&2 "ERROR: HTTP server did not become ready within 10 seconds"
		exit 1
	fi
done

FAIL=0

check() {
	local path="$1" needle="$2"
	local body
	if ! body=$(curl -sf "http://127.0.0.1:${PORT}/${path}"); then
		echo >&2 "ERROR: HTTP GET /${path} failed"
		FAIL=1
		return
	fi
	if ! grep -qi "$needle" <<<"$body"; then
		echo >&2 "ERROR: /${path} does not contain '${needle}'"
		FAIL=1
	fi
}

check "" "Causes"
check "Manifesto/" "Manifesto"
check "Proto-Reference/" "Protocol Documentation"

[[ "$FAIL" -eq 0 ]] || exit 1
echo "OK: serve_docs served all expected pages on port ${PORT}"
