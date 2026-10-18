#!/usr/bin/env bash
# Test driver for tools/check.sh's sourceable pieces: the suppression scanner
# (synthetic git-format diffs on stdin; exit 0 = clean, 1 = suppression, 3 =
# only unauthorised gate edits), the Stop-hook envelope and loop breaker, and
# run_all_gates' fail-fast sequencing.  Sources check.sh with
# _CHECK_SH_SOURCED=1 to skip the dispatch flow.
set -uo pipefail

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

CHECK_SH="$(rlocation _main/tools/check.sh)"
if [[ ! -f "$CHECK_SH" ]]; then
	echo "ERROR: cannot locate tools/check.sh via runfiles" >&2
	exit 1
fi

# shellcheck source=check.sh
_CHECK_SH_SOURCED=1 source "$CHECK_SH"

SUPPRESSION_GATE_FILES=(
	"gate/a"
	"gate/b.yml"
	"tools/gate.sh"
)

PASS=0
FAIL=0

case_pass() { case_run "$1" 0 "$2"; }
case_fail() { case_run "$1" 1 "$2"; }
# 3: only unauthorised gate edits, which the user clears rather than the author.
case_user() { case_run "$1" 3 "$2"; }
case_run() {
	local name="$1" expected="$2" diff="$3"
	local out rc=0
	out="$(printf '%s' "$diff" | scan_diff_for_suppressions 2>&1)" || rc=$?
	if [[ "$rc" == "$expected" ]]; then
		echo "PASS: $name"
		((PASS++)) || true
	else
		echo "FAIL: $name — expected rc=$expected, got $rc"
		echo "----- scanner output -----"
		printf '%s\n' "$out"
		echo "--------------------------"
		((FAIL++)) || true
	fi
}

# ── clean / empty ─────────────────────────────────────────────────────────

case_pass "empty stdin" ""

case_pass "clean source modification" 'diff --git a/src/foo.rs b/src/foo.rs
index 1..2 100644
--- a/src/foo.rs
+++ b/src/foo.rs
@@ -1,3 +1,3 @@
 fn main() {
-    let x = 1;
+    let x = 2;
 }
'

# ── source-line markers (one per case) ────────────────────────────────────

case_fail "+ # shellcheck disable" 'diff --git a/x.sh b/x.sh
--- a/x.sh
+++ b/x.sh
@@ -1 +1,2 @@
 echo hi
+# shellcheck disable=SC2086
'

case_fail "+ #[allow(...)]" 'diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1,2 @@
 fn x() {}
+#[allow(dead_code)]
'

case_fail "+ #![allow(...)] (crate-level)" 'diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1,2 @@
 fn x() {}
+#![allow(unused)]
'

case_fail "+ #[ignore]" 'diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1,2 @@
 #[test]
+#[ignore]
'

case_fail "+ #[ignore(reason)]" 'diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1,2 @@
 #[test]
+#[ignore("not yet")]
'

case_fail "+ // eslint-disable-next-line" 'diff --git a/x.ts b/x.ts
--- a/x.ts
+++ b/x.ts
@@ -1 +1,2 @@
 const x = 1;
+// eslint-disable-next-line
'

case_fail "+ // @ts-ignore" 'diff --git a/x.ts b/x.ts
--- a/x.ts
+++ b/x.ts
@@ -1 +1,2 @@
 const x = 1;
+// @ts-ignore
'

case_fail "+ // @ts-expect-error" 'diff --git a/x.ts b/x.ts
--- a/x.ts
+++ b/x.ts
@@ -1 +1,2 @@
 const x = 1;
+// @ts-expect-error
'

case_fail "+ # noqa: F401" 'diff --git a/x.py b/x.py
--- a/x.py
+++ b/x.py
@@ -1 +1,2 @@
 import x
+import unused  # noqa: F401
'

case_fail "+ # type: ignore[...]" 'diff --git a/x.py b/x.py
--- a/x.py
+++ b/x.py
@@ -1 +1,2 @@
 x = 1
+y: Any = foo()  # type: ignore[arg-type]
'

# ── gate-config edits / creations ─────────────────────────────────────────

case_user "gate file created" 'diff --git a/gate/a b/gate/a
new file mode 100644
--- /dev/null
+++ b/gate/a
@@ -0,0 +1 @@
+some/skipped/path
'

case_user "gate file edited" 'diff --git a/tools/gate.sh b/tools/gate.sh
--- a/tools/gate.sh
+++ b/tools/gate.sh
@@ -1 +1 @@
-MIN_PCT=25
+MIN_PCT=10
'

case_user "gate file in a subdirectory edited" 'diff --git a/gate/b.yml b/gate/b.yml
--- a/gate/b.yml
+++ b/gate/b.yml
@@ -1 +1 @@
-on: push
+on: workflow_dispatch
'

case_pass "file sharing a gate basename passes" 'diff --git a/other/gate.sh b/other/gate.sh
--- a/other/gate.sh
+++ b/other/gate.sh
@@ -1 +1 @@
-a=1
+a=2
'

# ── false-positive guards ─────────────────────────────────────────────────

case_pass "context line with shellcheck disable" 'diff --git a/x.sh b/x.sh
--- a/x.sh
+++ b/x.sh
@@ -1,2 +1,2 @@
 # shellcheck disable=SC2086
-old
+new
'

case_pass "removed line with #[allow]" 'diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1,2 +1 @@
-#[allow(dead_code)]
 fn foo() {}
'

case_pass "identifier containing noqa (no comment context)" 'diff --git a/x.sh b/x.sh
--- a/x.sh
+++ b/x.sh
@@ -1 +1,2 @@
 echo hi
+function show_noqa_doc() { :; }
'

case_pass "+++ b/file diff header is not content" 'diff --git a/regular/file.rs b/regular/file.rs
--- a/regular/file.rs
+++ b/regular/file.rs
@@ -1 +1 @@
-let x = 1;
+let x = 2;
'

case_pass "tools/foo.sh (sibling of gate path) passes" 'diff --git a/tools/foo.sh b/tools/foo.sh
--- a/tools/foo.sh
+++ b/tools/foo.sh
@@ -1 +1 @@
-old
+new
'

# An authorised gate file (the user minted a token for it this hour) passes.
AUTHORISED_GATE_FILES=$'tools/gate.sh\n' case_pass "authorised gate file passes" 'diff --git a/tools/gate.sh b/tools/gate.sh
--- a/tools/gate.sh
+++ b/tools/gate.sh
@@ -1 +1 @@
-MIN_PCT=25
+MIN_PCT=10
'

AUTHORISED_GATE_FILES=$'tools/gate.sh\n' case_user "authorisation covers only its own file" 'diff --git a/gate/a b/gate/a
--- a/gate/a
+++ b/gate/a
@@ -1 +1 @@
-a = 1
+a = 2
'

# ── multi-violation: every violation listed, not just the first ───────────

case_fail "two suppressions on two added lines" 'diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1,3 @@
 fn x() {}
+#[allow(dead_code)]
+#[ignore]
'

case_fail "an unauthorised gate edit beside a suppression is the author's" 'diff --git a/gate/a b/gate/a
--- a/gate/a
+++ b/gate/a
@@ -1 +1 @@
-a = 1
+a = 2
diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1,2 @@
 fn x() {}
+#[allow(dead_code)]
'

# ── .bazelrc content (file edits OK, lint-allow flags NOT) ────────────────

case_fail ".bazelrc adds -A warnings" 'diff --git a/.bazelrc b/.bazelrc
--- a/.bazelrc
+++ b/.bazelrc
@@ -1 +1,2 @@
 build --remote_cache=https://example
+build --@rules_rust//:extra_rustc_flags=-A,warnings
'

case_fail ".bazelrc.user adds -A clippy::all" 'diff --git a/.bazelrc.user b/.bazelrc.user
--- a/.bazelrc.user
+++ b/.bazelrc.user
@@ -1 +1,2 @@
 build --jobs=8
+build --rustc-flag=-Aclippy::all
'

case_pass ".bazelrc benign cache config" 'diff --git a/.bazelrc b/.bazelrc
--- a/.bazelrc
+++ b/.bazelrc
@@ -1 +1,2 @@
 build --remote_cache=https://example
+build --remote_upload_local_results=true
'

# ── BUILD.bazel suspect attributes ────────────────────────────────────────

case_fail "BUILD.bazel adds tags = [\"manual\"]" 'diff --git a/lib/x/BUILD.bazel b/lib/x/BUILD.bazel
--- a/lib/x/BUILD.bazel
+++ b/lib/x/BUILD.bazel
@@ -1,3 +1,4 @@
 rust_test(
     name = "foo",
+    tags = ["manual"],
 )
'

case_fail "BUILD.bazel adds target_compatible_with" 'diff --git a/lib/x/BUILD.bazel b/lib/x/BUILD.bazel
--- a/lib/x/BUILD.bazel
+++ b/lib/x/BUILD.bazel
@@ -1,3 +1,4 @@
 rust_test(
     name = "foo",
+    target_compatible_with = ["@platforms//os:macos"],
 )
'

case_pass "BUILD.bazel adds a normal dep" 'diff --git a/lib/x/BUILD.bazel b/lib/x/BUILD.bazel
--- a/lib/x/BUILD.bazel
+++ b/lib/x/BUILD.bazel
@@ -1,3 +1,4 @@
 rust_test(
     name = "foo",
+    deps = ["//lib/bar"],
 )
'

# ── new-language detection (driven by MASTER_FILE_EXTENSIONS) ─────────────

run_with_master_exts() {
	local name="$1" expected="$2" diff="$3"
	local out rc=0
	out="$(printf '%s' "$diff" |
		MASTER_FILE_EXTENSIONS="rs,sh,py,toml,md,bzl" scan_diff_for_suppressions 2>&1)" || rc=$?
	if [[ "$rc" == "$expected" ]]; then
		echo "PASS: $name"
		((PASS++)) || true
	else
		echo "FAIL: $name — expected rc=$expected, got $rc"
		printf '%s\n' "$out"
		((FAIL++)) || true
	fi
}
case_fail_with_master_exts() { run_with_master_exts "$1" 1 "$2"; }
case_pass_with_master_exts() { run_with_master_exts "$1" 0 "$2"; }

case_fail_with_master_exts "new .go file flagged when master lacks .go" 'diff --git a/lib/x/main.go b/lib/x/main.go
new file mode 100644
--- /dev/null
+++ b/lib/x/main.go
@@ -0,0 +1,1 @@
+package main
'

case_pass_with_master_exts "new .rs file fine (extension exists in master)" 'diff --git a/lib/x/y.rs b/lib/x/y.rs
new file mode 100644
--- /dev/null
+++ b/lib/x/y.rs
@@ -0,0 +1,1 @@
+fn foo() {}
'

case_pass_with_master_exts "modify .rs file (existing extension)" 'diff --git a/lib/x/y.rs b/lib/x/y.rs
--- a/lib/x/y.rs
+++ b/lib/x/y.rs
@@ -1 +1 @@
-fn foo() {}
+fn bar() {}
'

case_pass_with_master_exts "new .shlib file fine (seeded allowlist)" 'diff --git a/tools/x.shlib b/tools/x.shlib
new file mode 100644
--- /dev/null
+++ b/tools/x.shlib
@@ -0,0 +1,1 @@
+foo() { :; }
'

# Without master_exts set, language detector is a no-op (default fall-back
# for synthetic-diff tests that don't care about the language gate).
case_pass "new .go file passes when master_exts unset" 'diff --git a/lib/x/main.go b/lib/x/main.go
new file mode 100644
--- /dev/null
+++ b/lib/x/main.go
@@ -0,0 +1,1 @@
+package main
'

# ── Stop-hook envelope ────────────────────────────────────────────────────

# The hook runs on machines where only bash is guaranteed.
jq() {
	echo "jq is not available to check.sh" >&2
	return 127
}

# Classifies hook output $2 as silent, block, message or malformed; block
# and message require the detail text $1 to appear, JSON-escaped, in the
# envelope's single string.
envelope_class() {
	local detail="$1" out="$2" esc
	esc="$(json_string "$detail")"
	esc="${esc:1:-1}"
	if [[ -z "$out" ]]; then
		echo silent
	elif [[ "$out" == '{"decision":"block","reason":"'*"$esc"*'"}' ]]; then
		echo block
	elif [[ "$out" == '{"systemMessage":"'*"$esc"*'"}' ]]; then
		echo message
	else
		echo malformed
	fi
}

report() {
	local name="$1" want="$2" got="$3"
	if [[ "$got" == "$want" ]]; then
		echo "PASS: $name"
		((PASS++)) || true
	else
		echo "FAIL: $name — want $want, got $got"
		((FAIL++)) || true
	fi
}

envelope_case() {
	local name="$1" want="$2" detail="$6"
	shift 2
	report "$name" "$want" "$(envelope_class "$detail" "$(stop_envelope "$@")")"
}

report "json_string escapes what JSON requires" \
	'"a\"b\\c\nd\te"' "$(json_string "a\"b\\c"$'\n'"d"$'\t'"e")"

envelope_case "clean run is silent" silent 0 0 0 ""
envelope_case "fixable scan failure blocks" block 1 0 0 "detail"
envelope_case "gate failure blocks" block 0 1 0 "detail"
envelope_case "unauthorised gate edits alone end the turn" message 3 0 0 "detail"
envelope_case "unauthorised gate edits plus a gate failure block" block 3 1 0 "detail"
envelope_case "a repeat block on an unchanged tree ends the turn" message 1 0 1 "detail"
envelope_case "repeat with gate failure ends the turn" message 0 1 1 "detail"
envelope_case "detail with quotes and newlines stays one JSON string" block 1 0 0 \
	'gate/a: "x"'$'\n''  tools/x.sh: + matches /\[allow\(/'

# ── Stop-hook loop breaker ────────────────────────────────────────────────

STOP_TMP="$(mktemp -d)"
trap 'rm -rf "$STOP_TMP"' EXIT
STATE="$STOP_TMP/last-block"

# decide_case <name> <want> <want_state> <key> <scan_rc> <gates_rc> <detail>;
# want_state is "absent" or the key the state file must hold afterwards.
decide_case() {
	local name="$1" want="$2" want_state="$3" key="$4" detail="$7"
	shift 4
	local out state
	out="$(stop_decide "$key" "$STATE" "$@")"
	report "$name" "$want" "$(envelope_class "$detail" "$out")"
	state=absent
	[[ -f "$STATE" ]] && state="$(cat "$STATE")"
	report "$name: state" "$want_state" "$state"
}

decide_case "first block writes the key" block k1 k1 1 0 "detail"
decide_case "same key failing again ends the turn" message absent k1 1 0 "detail"
decide_case "a block after the state was cleared writes the key" block k1 k1 0 1 "detail"
decide_case "a different key failing blocks" block k2 k2 0 1 "detail"
decide_case "clean clears the state" silent absent k3 0 0 ""
printf '%s' k4 >"$STATE"
decide_case "unauthorised gate edits alone clear the state" message absent k4 3 0 "detail"
report "stop_key returns a non-empty cache key" cachekey "$(stop_key cachekey "scan" a b)"
EXPECTED="$(printf '%s\n%s' "scan" "a b" | sha256sum | cut -c1-64)"
report "stop_key hashes the failure text without a cache key" "$EXPECTED" "$(stop_key "" "scan" a b)"

# ── run_all_gates stops at the first failing gate ─────────────────────────

check_rules_rs_macros() { true; }
check_package_readmes() { true; }
QUIET_GATE_RC=1
check_bazel_quiet() { return "$QUIET_GATE_RC"; }
run_format_check() { LATER_GATE_RAN=1; }
run_bazel_coverage() { LATER_GATE_RAN=1; }
enforce_per_file_coverage() { LATER_GATE_RAN=1; }
TIMINGS=()
FAILED_GATES=()
LATER_GATE_RAN=""
rc=0
run_all_gates || rc=$?
report "a failing gate's rc is run_all_gates's rc" 1 "$rc"
report "only the failing gate is recorded" bazel_quiet "${FAILED_GATES[*]}"
report "gates up to the failure ran" 3 "${#TIMINGS[@]}"
report "no gate after the failure ran" "" "$LATER_GATE_RAN"

QUIET_GATE_RC=0
TIMINGS=()
FAILED_GATES=()
rc=0
run_all_gates || rc=$?
report "all gates passing returns 0" 0 "$rc"
report "all six gates ran" 6 "${#TIMINGS[@]}"
report "no failure recorded" "" "${FAILED_GATES[*]}"

# ── dispatch: agent mode is exactly the two hooks ─────────────────────────

dispatch_case() {
	local name="$1" want="$2"
	shift 2
	local rc=0
	bash "$CHECK_SH" "$@" >/dev/null 2>&1 || rc=$?
	report "$name" "$want" "$rc"
}
dispatch_case "agent without a hook is rejected" 2 agent
dispatch_case "agent with a target is rejected" 2 agent //...
dispatch_case "agent stop with a target is rejected" 2 agent stop //...

# ── summary ───────────────────────────────────────────────────────────────

echo ""
echo "$PASS passed, $FAIL failed"
if [[ "$FAIL" -gt 0 ]]; then
	exit 1
fi
