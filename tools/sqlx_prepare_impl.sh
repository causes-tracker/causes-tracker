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

# Phase timing — emit `[phase] name: <seconds>s` to stderr around each major
# step. Always on, visible in the test log so we can attribute time without
# re-running.
phase() {
	local name="$1"
	shift
	local start end
	start="$EPOCHREALTIME"
	"$@"
	local rc=$?
	end="$EPOCHREALTIME"
	# 10# forces decimal: a leading-zero fraction would otherwise be octal.
	local s_us e_us d_us
	s_us=$((${start%.*} * 1000000 + 10#${start#*.}))
	e_us=$((${end%.*} * 1000000 + 10#${end#*.}))
	d_us=$((e_us - s_us))
	printf '[phase] %-20s %d.%06ds\n' "$name" "$((d_us / 1000000))" "$((d_us % 1000000))" >&2
	return "$rc"
}

# shellcheck source=/dev/null
source "$(rlocation _main/infra/postgres/testfixture.sh)"
phase pg_start pg_start

SQLX="$(rlocation _main/tools/sqlx_bin)"
export CARGO
CARGO="$(rlocation rust_host_tools/bin/cargo)"
export RUSTC
RUSTC="$(rlocation rust_host_tools/bin/rustc)"

# Pass the hermetic stdlib sysroot to rustc so that `cargo check` (invoked by
# `sqlx prepare`) can find core/std without a system Rust installation.
SYSROOT="$(cat "$(rlocation rust_host_tools/sysroot_path.txt)")"
export RUSTFLAGS="--sysroot ${SYSROOT}"

# Persistent cargo cache. Layout:
#   ${HOME}/.cache/causes/sqlx-prepare-target/<pkg-slug>/
#     .sqlx_prepare.lock   flock for one prepare run (cross-worktree barrier)
#     staging/             rewritten Cargo.toml + rsync'd pkg files
#     target/              CARGO_TARGET_DIR
#     cargo-home/          CARGO_HOME (registry cache + extracted dep sources)
# Wipe with `rm -rf ~/.cache/causes/sqlx-prepare-target/`.
#
# CARGO_HOME and the staging path live here (not under HOME or TEST_TMPDIR)
# because cargo's incremental fingerprint includes the workspace and
# registry source paths; bazel's per-sandbox HOME makes both move every
# run, which invalidates the entire dep graph.
setup_persistent_target() {
	# bazel rewrites HOME to a per-sandbox tmpdir; resolve the real user
	# home from the passwd database instead.
	local real_home
	real_home=$(getent passwd "$(id -un)" | cut -d: -f6)
	local cache_root="${real_home}/.cache/causes/sqlx-prepare-target"
	local pkg_slug="${BAZEL_PACKAGE//\//_}"

	WORKSPACE_ROOT="$(dirname "$(rlocation _main/Cargo.toml)")"
	PACKAGE_DIR="${WORKSPACE_ROOT}/${BAZEL_PACKAGE}"

	local cache_dir="${cache_root}/${pkg_slug}"
	mkdir -p "$cache_dir"

	exec 9>"$cache_dir/.sqlx_prepare.lock"
	flock 9

	export CARGO_TARGET_DIR="$cache_dir/target"
	export CARGO_HOME="$cache_dir/cargo-home"
	ISOLATED="$cache_dir/staging"
	mkdir -p "$ISOLATED/pkg" "$CARGO_TARGET_DIR" "$CARGO_HOME"
}

stage_isolated() {
	# mv-if-different so the manifest's mtime only moves on real change
	# (cargo invalidates the crate when its manifest mtime moves).
	python3 - "${WORKSPACE_ROOT}/Cargo.toml" "$ISOLATED/Cargo.toml.new" <<'PYEOF'
import sys, re
src, dst = sys.argv[1], sys.argv[2]
text = open(src).read()
text = re.sub(r'members\s*=\s*\[[^\]]*\]', 'members = ["pkg"]', text, flags=re.DOTALL)
open(dst, 'w').write(text)
PYEOF
	if ! cmp -s "$ISOLATED/Cargo.toml.new" "$ISOLATED/Cargo.toml" 2>/dev/null; then
		mv "$ISOLATED/Cargo.toml.new" "$ISOLATED/Cargo.toml"
	else
		rm -f "$ISOLATED/Cargo.toml.new"
	fi

	# Stage individual files only when content differs, so unchanged files
	# keep their staging mtime and cargo's mtime-based fingerprint stays
	# valid. Plain `cp` would touch every file and defeat the cache.
	# Cargo.lock gets re-staged each run too — cargo's prune step is
	# deterministic, so re-applying the source lockfile resolves to the
	# same pruned content and doesn't invalidate the cache.
	sync_file "${WORKSPACE_ROOT}/Cargo.lock" "$ISOLATED/Cargo.lock"
	sync_file "${PACKAGE_DIR}/Cargo.toml" "$ISOLATED/pkg/Cargo.toml"
	sync_dir "${PACKAGE_DIR}/src" "$ISOLATED/pkg/src"
	sync_dir "${PACKAGE_DIR}/migrations" "$ISOLATED/pkg/migrations"
	sync_dir "${PACKAGE_DIR}/.sqlx" "$ISOLATED/pkg/.sqlx"

	chmod -R u+w "$ISOLATED"
}

# sync_file SRC DST: copy SRC over DST iff content differs. Source symlinks
# are dereferenced. Skipping the cp leaves DST's mtime untouched.
sync_file() {
	local src="$1" dst="$2"
	if ! cmp -s "$src" "$dst" 2>/dev/null; then
		mkdir -p "$(dirname "$dst")"
		cp -L "$src" "$dst"
	fi
}

# sync_dir SRC DST: mirror SRC tree into DST. Files unchanged in content
# keep their existing mtime; changed files get the current time; files
# missing from SRC are deleted from DST. Symlinks are followed.
sync_dir() {
	local src="$1" dst="$2"
	mkdir -p "$dst"
	local rel
	while IFS= read -r -d '' rel; do
		if [[ -d "$src/$rel" ]]; then
			mkdir -p "$dst/$rel"
		else
			sync_file "$src/$rel" "$dst/$rel"
		fi
	done < <(cd "$src" && find -L . -mindepth 1 -print0)
	while IFS= read -r -d '' rel; do
		[[ -e "$src/$rel" ]] || rm -rf "${dst:?}/$rel"
	done < <(cd "$dst" && find . -mindepth 1 -print0)
}

run_migrate() {
	DATABASE_URL="$TEST_POSTGRES_URL" "$SQLX" migrate run \
		--source "$1"
}

run_prepare_check() {
	cd "$ISOLATED/pkg"
	DATABASE_URL="$TEST_POSTGRES_URL" "$SQLX" prepare --check -- --tests
}

run_prepare_update() {
	cd "$PACKAGE_DIR"
	DATABASE_URL="$TEST_POSTGRES_URL" "$SQLX" prepare -- --tests
}

if [[ "$CHECK" == "true" ]]; then
	phase setup_target setup_persistent_target
	phase stage_isolated stage_isolated
	phase migrate run_migrate "$ISOLATED/pkg/migrations"
	phase prepare_check run_prepare_check
else
	PACKAGE_DIR="${BUILD_WORKSPACE_DIRECTORY}/${BAZEL_PACKAGE}"
	phase migrate run_migrate "${PACKAGE_DIR}/migrations"
	phase prepare_update run_prepare_update
fi
