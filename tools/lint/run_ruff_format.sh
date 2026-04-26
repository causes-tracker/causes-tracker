#!/usr/bin/env bash
# format_multirun driver: invoked by aspect_rules_lint as
#   $0 format [--check] [--force-exclude] [files...]
#
# `ruff format` doesn't sort imports — that's the I rule on `ruff check`.
# To make a single `bazel run //:format` cover both whitespace AND import
# sorting, we run `ruff format` exactly as aspect_rules_lint asked, then
# do a second pass with `ruff check --select I` (with --fix in fix mode,
# without in --check mode).
set -euo pipefail

# shellcheck source=/dev/null
if [[ -f "${RUNFILES_DIR:-/dev/null}/rules_shell/shell/runfiles/runfiles.bash" ]]; then
	source "${RUNFILES_DIR}/rules_shell/shell/runfiles/runfiles.bash"
elif [[ -f "${RUNFILES_DIR:-/dev/null}/bazel_tools/tools/bash/runfiles/runfiles.bash" ]]; then
	source "${RUNFILES_DIR}/bazel_tools/tools/bash/runfiles/runfiles.bash"
else
	echo >&2 "ERROR: cannot find runfiles.bash"
	exit 1
fi

RUFF="$(rlocation rules_multitool++multitool+multitool/tools/ruff/ruff)"

[[ $# -gt 0 ]] || exit 0

# Pass 1: ruff format (verbatim args).
"$RUFF" "$@"

# Pass 2: ruff check --select I to enforce import sort.
# Drop the leading `format` subcommand, then translate flags:
#   --check  →  no --fix (check mode, exit non-zero on issues)
#   absent   →  --fix    (apply fixes in place)
# Other format flags (e.g. --force-exclude) are forwarded.
shift
fix_flag="--fix"
filtered=()
for arg in "$@"; do
	if [[ "$arg" == "--check" ]]; then
		fix_flag=""
	else
		filtered+=("$arg")
	fi
done

exec "$RUFF" check --select I ${fix_flag:+$fix_flag} "${filtered[@]}"
