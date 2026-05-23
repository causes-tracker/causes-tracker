#!/usr/bin/env bash
# Test driver for the `scan_diff_for_suppressions` function in tools/check.sh.
# Sources check.sh (with _CHECK_SH_SOURCED=1 to skip the dispatch flow) and
# feeds the scanner synthetic git-format diffs via stdin, asserting the
# expected exit code (0 = clean, 1 = suppression detected).
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

# shellcheck source=/dev/null
_CHECK_SH_SOURCED=1 source "$CHECK_SH"

PASS=0
FAIL=0

case_pass() { case_run "$1" 0 "$2"; }
case_fail() { case_run "$1" 1 "$2"; }
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

case_fail ".bazelignore created" 'diff --git a/.bazelignore b/.bazelignore
new file mode 100644
--- /dev/null
+++ b/.bazelignore
@@ -0,0 +1 @@
+some/skipped/path
'

case_fail "tools/check.sh edited" 'diff --git a/tools/check.sh b/tools/check.sh
--- a/tools/check.sh
+++ b/tools/check.sh
@@ -1 +1 @@
-MIN_PCT=25
+MIN_PCT=10
'

case_fail ".clippy.toml edited" 'diff --git a/.clippy.toml b/.clippy.toml
--- a/.clippy.toml
+++ b/.clippy.toml
@@ -1 +1 @@
-doc-valid-idents = ["X"]
+doc-valid-idents = ["X","Y"]
'

case_fail "rustfmt.toml edited" 'diff --git a/rustfmt.toml b/rustfmt.toml
--- a/rustfmt.toml
+++ b/rustfmt.toml
@@ -1 +1 @@
-max_width = 100
+max_width = 200
'

case_fail ".shellcheckrc edited" 'diff --git a/.shellcheckrc b/.shellcheckrc
--- a/.shellcheckrc
+++ b/.shellcheckrc
@@ -1 +1,2 @@
 enable=all
+disable=SC2086
'

case_fail ".yamlfmt edited" 'diff --git a/.yamlfmt b/.yamlfmt
--- a/.yamlfmt
+++ b/.yamlfmt
@@ -1 +1 @@
-indent: 2
+indent: 4
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

# ── multi-violation: every violation listed, not just the first ───────────

case_fail "two suppressions on two added lines" 'diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1,3 @@
 fn x() {}
+#[allow(dead_code)]
+#[ignore]
'

# ── summary ───────────────────────────────────────────────────────────────

echo ""
echo "$PASS passed, $FAIL failed"
if [[ "$FAIL" -gt 0 ]]; then
	exit 1
fi
