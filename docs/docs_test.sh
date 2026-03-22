#!/usr/bin/env bash
# Smoke-tests the docs build by running zensical build in a temp directory.
# Run with: bazel test //docs:docs_test
set -euo pipefail

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

ZENSICAL=$(rlocation _main/docs/zensical)
MKDOCS_YML=$(rlocation _main/docs/mkdocs.yml)
ZENSICAL_ASSETS=$(find "$RUNFILES_DIR" -path "*/zensical/templates/assets" -type d | head -1)

# Resolve the runfiles designdocs directory. Runfiles use a double-symlink chain
# (runfiles → execroot → source); Zensical silently finds 0 pages through
# double-symlinks, so copy files with -L to dereference all symlinks.
DESIGNDOCS_RUNFILES=$(rlocation _main/designdocs/README.md)
DESIGNDOCS_DIR=$(dirname "$DESIGNDOCS_RUNFILES")

# Zensical requires the config to sit at the project root with relative paths.
# Set up a minimal project tree in TEST_TMPDIR with real (dereferenced) doc files.
mkdir "$TEST_TMPDIR/designdocs"
cp -rL "$DESIGNDOCS_DIR/." "$TEST_TMPDIR/designdocs/"
sed -e '/^docs_dir:/d' -e '/^site_dir:/d' "$MKDOCS_YML" > "$TEST_TMPDIR/mkdocs.yml"
echo "docs_dir: designdocs" >> "$TEST_TMPDIR/mkdocs.yml"

cd "$TEST_TMPDIR"
"$ZENSICAL" build
cp -rL "$ZENSICAL_ASSETS" "$TEST_TMPDIR/site/assets"

SITE="$TEST_TMPDIR/site"
FAIL=0

# Every page in the nav must produce an index.html with non-trivial content.
# Keys are site-relative paths; values are strings that must appear in the page.
declare -A EXPECTED_PAGES=(
  ["index.html"]="Causes"
  ["Manifesto/index.html"]="Manifesto"
  ["Design-Choices/index.html"]="Design"
  ["Decisions/index.html"]="Decision"
  ["Schema/index.html"]="Schema"
  ["Contributing/index.html"]="Contributing"
  ["Raw-Discussion/index.html"]="Discussion"
)

# Assets must be present so the site renders correctly in a browser.
if ! find "$SITE/assets/javascripts" -name "bundle*.min.js" -type f | grep -q .; then
  echo "ERROR: theme JS bundle missing from site/assets/javascripts/" >&2
  FAIL=1
fi
if ! find "$SITE/assets/stylesheets" -name "*.min.css" -type f | grep -q .; then
  echo "ERROR: theme CSS missing from site/assets/stylesheets/" >&2
  FAIL=1
fi

for page in "${!EXPECTED_PAGES[@]}"; do
  needle="${EXPECTED_PAGES[$page]}"
  path="$SITE/$page"
  if [[ ! -f "$path" ]]; then
    echo "ERROR: expected page missing: $page" >&2
    FAIL=1
  elif ! grep -qi "$needle" "$path"; then
    echo "ERROR: $page exists but does not contain '$needle'" >&2
    FAIL=1
  fi
done

[[ "$FAIL" -eq 0 ]] || exit 1
echo "OK: all ${#EXPECTED_PAGES[@]} pages built with expected content"
