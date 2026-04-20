#!/usr/bin/env bash
# Hermetic TLC runner.  Used by `tla_test` rules (e.g. //tla:BUILD.bazel).
#
# Arguments:
#   $1: relative path to the .cfg file (resolved via runfiles)
#   $2: relative path to the entry .tla file
#   remaining: additional TLC flags
#
# Resolves the JRE and tla2tools.jar from runfiles so the entire toolchain
# is hermetic.

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

JAVA="$(rlocation +http_archive+temurin_jre_linux_amd64/bin/java)"
JAR="$(rlocation +http_file+tla2tools/file/tla2tools.jar)"

if [[ ! -x "${JAVA}" ]]; then
	echo "ERROR: java not found in runfiles at ${JAVA}" >&2
	exit 1
fi
if [[ ! -f "${JAR}" ]]; then
	echo "ERROR: tla2tools.jar not found in runfiles at ${JAR}" >&2
	exit 1
fi

CFG_REL="$1"
TLA_REL="$2"
shift 2

CFG="$(rlocation "${CFG_REL}")"
TLA="$(rlocation "${TLA_REL}")"
if [[ ! -f "${CFG}" || ! -f "${TLA}" ]]; then
	echo "ERROR: spec files not found: cfg=${CFG_REL} tla=${TLA_REL}" >&2
	exit 1
fi

# TLC reads the .cfg with the same basename as the .tla unless told otherwise,
# and resolves modules from the cwd.  Stage everything into a temp dir.
WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

# Copy the entry .tla and .cfg into work dir; also copy any sibling .tla files
# the entry might EXTEND (we copy all .tla files in the same directory as the
# entry to keep this simple).
TLA_DIR="$(dirname "${TLA}")"
cp "${TLA_DIR}"/*.tla "${WORK}/"
cp "${CFG}" "${WORK}/$(basename "${TLA}" .tla).cfg"

cd "${WORK}"
exec "${JAVA}" -XX:+UseParallelGC -cp "${JAR}" tlc2.TLC \
	-deadlock \
	-workers auto \
	"$@" \
	"$(basename "${TLA}")"
