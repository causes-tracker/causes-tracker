#!/usr/bin/env bash
# jj merge driver for MODULE.bazel.lock.
# Ignores all three sides and regenerates the lockfile from the current
# workspace state.  jj passes the output path as $1.
# Configure in .jj/repo/config.toml — see CLAUDE.md.
set -euo pipefail
output="$1"
bazel mod tidy >&2
cp MODULE.bazel.lock "$output"
