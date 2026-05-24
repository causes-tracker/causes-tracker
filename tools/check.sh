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
#   3. Output-captured `bazel run|build` invocations use `--quiet`.
#   4. //:format.check (every formatter the project configures).
#   5. bazel coverage --lockfile_mode=error (runs every test).
#   6. Per-file Rust coverage threshold (MIN_PCT).
#
# Usage:
#   tools/check.sh ci    [bazel-flags...] [target...]
#   tools/check.sh       [bazel-flags...] [target...]
#   tools/check.sh agent [bazel-flags...] [target...]
#
# Anything starting with `-` is treated as a bazel flag (forwarded to
# format.check and coverage); the rest is treated as bazel targets
# (forwarded only to coverage).  Default target is //....
#
# A per-gate timing summary is printed to stderr at exit.  Setting
# CHECK_TIMING_JSONL=<path> additionally appends one JSON object per
# gate (`{"gate":"…","rc":N,"seconds":N.NNN}`) to that path.

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
# Backslashes are doubled: `awk -v var=value` strips one level of escapes
# before the value becomes a regex, and stricter awks (CI's mawk emits
# `escape sequence '\[' treated as plain '['`) otherwise leave a bare `[`
# in the pattern and fail to compile it.
SUPPRESSION_LINE_PATTERNS=(
	'# *shellcheck +disable'        # bash
	'#!?\\[allow\\('                # rust attribute (item or crate)
	'#\\[ignore(\\]|\\()'           # rust #[ignore] or #[ignore("...")]
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

# Patterns matched on `+` lines whose file is `.bazelrc` or `.bazelrc.user`.
# Targeted at lint-allow flags (rustc/clippy `-A`); we do NOT flag the file
# itself, since legitimate cache/network/toolchain config lives here too.
SUPPRESSION_BAZELRC_PATTERNS=(
	'(^|[ \t=,])-A(warnings|clippy|[ \t=,])' # rustc/clippy lint-allow
)

# Patterns matched on `+` lines whose basename is `BUILD.bazel`.
# These are the test-evasion attributes a human reviewer would want to see
# flagged at the door, since they detach a target from the default build.
SUPPRESSION_BUILDBAZEL_PATTERNS=(
	'tags *= *\\[[^]]*"(manual|no-ci|flaky)"' # opt-out tags on tests
	'target_compatible_with *= *'             # platform-incompatible carve-out
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
		-v gate_files="$(printf '%s\n' "${SUPPRESSION_GATE_FILES[@]}")" \
		-v bazelrc_patterns="$(printf '%s\n' "${SUPPRESSION_BAZELRC_PATTERNS[@]}")" \
		-v buildbazel_patterns="$(printf '%s\n' "${SUPPRESSION_BUILDBAZEL_PATTERNS[@]}")" \
		-v master_exts="${MASTER_FILE_EXTENSIONS:-}" '
BEGIN {
	n_line = split(line_patterns, line_pat, "\n")
	n_gate = split(gate_files, gate_arr, "\n")
	for (i = 1; i <= n_gate; i++) gate_set[gate_arr[i]] = 1
	n_bazelrc = split(bazelrc_patterns, bazelrc_pat, "\n")
	n_buildbazel = split(buildbazel_patterns, buildbazel_pat, "\n")
	n_master_ext = split(master_exts, ext_arr, ",")
	have_master_exts = 0
	for (i = 1; i <= n_master_ext; i++) {
		if (ext_arr[i] != "") {
			master_ext_set[ext_arr[i]] = 1
			have_master_exts = 1
		}
	}
	n_violations = 0
	file = ""
	is_bazelrc = 0
	is_buildbazel = 0
}
/^diff --git a\// {
	file = $0
	sub(/^diff --git a\//, "", file)
	sub(/ b\/.*$/, "", file)
	if (file in gate_set) {
		violations[n_violations++] = file ": edits to a quality-gate config file"
	}
	base = file
	sub(/.*\//, "", base)
	is_bazelrc = (file == ".bazelrc" || file == ".bazelrc.user")
	is_buildbazel = (base == "BUILD.bazel")
	# New-language detection: extension not seen in master is a likely
	# bypass via the simple "switch language to dodge the existing gates"
	# route. Only runs when the harness supplied master_exts (i.e. agent
	# mode against a real jj tree, not synthetic test diffs).
	if (have_master_exts && index(base, ".") > 0) {
		ext = base
		sub(/^.*\./, "", ext)
		if (!(ext in master_ext_set)) {
			violations[n_violations++] = file ": introduces file extension '." ext "' not present in master (new language?)"
		}
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
	if (is_bazelrc) {
		for (i = 1; i <= n_bazelrc; i++) {
			if (bazelrc_pat[i] != "" && body ~ bazelrc_pat[i]) {
				violations[n_violations++] = file ": + matches /" bazelrc_pat[i] "/ (lint-allow flag in bazelrc)"
			}
		}
	}
	if (is_buildbazel) {
		for (i = 1; i <= n_buildbazel; i++) {
			if (buildbazel_pat[i] != "" && body ~ buildbazel_pat[i]) {
				violations[n_violations++] = file ": + matches /" buildbazel_pat[i] "/ (suspect BUILD attribute)"
			}
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
	# Comma-separated set of extensions present in master, used by the
	# new-language detector. Computed once per agent run; cheap (a single
	# jj invocation listing tracked paths).
	local master_exts
	master_exts="$(jj file list --ignore-working-copy -r master 2>/dev/null |
		awk -F/ '{print $NF}' |
		awk -F. 'NF > 1 { print $NF }' |
		sort -u | tr '\n' ',')"
	MASTER_FILE_EXTENSIONS="$master_exts" \
		scan_diff_for_suppressions <<<"$diff"
}

# ── individual gates ──────────────────────────────────────────────────────

check_rules_rs_macros() {
	local files
	files="$(jj file list --ignore-working-copy 'glob:**/BUILD.bazel' 2>/dev/null)"
	if [[ -z "$files" ]]; then
		return 0
	fi
	if echo "$files" | xargs grep -n \
		'load("@rules_rs//rs:rust_\(binary\|library\|test\)\.bzl"' 2>/dev/null; then
		echo "ERROR: Use //build:rust.bzl macros instead of @rules_rs directly." >&2
		return 1
	fi
}

check_package_readmes() {
	bash tools/require_readme_test.sh
}

check_bazel_quiet() {
	bash tools/require_bazel_quiet_test.sh
}

run_format_check() {
	# Match the analysis-config hash used by run_bazel_coverage so the analysis
	# cache survives between gates instead of being discarded each turn (the
	# "Build options ... have changed, discarding analysis cache" warning).
	# These flags are no-ops for a `bazel run` of a sh_binary but they're part
	# of the analysis key.
	bazel run \
		--collect_code_coverage \
		--instrumentation_filter='^//' \
		--action_env=GENERATE_LLVM_LCOV=1 \
		--action_env=COVERAGE_GCOV_PATH=/usr/bin/gcov \
		"${BAZEL_FLAGS[@]}" //:format.check
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

# Per-gate wall-clock timings, populated by run_gate.
# Emitted as a summary to stderr at exit (via the EXIT trap below) and,
# when CHECK_TIMING_JSONL is set, appended as one JSON object per gate
# to that path.
TIMINGS=()

run_gate() {
	local name="$1"
	shift
	local start end elapsed rc=0
	start="$EPOCHREALTIME"
	"$@" || rc=$?
	end="$EPOCHREALTIME"
	elapsed=$(awk "BEGIN {printf \"%.3f\", $end - $start}")
	TIMINGS+=("$name"$'\t'"$rc"$'\t'"$elapsed")
	if [[ -n "${CHECK_TIMING_JSONL:-}" ]]; then
		jq -nc \
			--arg gate "$name" \
			--argjson rc "$rc" \
			--argjson seconds "$elapsed" \
			'{gate: $gate, rc: $rc, seconds: $seconds}' \
			>>"$CHECK_TIMING_JSONL"
	fi
	return $rc
}

emit_timing_summary() {
	[[ ${#TIMINGS[@]} -eq 0 ]] && return 0
	local total=0 t n r s
	printf '\nGate timings:\n' >&2
	for t in "${TIMINGS[@]}"; do
		IFS=$'\t' read -r n r s <<<"$t"
		printf '  %-24s rc=%s %8ss\n' "$n" "$r" "$s" >&2
		total=$(awk "BEGIN {printf \"%.3f\", $total + $s}")
	done
	printf '  %-24s     %8ss\n' "TOTAL" "$total" >&2
}
trap emit_timing_summary EXIT

run_all_gates() {
	if [[ -n "${CHECK_TIMING_JSONL:-}" ]]; then
		: >"$CHECK_TIMING_JSONL"
	fi
	run_gate rules_rs_macros check_rules_rs_macros
	run_gate package_readmes check_package_readmes
	run_gate bazel_quiet check_bazel_quiet
	run_gate format_check run_format_check
	run_gate bazel_coverage run_bazel_coverage
	run_gate per_file_coverage enforce_per_file_coverage
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
