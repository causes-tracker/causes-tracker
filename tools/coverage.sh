#!/usr/bin/env bash
# Run bazel coverage and enforce a minimum per-file coverage threshold for
# every non-test Rust source file (services/ and lib/rust/).
#
# Usage: tools/coverage.sh [bazel-args...]
#   e.g. tools/coverage.sh //...
#        tools/coverage.sh --config=ci //...
set -euo pipefail

REPORT="bazel-out/_coverage/_coverage_report.dat"
MIN_PCT=25

# Short-circuit: if the working copy's commit_id matches a prior green run,
# skip the bazel invocation entirely. jj auto-snapshots the working copy
# into @, so any file edit changes commit_id; MODULE.bazel.lock, .bazelversion,
# and every other tracked config is included in the hash.
#
# The cache lives at the workspace root in .coverage-green (gitignored).
# Per-worktree by filesystem location, so parallel worktrees never ping-pong.
# The file is ignored so jj does not snapshot it — otherwise writing the cache
# would itself change commit_id and invalidate the next run.
GREEN_CACHE=".coverage-green"
WORKING_COPY_ID="$(jj log -r @ -T commit_id --no-graph 2>/dev/null || true)"
if [[ -n "$WORKING_COPY_ID" && -f "$GREEN_CACHE" ]] &&
	[[ "$(cat "$GREEN_CACHE")" == "$WORKING_COPY_ID $*" ]]; then
	echo "coverage ok: unchanged since last green (commit $WORKING_COPY_ID)"
	exit 0
fi

# Files excluded from the per-file coverage threshold.
# "Hard to test" is NOT a valid reason — only exclude files where the code
# is entirely constrained by the type system with no alternative implementations.
SKIP_FILES=(
	"lib/rust/causes_proto/src/generated/causes.v1.rs" # machine-generated
	"services/causes_api/src/store.rs"                 # trait delegation to api_db
)

# Runs every test in one bazel invocation, including the format_check_auto
# tagged sh_tests. Those produce no Rust coverage data — they're bundled
# here so the hook pays only one bazel analysis phase instead of two.
bazel coverage "$@"

if [[ ! -f "$REPORT" ]]; then
	echo "error: coverage report not found at $REPORT" >&2
	exit 1
fi

# Collect all Rust source files on disk under services/ and lib/rust/.
# These are the files the coverage check must account for.
mapfile -t disk_files < <(
	find services lib/rust -name '*.rs' | sort
)

if [[ ${#disk_files[@]} -eq 0 ]]; then
	echo "error: no Rust source files found under services/ or lib/rust/" >&2
	exit 1
fi

# Parse the LCOV report using its LH/LF summary lines (lines hit / lines found).
# Emit one line per Rust source: "<lh> <lf> <path>"
lcov_summary=$(awk '
/^SF:/ {
    sf = substr($0, 4)
    lh = 0
    lf = 0
}
/^LH:/ { lh = substr($0, 4) + 0 }
/^LF:/ { lf = substr($0, 4) + 0 }
/^end_of_record/ {
    if (sf ~ /\.rs$/ && (sf ~ /^services\// || sf ~ /^lib\/rust\//)) {
        printf "%d %d %s\n", lh, lf, sf
    }
    sf = ""; lh = 0; lf = 0
}
' "$REPORT")

# For each file on disk, look it up and report its coverage.
failed=0
for f in "${disk_files[@]}"; do
	skip=0
	for s in "${SKIP_FILES[@]}"; do
		if [[ "$f" == "$s" ]]; then
			skip=1
			break
		fi
	done
	if [[ "$skip" -eq 1 ]]; then
		printf "%-6s  %5s  (%s)  %s\n" "skip" "n/a" "excluded" "$f"
		continue
	fi

	entry=$(echo "$lcov_summary" | awk -v f="$f" '$3 == f { print; exit }')

	if [[ -z "$entry" ]]; then
		printf "%-6s  %5s  (%s)  %s\n" "FAIL" "0.0%" "not in report" "$f"
		((failed++)) || true
		continue
	fi

	lh=$(echo "$entry" | awk '{print $1}')
	lf=$(echo "$entry" | awk '{print $2}')

	if [[ "$lf" -eq 0 ]]; then
		printf "%-6s  %5s  (%d/%d lines)  %s\n" "skip" "n/a" "$lh" "$lf" "$f"
		continue
	fi

	pct=$(awk -v h="$lh" -v f="$lf" 'BEGIN { printf "%.1f", h * 100.0 / f }')
	below=$(awk -v p="$pct" -v m="$MIN_PCT" 'BEGIN { print (p + 0 < m + 0) ? 1 : 0 }')
	if [[ "$below" -eq 1 ]]; then
		printf "%-6s  %5s%%  (%d/%d lines)  %s\n" "FAIL" "$pct" "$lh" "$lf" "$f"
		((failed++)) || true
	else
		printf "%-6s  %5s%%  (%d/%d lines)  %s\n" "ok" "$pct" "$lh" "$lf" "$f"
	fi
done

echo ""
if [[ "$failed" -gt 0 ]]; then
	echo "${failed}/${#disk_files[@]} Rust source file(s) below ${MIN_PCT}% threshold" >&2
	exit 1
fi

echo "coverage ok: ${#disk_files[@]} Rust source file(s) checked, all >= ${MIN_PCT}%"

# Record the green state so the next identical turn short-circuits. Keyed on
# both commit_id and the args, so a different bazel target set re-verifies.
if [[ -n "$WORKING_COPY_ID" ]]; then
	printf '%s %s' "$WORKING_COPY_ID" "$*" >"$GREEN_CACHE"
fi
