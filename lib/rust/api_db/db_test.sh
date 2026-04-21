#!/usr/bin/env bash
# Database integration tests for api_db.
# Starts a hermetic PostgreSQL instance via the Bazel test fixture,
# then runs the compiled Rust test binary with DATABASE_URL set.
#
# $1 — rlocation path to llvm-profdata (supplied by sh_test args)
# $2 — rlocation path to llvm-cov
# $3 — path to the api_db_test binary
# remaining — extra args passed through to the test binary
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

# shellcheck source=/dev/null
source "$(rlocation _main/infra/postgres/testfixture.sh)"
pg_start

llvm_profdata_rloc="${1:?usage: db_test.sh <llvm-profdata> <llvm-cov> <api_db_test> [args...]}"
llvm_cov_rloc="${2:?usage: db_test.sh <llvm-profdata> <llvm-cov> <api_db_test> [args...]}"
test_binary="${3:?usage: db_test.sh <llvm-profdata> <llvm-cov> <api_db_test> [args...]}"

if [[ -n "${COVERAGE_DIR:-}" ]]; then
	# Coverage mode: direct the instrumented binary to write profraw data into
	# a scratch dir, then merge and export LCOV into $COVERAGE_DIR/coverage.dat.
	# Intermediate .profraw/.profdata files must stay outside $COVERAGE_DIR so
	# Bazel's CoverageOutputGenerator does not try to parse them alongside LCOV.
	profraw_dir="$(mktemp -d)"
	export LLVM_PROFILE_FILE="${profraw_dir}/rust_test_%m.profraw"
	DATABASE_URL="$TEST_POSTGRES_URL" "$test_binary" "${@:4}"

	llvm_profdata="$(rlocation "$llvm_profdata_rloc")"
	llvm_cov="$(rlocation "$llvm_cov_rloc")"

	"$llvm_profdata" merge -sparse "${profraw_dir}"/*.profraw \
		-o "${profraw_dir}/merged.profdata"
	"$llvm_cov" export --format=lcov \
		--instr-profile="${profraw_dir}/merged.profdata" \
		"$test_binary" \
		>"${COVERAGE_DIR}/coverage.dat"
	rm -rf "$profraw_dir"
else
	DATABASE_URL="$TEST_POSTGRES_URL" "$test_binary" "${@:4}"
fi
