#!/usr/bin/env bash
# Run the TLA+ TLC model checker against Replication.tla.
#
# Prerequisites:
#   - Java 11 or later (e.g. `apt install openjdk-21-jre-headless`).
#
# Downloads tla2tools.jar on first run and caches it in ~/.cache/tla.
# Pass extra TLC arguments after `--`, e.g. `run_tlc.sh -- -workers auto`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CACHE_DIR="${HOME}/.cache/tla"
JAR_PATH="${CACHE_DIR}/tla2tools.jar"
JAR_VERSION="1.8.0"
JAR_URL="https://github.com/tlaplus/tlaplus/releases/download/v${JAR_VERSION}/tla2tools.jar"

if ! command -v java >/dev/null 2>&1; then
	echo "ERROR: java is not installed." >&2
	echo "Install OpenJDK 11+ (e.g. 'sudo apt install openjdk-21-jre-headless')." >&2
	exit 1
fi

if [ ! -f "${JAR_PATH}" ]; then
	echo "Downloading tla2tools.jar v${JAR_VERSION} to ${JAR_PATH}..."
	mkdir -p "${CACHE_DIR}"
	curl -fsSL -o "${JAR_PATH}.tmp" "${JAR_URL}"
	mv "${JAR_PATH}.tmp" "${JAR_PATH}"
fi

cd "${SCRIPT_DIR}"
exec java -XX:+UseParallelGC -cp "${JAR_PATH}" tlc2.TLC \
	-config Replication.cfg \
	-deadlock \
	"$@" \
	ReplicationMC.tla
