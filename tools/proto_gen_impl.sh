#!/usr/bin/env bash
# proto_gen_impl.sh — generate or check Rust code from proto definitions.
#
# Args: [--check]
#
# update mode (bazel run):  regenerates lib/rust/causes_proto/src/generated/ in source tree
# check  mode (bazel test): fails if committed generated code is stale
set -euo pipefail

CHECK=false
if [[ "${1:-}" == "--check" ]]; then
	CHECK=true
	shift
fi

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

# Suppress LLVM profraw output from instrumented binaries (proto_gen, protoc).
# Under `bazel coverage` these binaries are instrumented, but this is a
# golden-file check — there is no coverage to collect.  If .profraw files
# land in COVERAGE_DIR, Bazel's collect_cc_coverage.sh tries LLVM_PROFDATA
# which is not set for sh_test targets.
export LLVM_PROFILE_FILE=/dev/null

export PROTOC
PROTOC="$(rlocation protobuf+/protoc)"

# Well-known types (google/protobuf/*.proto) bundled with the protobuf module.
# Find one WKT file and derive the root include directory.
WKT_FILE="$(rlocation protobuf+/src/google/protobuf/timestamp.proto)"
export PROTO_GEN_WKT_DIR
PROTO_GEN_WKT_DIR="${WKT_FILE%/google/protobuf/timestamp.proto}"

PROTO_GEN="$(rlocation _main/tools/proto-gen/proto_gen)"
RUSTFMT="$(rlocation rust_host_tools/bin/rustfmt)"

if [[ "$CHECK" == "true" ]]; then
	# Generate into a temp directory and diff against committed files.
	WORKSPACE_ROOT="$(dirname "$(rlocation _main/Cargo.toml)")"
	TMPDIR="$(mktemp -d)"
	GEN_DIR="$TMPDIR/gen"
	mkdir -p "$GEN_DIR"

	# Run proto-gen pointed at the temp output, then format.
	PROTO_GEN_OUT_DIR="$GEN_DIR" \
		PROTO_GEN_PROTO_DIR="$WORKSPACE_ROOT/proto" \
		"$PROTO_GEN"
	"$RUSTFMT" --edition 2024 "$GEN_DIR"/*.rs

	COMMITTED="$WORKSPACE_ROOT/lib/rust/causes_proto/src/generated"
	if ! diff -r "$GEN_DIR" "$COMMITTED" >/dev/null 2>&1; then
		echo >&2 "ERROR: generated proto code is stale."
		echo >&2 "Run: bazel run //tools:proto_gen"
		diff -r "$GEN_DIR" "$COMMITTED" >&2 || true
		rm -rf "$TMPDIR"
		exit 1
	fi
	rm -rf "$TMPDIR"
	echo "proto codegen is up to date"
else
	# Update mode — write directly into the source tree, then format.
	cd "${BUILD_WORKSPACE_DIRECTORY}"
	"$PROTO_GEN"
	"$RUSTFMT" --edition 2024 lib/rust/causes_proto/src/generated/*.rs
fi
