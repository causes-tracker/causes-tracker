#!/usr/bin/env bash
# Run project quality gates against the current changeset.
#
# Three modes for three callers, sharing one set of gates:
#
#   ci        — runs every gate, no caching, no jj dependency.
#               Called from .github/workflows/build.yml.
#
#   [default] — runs every gate, with a per-changeset green-marker cache so
#               that re-running on an unchanged stack entry is instant.
#               Called by humans working a stack of jj changes.
#
#   agent     — `stack` plus a pre-flight scan: fails if the diff between
#               master and @ introduces any quality-check suppression that
#               a human hasn't reviewed.  Called from the Claude Code Stop
#               hook (.claude/settings.json) for AI end-turn validation.
#
# The gates, in order:
#   1. No BUILD.bazel file may load @rules_rs directly — use //build:rust.bzl.
#   2. Every Bazel package must have a README.md.
#   3. //:format.check (every formatter the project configures).
#   4. bazel coverage --lockfile_mode=error (runs every test).
#   5. Per-file Rust coverage threshold (MIN_PCT).
#
# Usage:
#   tools/check.sh ci    [bazel-flags...] [target...]
#   tools/check.sh       [bazel-flags...] [target...]
#   tools/check.sh agent [bazel-flags...] [target...]
#
# Anything starting with `-` is treated as a bazel flag (forwarded to
# format.check and coverage); the rest is treated as bazel targets
# (forwarded only to coverage).  Default target is //....

set -euo pipefail

REPORT="bazel-out/_coverage/_coverage_report.dat"
MIN_PCT=25
GREEN_CACHE_DIR=".coverage-green"

# Files excluded from the per-file coverage threshold.
# "Hard to test" is NOT a valid reason — only exclude files where the code
# is entirely constrained by the type system with no alternative implementations.
#
# Adding entries to this list requires explicit human approval. Automation
# (or AI assistants) must not append entries unprompted; ask first, then
# extract testable logic before reaching for the skip list.
SKIP_FILES=(
	"lib/rust/causes_proto/src/generated/causes.v1.rs" # machine-generated
	"services/causes_api/src/store.rs"                 # trait delegation to api_db
)

# Patterns the agent scanner flags when added on a `+` line.
# Each is a POSIX ERE matched by awk against the line body (minus the +).
SUPPRESSION_LINE_PATTERNS=(
	'# *shellcheck +disable'        # bash
	'#!?\[allow\('                  # rust attribute (item or crate)
	'#\[ignore(\]|\()'              # rust #[ignore] or #[ignore("...")]
	'// *eslint-disable'            # js/ts
	'// *@ts-(ignore|expect-error)' # ts
	'(^|[ \t])# *noqa([: ]|$)'      # python
	'# *type: *ignore'              # python (mypy)
)

# Files whose modification (or creation) the agent scanner flags outright,
# regardless of content.  These either DEFINE the rule the AI might want to
# bypass, or ARE a suppression mechanism by their very existence.
SUPPRESSION_GATE_FILES=(
	".bazelignore"
	".clippy.toml"
	"rustfmt.toml"
	".shellcheckrc"
	".yamlfmt"
	"tools/check.sh"
)

# Argument parsing is deferred until after the function definitions so that
# tests can `source tools/check.sh` with _CHECK_SH_SOURCED=1 and reuse the
# scanner without entering the dispatch flow.  See the matching guard near
# the dispatch block at the bottom.

# ── stack-mode cache ──────────────────────────────────────────────────────
#
# Short-circuit: if @'s patch diff matches a prior green run, skip bazel.
# Keyed on the semantic content of the change — git blob hashes and hunk
# line offsets are stripped — so the cache survives clean rebases and stack
# navigation (each change in the stack keeps its own entry).
#
# False-positive window: a lint rule added on master leaves the patch hash
# unchanged, so the cache says green while CI would fail. The test suite /
# CI is authoritative; this cache is a local turn-end optimization only.
#
# Empty @: every empty working copy hashes to the same key regardless of
# parent, so caching against an empty diff would alias unrelated states.
# Handling: walk up single-parent ancestors until a non-empty change is
# found, and key on that. If we hit a merge (multiple parents) or run out
# of ancestors with the diff still empty, error — there is no unambiguous
# patch to verify.
CACHE_KEY=""
compute_cache_key() {
	local mode="$1"
	shift
	local jj_conflicts target parent_ids parent_count diff_hash lockfile_hash
	if ! jj_conflicts="$(jj log -r '@ & conflicts()' --no-graph -T commit_id 2>/dev/null)"; then
		return 0
	fi
	if [[ -n "$jj_conflicts" ]]; then
		return 0
	fi
	target="@"
	while [[ -z "$(jj diff -r "$target" --summary 2>/dev/null)" ]]; do
		parent_ids="$(jj log -r "$target-" --no-graph -T 'commit_id ++ "\n"' 2>/dev/null)"
		parent_count=$(printf '%s' "$parent_ids" | grep -c . || true)
		if [[ "$parent_count" -ne 1 ]]; then
			# Walked up empty diffs and hit either a merge (2+ parents) or
			# the root (0 parents). Two cases:
			#
			# - In immutable history (e.g. master, a chain of GH-merged PR
			#   commits with multiple parents). Unfixable by the user.
			#   Treat as no-op success so the Stop hook doesn't block on
			#   a transient navigation state (common right after
			#   `jj rebase -r 'mutable()' -d master` leaves @ empty on a
			#   synthetic merge).
			#
			# - In mutable history (e.g. a user-constructed merge-of-all-
			#   work). The user can rebase or resolve; surface as error.
			if jj log -r "$target & immutable()" --no-graph -T commit_id 2>/dev/null | grep -q .; then
				echo "check skipped${CHANGE_ID:+ ($CHANGE_ID)}: nothing to verify — $target is in immutable history with empty diff and $parent_count parent(s)"
				exit 0
			fi
			echo "error: $target has empty diff and $parent_count parent(s); cannot determine what to verify" >&2
			exit 1
		fi
		target="${target}-"
	done
	diff_hash="$(jj diff --git -r "$target" 2>/dev/null |
		sed -E -e '/^index [0-9a-f]+\.\.[0-9a-f]+/d' \
			-e 's/^@@ -[0-9,]+ \+[0-9,]+ @@/@@/' |
		sha256sum | awk '{print $1}')"
	# Lockfile state is folded into the key so that a stack rewrite which
	# leaves a commit's patch unchanged but shifts the lockfile content
	# (e.g. bumping a workspace dep in an ancestor) invalidates the cached
	# verdict.
	lockfile_hash="$(sha256sum MODULE.bazel.lock Cargo.lock requirements_lock.txt 2>/dev/null |
		sha256sum | awk '{print $1}')"
	CACHE_KEY="$(printf '%s\t%s\t%s\t%s' "$diff_hash" "$lockfile_hash" "$mode" "$*" |
		sha256sum | awk '{print $1}')"
}

# ── agent-mode suppression scan ───────────────────────────────────────────
#
# Reads a git-format diff from stdin and prints a violation list to stderr.
# Exits 0 if clean, 1 if any suppression is introduced.  Pulled out as a
# function so the sh_test can drive it with synthetic diffs.
scan_diff_for_suppressions() {
	awk \
		-v line_patterns="$(printf '%s\n' "${SUPPRESSION_LINE_PATTERNS[@]}")" \
		-v gate_files="$(printf '%s\n' "${SUPPRESSION_GATE_FILES[@]}")" '
BEGIN {
	n_line = split(line_patterns, line_pat, "\n")
	n_gate = split(gate_files, gate_arr, "\n")
	for (i = 1; i <= n_gate; i++) gate_set[gate_arr[i]] = 1
	n_violations = 0
	file = ""
}
/^diff --git a\// {
	file = $0
	sub(/^diff --git a\//, "", file)
	sub(/ b\/.*$/, "", file)
	if (file in gate_set) {
		violations[n_violations++] = file ": edits to a quality-gate config file"
	}
	next
}
/^(\+\+\+|---|@@|index )/ { next }
/^\+/ {
	body = substr($0, 2)
	for (i = 1; i <= n_line; i++) {
		if (line_pat[i] != "" && body ~ line_pat[i]) {
			violations[n_violations++] = file ": + matches /" line_pat[i] "/"
		}
	}
}
END {
	if (n_violations == 0) exit 0
	print "Quality-check suppressions detected in diff:" > "/dev/stderr"
	for (i = 0; i < n_violations; i++) print "  " violations[i] > "/dev/stderr"
	print "" > "/dev/stderr"
	print "Fix the underlying issue." > "/dev/stderr"
	exit 1
}
'
}

run_agent_scan() {
	local diff
	if ! diff="$(jj diff -r 'master..@' --git 2>/dev/null)"; then
		echo "agent mode requires jj (could not compute diff master..@)" >&2
		exit 1
	fi
	[[ -z "$diff" ]] && return 0
	printf '%s\n' "$diff" | scan_diff_for_suppressions
}

# ── individual gates ──────────────────────────────────────────────────────

check_rules_rs_macros() {
	if grep -rn 'load("@rules_rs//rs:rust_\(binary\|library\|test\)\.bzl"' \
		--include='BUILD.bazel' . 2>/dev/null; then
		echo "ERROR: Use //build:rust.bzl macros instead of @rules_rs directly." >&2
		return 1
	fi
}

check_package_readmes() {
	bash tools/require_readme_test.sh
}

run_format_check() {
	bazel run "${BAZEL_FLAGS[@]}" //:format.check
}

run_bazel_coverage() {
	# --lockfile_mode=error makes this script predict CI: a dep added without
	# committing the regenerated MODULE.bazel.lock fails here rather than
	# silently churning the lockfile.  Regenerate with
	# `bazel mod deps --lockfile_mode=update` and commit the result.
	bazel coverage --lockfile_mode=error "${BAZEL_FLAGS[@]}" "${BAZEL_TARGETS[@]}"
}

enforce_per_file_coverage() {
	if [[ ! -f "$REPORT" ]]; then
		echo "error: coverage report not found at $REPORT" >&2
		return 1
	fi

	local disk_files
	mapfile -t disk_files < <(find services lib/rust -name '*.rs' | sort)
	if [[ ${#disk_files[@]} -eq 0 ]]; then
		echo "error: no Rust source files found under services/ or lib/rust/" >&2
		return 1
	fi

	# Parse the LCOV report's LH/LF summary lines (lines hit / lines found).
	# Emit one row per Rust source: "<lh> <lf> <path>".
	local lcov_summary
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

	local failed=0
	local f s skip entry lh lf pct below
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
		return 1
	fi
	echo "check ok${CHANGE_ID:+ ($CHANGE_ID)}: ${#disk_files[@]} Rust source file(s) checked, all >= ${MIN_PCT}%"
}

run_all_gates() {
	check_rules_rs_macros
	check_package_readmes
	run_format_check
	run_bazel_coverage
	enforce_per_file_coverage
}

# ── dispatch ──────────────────────────────────────────────────────────────

if [[ "${_CHECK_SH_SOURCED:-}" == "1" ]]; then
	return 0
fi

if [[ $# -eq 0 ]]; then
	MODE="stack"
else
	MODE="$1"
	shift
	case "$MODE" in
	ci | stack | agent) ;;
	*)
		echo "tools/check.sh: unknown mode '$MODE' (expected ci or agent)" >&2
		exit 2
		;;
	esac
fi

BAZEL_FLAGS=()
BAZEL_TARGETS=()
for arg in "$@"; do
	if ((${#BAZEL_TARGETS[@]} == 0)) && [[ "$arg" == -* ]]; then
		BAZEL_FLAGS+=("$arg")
	else
		BAZEL_TARGETS+=("$arg")
	fi
done
((${#BAZEL_TARGETS[@]} > 0)) || BAZEL_TARGETS=("//...")

# Shortest unambiguous jj change id of @, for human-readable messages.
# Empty in CI mode and on machines without jj.
CHANGE_ID=""
if [[ "$MODE" != "ci" ]]; then
	CHANGE_ID="$(jj log -r @ --no-graph -T 'change_id.shortest()' 2>/dev/null || true)"
fi

case "$MODE" in
ci)
	run_all_gates
	;;
stack | agent)
	compute_cache_key "$MODE" "${BAZEL_FLAGS[@]}" "${BAZEL_TARGETS[@]}"
	if [[ -n "$CACHE_KEY" && -f "$GREEN_CACHE_DIR/$CACHE_KEY" ]]; then
		echo "check ok${CHANGE_ID:+ ($CHANGE_ID)}: unchanged since last green ($GREEN_CACHE_DIR/$CACHE_KEY)"
		exit 0
	fi
	if [[ "$MODE" == "agent" ]]; then
		run_agent_scan
	fi
	run_all_gates
	if [[ -n "$CACHE_KEY" ]]; then
		# Migrate the earlier single-file cache layout if present.
		[[ -f "$GREEN_CACHE_DIR" ]] && rm -f "$GREEN_CACHE_DIR"
		mkdir -p "$GREEN_CACHE_DIR"
		: >"$GREEN_CACHE_DIR/$CACHE_KEY"
	fi
	;;
esac
