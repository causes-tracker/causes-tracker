#!/usr/bin/env bash
# sqlx_prepare_impl.sh — generate or check sqlx offline query metadata.
#
# Args: [--check] <cargo_package> <bazel_package>
#
# update mode (bazel run):  starts hermetic postgres, regenerates .sqlx/ in source tree
# check  mode (bazel test): starts hermetic postgres, fails if .sqlx/ is stale
set -euo pipefail

CHECK=false
if [[ "${1:-}" == "--check" ]]; then
	CHECK=true
	shift
fi
BAZEL_PACKAGE="${1:?bazel package path required}"

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

# TEST_TMPDIR is set by `bazel test` but not by `bazel run`.
# Provide a temporary directory for update mode.
export TEST_TMPDIR="${TEST_TMPDIR:-$(mktemp -d)}"

# shellcheck source=/dev/null
source "$(rlocation _main/infra/postgres/testfixture.sh)"
pg_start

SQLX="$(rlocation _main/tools/sqlx_bin)"
export CARGO
CARGO="$(rlocation rust_host_tools/bin/cargo)"
export RUSTC
RUSTC="$(rlocation rust_host_tools/bin/rustc)"

# Pass the hermetic stdlib sysroot to rustc so that `cargo check` (invoked by
# `sqlx prepare`) can find core/std without a system Rust installation.
SYSROOT="$(cat "$(rlocation rust_host_tools/sysroot_path.txt)")"
export RUSTFLAGS="--sysroot ${SYSROOT}"

if [[ "$CHECK" == "true" ]]; then
	# BUILD_WORKSPACE_DIRECTORY is not set in bazel test.
	# The runfiles tree contains the committed package files but not the full
	# workspace (other workspace members are absent, so cargo metadata fails).
	# Build an isolated single-member workspace in TEST_TMPDIR instead.
	WORKSPACE_ROOT="$(dirname "$(rlocation _main/Cargo.toml)")"
	PACKAGE_DIR="${WORKSPACE_ROOT}/${BAZEL_PACKAGE}"

	ISOLATED="$TEST_TMPDIR/isolated"
	mkdir -p "$ISOLATED/pkg"

	# Discover any path = "../foo" deps in the package's Cargo.toml so we can
	# copy those sibling crates into the isolated workspace alongside `pkg`.
	# Without this, cargo metadata fails on the path lookup.
	mapfile -t SIBLING_DEPS < <(
		python3 - "${PACKAGE_DIR}/Cargo.toml" <<'PYEOF'
import re, sys
text = open(sys.argv[1]).read()
# Match path = "../<name>" — pulls just the trailing dir name.
for m in re.finditer(r'path\s*=\s*"\.\./([A-Za-z0-9_-]+)"', text):
    print(m.group(1))
PYEOF
	)

	# Rewrite the workspace Cargo.toml so its members are pkg + each sibling
	# discovered above.  This lets cargo metadata resolve without the other
	# workspace members.
	WORKSPACE_MEMBERS='"pkg"'
	for s in "${SIBLING_DEPS[@]}"; do
		WORKSPACE_MEMBERS+=", \"${s}\""
	done
	WORKSPACE_MEMBERS_LITERAL="$WORKSPACE_MEMBERS" python3 - \
		"${WORKSPACE_ROOT}/Cargo.toml" "$ISOLATED/Cargo.toml" <<'PYEOF'
import os, re, sys
src, dst = sys.argv[1], sys.argv[2]
members = os.environ['WORKSPACE_MEMBERS_LITERAL']
text = open(src).read()
text = re.sub(r'members\s*=\s*\[[^\]]*\]', f'members = [{members}]', text, flags=re.DOTALL)
open(dst, 'w').write(text)
PYEOF

	# Bazel runfiles are symlinks into a read-only execroot.  We must
	# dereference them (-L) so that sqlx can touch source files.  If the
	# touch fails, sqlx falls back to `cargo clean`, which nukes the
	# target/sqlx-prepare-check/ dir that sqlx itself just created —
	# and then cargo check fails because the offline data is gone.
	cp -rL "${WORKSPACE_ROOT}/Cargo.lock" "$ISOLATED/Cargo.lock"
	cp -rL "${PACKAGE_DIR}/Cargo.toml" "$ISOLATED/pkg/Cargo.toml"
	cp -rL "${PACKAGE_DIR}/src" "$ISOLATED/pkg/src"
	cp -rL "${PACKAGE_DIR}/migrations" "$ISOLATED/pkg/migrations"
	cp -rL "${PACKAGE_DIR}/.sqlx" "$ISOLATED/pkg/.sqlx"

	# Copy each sibling crate that the package depends on via path = "../foo".
	# These live one directory up from PACKAGE_DIR (sibling of the package).
	for s in "${SIBLING_DEPS[@]}"; do
		cp -rL "$(dirname "${PACKAGE_DIR}")/${s}" "$ISOLATED/${s}"
	done

	chmod -R u+w "$ISOLATED"

	DATABASE_URL="$TEST_POSTGRES_URL" "$SQLX" migrate run \
		--source "$ISOLATED/pkg/migrations"
	cd "$ISOLATED/pkg"
	DATABASE_URL="$TEST_POSTGRES_URL" "$SQLX" prepare --check -- --tests
else
	PACKAGE_DIR="${BUILD_WORKSPACE_DIRECTORY}/${BAZEL_PACKAGE}"

	DATABASE_URL="$TEST_POSTGRES_URL" "$SQLX" migrate run \
		--source "${PACKAGE_DIR}/migrations"
	cd "$PACKAGE_DIR"
	DATABASE_URL="$TEST_POSTGRES_URL" "$SQLX" prepare -- --tests
fi
