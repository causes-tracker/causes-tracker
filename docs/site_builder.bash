#!/usr/bin/env bash
# Shared site-build logic sourced by docs_test.sh and deploy_docs.sh.
# Source this file after the Bazel runfiles init; then call build_docs_site.
#
# Required env vars (set by caller before calling build_docs_site):
#   ZENSICAL        — path to the zensical binary (from rlocation)
#   MKDOCS_YML      — path to the source mkdocs.yml (from rlocation or workspace)
#   DESIGNDOCS_SRC  — directory containing the .md source files; the caller is
#                     responsible for staging any generated files (e.g. Proto-Reference.md)
#                     into this directory before calling build_docs_site.
#
# Output env var (set by build_docs_site on success):
#   SITE_DIR        — absolute path to the built site directory

build_docs_site() {
  local build_root
  build_root=$(mktemp -d)

  # Dereference symlinks: Zensical silently finds 0 pages through double-symlinks.
  mkdir "$build_root/designdocs"
  cp -rL "$DESIGNDOCS_SRC/." "$build_root/designdocs/"

  # Inject generated docs that are not in the source tree.
  # Callers set PROTO_DOCS to the path of the generated proto reference Markdown.
  if [[ -n "${PROTO_DOCS:-}" ]]; then
    cp "$PROTO_DOCS" "$build_root/designdocs/Proto-Reference.md"
  fi

  # Zensical rejects docs_dir values that contain '..'.
  # Strip docs_dir and site_dir from the source config and substitute a
  # local-relative path.
  sed -e '/^docs_dir:/d' -e '/^site_dir:/d' "$MKDOCS_YML" > "$build_root/mkdocs.yml"
  echo "docs_dir: designdocs" >> "$build_root/mkdocs.yml"

  # Build in a subshell to avoid changing the caller's working directory.
  (cd "$build_root" && "$ZENSICAL" build)

  # Zensical locates its theme assets via __file__ at runtime, but the Bazel
  # runfiles layout differs from a regular pip install, so it silently skips
  # the copy.  Copy them manually.
  #
  # Derive the runfiles root from ZENSICAL, which rlocation resolved to
  # {root}/_main/docs/zensical — three dirname calls reach the root.
  # This works for both bazel test (where RUNFILES_DIR is also set) and
  # bazel run (where RUNFILES_DIR may be absent).
  local runfiles_root assets
  runfiles_root="$(dirname "$(dirname "$(dirname "$ZENSICAL")")")"
  assets=$(find "$runfiles_root" -path "*/zensical/templates/assets" -type d 2>/dev/null | head -1)
  if [[ -z "$assets" ]]; then
    echo >&2 "ERROR: zensical theme assets not found under runfiles root: $runfiles_root"
    return 1
  fi
  cp -rL "$assets" "$build_root/site/assets"

  SITE_DIR="$build_root/site"
}
