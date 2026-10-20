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
#   agent     — the Claude Code hooks, `agent stop` and `agent push`: the
#               default gates plus a pre-flight scan that fails if the diff
#               between master and @ introduces a quality-check suppression
#               a human hasn't reviewed.  Takes no bazel arguments.
#   agent stop — the Claude Code Stop hook.  A failure the agent can fix
#               blocks the turn end (the agent is re-prompted); a failure
#               only the user can clear — an unauthorised quality-gate edit
#               — or a repeat of the last block on an unchanged tree ends the
#               turn with a message to the user, so the agent cannot loop on
#               it.
#   agent push — the PreToolUse hook on every Bash call.  Exits at once
#               unless the call's payload (stdin) is a `jj git push`; for a
#               push, a failure denies the call and ends the turn.
#   Both hooks always exit 0, because the harness reads a non-zero exit as a
#   broken hook rather than a verdict; the verdict is the JSON on stdout.
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
#   tools/check.sh agent stop
#   tools/check.sh agent push
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
# Returns 0 with CACHE_KEY set (empty when there is nothing to key on),
# 2 when the key is ambiguous and 3 when there is nothing to verify; for 2
# and 3 the text to show is in CACHE_KEY_MESSAGE.
CACHE_KEY=""
CACHE_KEY_MESSAGE=""
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
		if ! parent_ids="$(jj log -r "$target-" --no-graph -T 'commit_id ++ "\n"' 2>/dev/null)"; then
			CACHE_KEY_MESSAGE="error: could not list the parents of $target"
			return 2
		fi
		parent_count="$(printf '%s\n' "$parent_ids" | awk 'NF{n++} END{print n+0}')"
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
				CACHE_KEY_MESSAGE="check skipped${CHANGE_ID:+ ($CHANGE_ID)}: nothing to verify — $target is in immutable history with empty diff and $parent_count parent(s)"
				return 3
			fi
			CACHE_KEY_MESSAGE="error: $target has empty diff and $parent_count parent(s); cannot determine what to verify"
			return 2
		fi
		target="${target}-"
	done
	if ! diff_hash="$(jj diff --git -r "$target" 2>/dev/null |
		sed -E -e '/^index [0-9a-f]+\.\.[0-9a-f]+/d' \
			-e 's/^@@ -[0-9,]+ \+[0-9,]+ @@/@@/' |
		sha256sum | awk '{print $1}')"; then
		CACHE_KEY_MESSAGE="error: could not hash the diff of $target"
		return 2
	fi
	# Lockfile state is folded into the key so that a stack rewrite which
	# leaves a commit's patch unchanged but shifts the lockfile content
	# (e.g. bumping a workspace dep in an ancestor) invalidates the cached
	# verdict.
	if ! lockfile_hash="$(sha256sum MODULE.bazel.lock Cargo.lock requirements_lock.txt 2>/dev/null |
		sha256sum | awk '{print $1}')"; then
		CACHE_KEY_MESSAGE="error: could not hash the lockfiles"
		return 2
	fi
	CACHE_KEY="$(printf '%s\t%s\t%s\t%s' "$diff_hash" "$lockfile_hash" "$mode" "$*" |
		sha256sum | awk '{print $1}')"
}

# ── agent-mode suppression scan ───────────────────────────────────────────
#
# Reads a git-format diff from stdin and prints a violation list to stderr.
# Exits 0 if clean, 1 if any suppression is introduced, 3 when every
# violation is an unauthorised gate edit.  Pulled out as a function so the
# sh_test can drive it with synthetic diffs.
scan_diff_for_suppressions() {
	awk \
		-v line_patterns="$(printf '%s\n' "${SUPPRESSION_LINE_PATTERNS[@]}")" \
		-v gate_files="$(printf '%s\n' "${SUPPRESSION_GATE_FILES[@]}")" \
		-v authorised="${AUTHORISED_GATE_FILES:-}" \
		-v authorise_hint="${AUTHORISE_HINT:-}" \
		-v bazelrc_patterns="$(printf '%s\n' "${SUPPRESSION_BAZELRC_PATTERNS[@]}")" \
		-v buildbazel_patterns="$(printf '%s\n' "${SUPPRESSION_BUILDBAZEL_PATTERNS[@]}")" \
		-v master_exts="${MASTER_FILE_EXTENSIONS:-}" '
BEGIN {
	n_line = split(line_patterns, line_pat, "\n")
	n_gate = split(gate_files, gate_arr, "\n")
	for (i = 1; i <= n_gate; i++) gate_set[gate_arr[i]] = 1
	n_auth = split(authorised, auth_arr, "\n")
	for (i = 1; i <= n_auth; i++) auth_set[auth_arr[i]] = 1
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
	# Buck2 root-config files (.buckversion/.buckroot/.buckconfig) use
	# extensions absent from master but are legitimate, not a new language.
	master_ext_set["buckversion"] = 1
	master_ext_set["buckroot"] = 1
	master_ext_set["buckconfig"] = 1
	# json5: the NativeLink config format (JSON with comments), not a language.
	master_ext_set["json5"] = 1
	# shlib: sourced shell libraries; shell is already an in-tree language.
	master_ext_set["shlib"] = 1
	n_violations = 0
	n_unauthorised = 0
	file = ""
	is_bazelrc = 0
	is_buildbazel = 0
}
/^diff --git a\// {
	file = $0
	sub(/^diff --git a\//, "", file)
	sub(/ b\/.*$/, "", file)
	if ((file in gate_set) && !(file in auth_set)) {
		hint = authorise_hint
		sub(/\{path\}/, file, hint)
		violations[n_violations++] = file ": edits a quality-gate file; the user authorises it with `" hint "`"
		n_unauthorised++
	}
	base = file
	sub(/.*\//, "", base)
	is_bazelrc = (file == ".bazelrc" || file == ".bazelrc.user")
	is_buildbazel = (base == "BUILD.bazel")
	# The vendored Buck2 prelude is third-party code, not agent-authored;
	# its many fixture extensions are not a new-language bypass signal.
	is_vendored = (file ~ /^prelude\//)
	# New-language detection: extension not seen in master is a likely
	# bypass via the simple "switch language to dodge the existing gates"
	# route. Only runs when the harness supplied master_exts (i.e. agent
	# mode against a real jj tree, not synthetic test diffs).
	if (have_master_exts && !is_vendored && index(base, ".") > 0) {
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
	# 3: every violation is an unauthorised gate edit, which only the user
	# can clear; 1: at least one the author must fix.
	if (n_unauthorised == n_violations) exit 3
	print "Fix the underlying issue." > "/dev/stderr"
	exit 1
}
'
}

run_agent_scan() {
	local diff
	if ! diff="$(jj diff -r 'master..@' --git 2>/dev/null)"; then
		echo "agent mode requires jj (could not compute diff master..@)" >&2
		return 1
	fi
	[[ -z "$diff" ]] && return 0
	# Comma-separated set of extensions present in master, used by the
	# new-language detector. Computed once per agent run; cheap (a single
	# jj invocation listing tracked paths).
	local master_exts master_files
	if ! master_files="$(jj file list --ignore-working-copy -r master)"; then
		echo "agent mode requires jj (could not list master files)" >&2
		return 1
	fi
	master_exts="$(printf '%s\n' "$master_files" |
		awk -F/ '{print $NF}' |
		awk -F. 'NF > 1 { print $NF }' |
		sort -u | tr '\n' ',')"
	local gate_files="" authorised="" path hint
	while IFS= read -r path; do
		if python3 tools/authorise.py --match "$path"; then
			gate_files+="$path"$'\n'
			if python3 tools/authorise.py --verify "$path"; then
				authorised+="$path"$'\n'
			fi
		fi
	done < <(printf '%s\n' "$diff" | sed -n 's|^diff --git a/\(.*\) b/.*$|\1|p')
	if ! hint="$(python3 tools/authorise.py --hint)"; then
		echo "agent mode requires tools/authorise.py (could not get the authorise hint)" >&2
		return 1
	fi
	mapfile -t SUPPRESSION_GATE_FILES < <(printf '%s' "$gate_files")
	AUTHORISED_GATE_FILES="$authorised" AUTHORISE_HINT="$hint" MASTER_FILE_EXTENSIONS="$master_exts" \
		scan_diff_for_suppressions <<<"$diff"
}

# ── individual gates ──────────────────────────────────────────────────────

check_rules_rs_macros() {
	local files
	# List tracked BUILD.bazel files: jj locally, git in CI (no jj there).
	if command -v jj >/dev/null; then
		files="$(jj file list --ignore-working-copy 'glob:**/BUILD.bazel')"
	else
		files="$(git ls-files ':(glob)**/BUILD.bazel')"
	fi
	if [[ -z "$files" ]]; then
		return 0
	fi
	local -a filelist
	mapfile -t filelist <<<"$files"
	local rc=0
	grep -n 'load("@rules_rs//rs:rust_\(binary\|library\|test\)\.bzl"' \
		-- "${filelist[@]}" || rc=$?
	case "$rc" in
	0)
		echo "ERROR: Use //build:rust.bzl macros instead of @rules_rs directly." >&2
		return 1
		;;
	1) ;;
	*)
		echo "check_rules_rs_macros: grep errored ($rc)" >&2
		return 1
		;;
	esac
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
# Names of the gates that returned nonzero, populated by run_gate.
FAILED_GATES=()

run_gate() {
	local name="$1"
	shift
	local start end elapsed rc=0
	start="$EPOCHREALTIME"
	"$@" || rc=$?
	end="$EPOCHREALTIME"
	elapsed=$(awk "BEGIN {printf \"%.3f\", $end - $start}")
	TIMINGS+=("$name"$'\t'"$rc"$'\t'"$elapsed")
	[[ "$rc" == 0 ]] || FAILED_GATES+=("$name")
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

# How the Stop hook answers scan/gate results $1/$2; $3 is 1 when the same
# tree was already blocked once.  Prints one of clean, user (only the user
# can clear it), repeat, block.
stop_class() {
	local scan_rc="$1" gates_rc="$2" repeated="$3"
	if [[ "$scan_rc" == 0 && "$gates_rc" == 0 ]]; then
		echo clean
	elif [[ "$scan_rc" == 3 && "$gates_rc" == 0 ]]; then
		echo user
	elif [[ "$repeated" == 1 ]]; then
		echo repeat
	else
		echo block
	fi
}

# The JSON string literal for $1, so the hook's envelope needs only bash.
json_string() {
	local s="$1"
	s="${s//\\/\\\\}"
	s="${s//\"/\\\"}"
	s="${s//$'\n'/\\n}"
	s="${s//$'\r'/\\r}"
	s="${s//$'\t'/\\t}"
	printf '"%s"' "$s"
}

# The Stop hook's JSON for stop_class's arguments; $4 is the detail text.
# Only a `decision: block` re-prompts the agent.
stop_envelope() {
	local detail="$4"
	case "$(stop_class "$1" "$2" "$3")" in
	clean) ;;
	user)
		printf '{"systemMessage":%s}\n' "$(json_string "$detail")"
		;;
	repeat)
		printf '{"systemMessage":%s}\n' \
			"$(json_string "End-of-turn gate still failing on an unchanged tree; stopping:"$'\n'"$detail")"
		;;
	block)
		printf '{"decision":"block","reason":%s}\n' \
			"$(json_string "End-of-turn gate failed — fix the issue before finishing:"$'\n'"$detail")"
		;;
	esac
}

# Whether the PreToolUse payload on stdin is a `jj git push`.
# A match on the raw JSON is enough: a false positive costs one gate run.
push_requested() {
	local payload
	payload="$(cat)"
	[[ "$payload" =~ jj.*git.*push ]]
}

# The push hook's JSON for a failed run; $1 is the detail text.
# The deny stops the push; `continue: false` ends the turn.
push_envelope() {
	local reason
	reason="$(json_string "Pre-push gate failed — fix the issue before pushing:"$'\n'"$1")"
	printf '{"continue":false,"stopReason":%s,"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny"}}\n' \
		"$reason"
}

# stop_key <cache_key> <scan_out> <failed_gates…>
# The loop breaker's key for the tree: the cache key, or when there is none
# (conflicted @, or jj unavailable) a hash of what failed.
stop_key() {
	local cache_key="$1" scan_out="$2"
	shift 2
	if [[ -n "$cache_key" ]]; then
		echo "$cache_key"
	else
		printf '%s\n%s' "$scan_out" "$*" | sha256sum | cut -c1-64
	fi
}

# stop_decide <key> <state_file> <scan_rc> <gates_rc> <detail>
# Emits the Stop hook's JSON and keeps the loop breaker's state: the state
# file holds the key of the last block, so a second failure with the same
# key means the agent ended a turn without changing anything and the turn
# ends.  Every outcome that ends the turn clears the state file.
stop_decide() {
	local key="$1" state_file="$2" scan_rc="$3" gates_rc="$4" detail="$5"
	local repeated=0
	if [[ -f "$state_file" && "$(cat "$state_file")" == "$key" ]]; then
		repeated=1
	fi
	stop_envelope "$scan_rc" "$gates_rc" "$repeated" "$detail"
	if [[ "$(stop_class "$scan_rc" "$gates_rc" "$repeated")" == block ]]; then
		mkdir -p "$(dirname "$state_file")"
		printf '%s' "$key" >"$state_file"
	else
		rm -f "$state_file"
	fi
}

run_all_gates() {
	if [[ -n "${CHECK_TIMING_JSONL:-}" ]]; then
		: >"$CHECK_TIMING_JSONL"
	fi
	run_gate rules_rs_macros check_rules_rs_macros || return $?
	run_gate package_readmes check_package_readmes || return $?
	run_gate bazel_quiet check_bazel_quiet || return $?
	run_gate format_check run_format_check || return $?
	run_gate bazel_coverage run_bazel_coverage || return $?
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

HOOK=""
if [[ "$MODE" == "agent" ]]; then
	case "${1:-}" in
	stop | push) HOOK="$1" ;;
	*)
		echo "tools/check.sh: agent takes exactly one of: stop, push" >&2
		exit 2
		;;
	esac
	shift
	if [[ $# -gt 0 ]]; then
		echo "tools/check.sh: agent $HOOK takes no arguments" >&2
		exit 2
	fi
	if [[ "$HOOK" == "push" ]] && ! push_requested; then
		exit 0
	fi
fi

# A hook's stdout is its JSON envelope, so everything else goes to stderr.
if [[ -n "$HOOK" ]]; then
	exec 3>&2
else
	exec 3>&1
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
	STOP_STATE="$GREEN_CACHE_DIR/last-block"
	cache_rc=0
	compute_cache_key "$MODE" "${BAZEL_FLAGS[@]}" "${BAZEL_TARGETS[@]}" || cache_rc=$?
	case "$cache_rc" in
	0) ;;
	3)
		echo "$CACHE_KEY_MESSAGE" >&3
		rm -f "$STOP_STATE"
		exit 0
		;;
	*)
		if [[ "$HOOK" == "stop" ]]; then
			printf '{"systemMessage":%s}\n' "$(json_string "$CACHE_KEY_MESSAGE")"
			rm -f "$STOP_STATE"
			exit 0
		fi
		if [[ "$HOOK" == "push" ]]; then
			push_envelope "$CACHE_KEY_MESSAGE"
			exit 0
		fi
		echo "$CACHE_KEY_MESSAGE" >&2
		exit 1
		;;
	esac
	if [[ -n "$CACHE_KEY" && -f "$GREEN_CACHE_DIR/$CACHE_KEY" ]]; then
		echo "check ok${CHANGE_ID:+ ($CHANGE_ID)}: unchanged since last green ($GREEN_CACHE_DIR/$CACHE_KEY)" >&3
		rm -f "$STOP_STATE"
		exit 0
	fi
	scan_rc=0
	scan_out=""
	if [[ "$MODE" == "agent" ]]; then
		scan_out="$(run_agent_scan 2>&1)" || scan_rc=$?
		if [[ -n "$scan_out" ]]; then
			printf '%s\n' "$scan_out" >&2
		fi
	fi
	gates_rc=0
	run_all_gates >&3 || gates_rc=$?
	detail="$scan_out"
	if ((${#FAILED_GATES[@]} > 0)); then
		detail+="${detail:+$'\n'}failed gates: ${FAILED_GATES[*]}"
	fi
	if [[ "$HOOK" == "stop" ]]; then
		key="$(stop_key "$CACHE_KEY" "$scan_out" "${FAILED_GATES[@]}")"
		stop_decide "$key" "$STOP_STATE" "$scan_rc" "$gates_rc" "$detail"
	fi
	if [[ "$scan_rc" != 0 || "$gates_rc" != 0 ]]; then
		[[ "$HOOK" == "stop" ]] && exit 0
		if [[ "$HOOK" == "push" ]]; then
			push_envelope "$detail"
			exit 0
		fi
		exit 1
	fi
	if [[ -n "$CACHE_KEY" ]]; then
		# Migrate the earlier single-file cache layout if present.
		[[ -f "$GREEN_CACHE_DIR" ]] && rm -f "$GREEN_CACHE_DIR"
		mkdir -p "$GREEN_CACHE_DIR"
		: >"$GREEN_CACHE_DIR/$CACHE_KEY"
	fi
	;;
esac
