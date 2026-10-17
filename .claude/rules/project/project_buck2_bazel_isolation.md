# buck2 cannot share a checkout with Bazel

buck2's file watcher follows `bazel-*` convenience symlinks (buck2#465, #474; `project.ignore` does not prevent it), with two observed failure modes:

- At daemon startup over a populated multi-GB Bazel output tree, the watch crawl hangs (>90s timeout).
- With the symlinks present but the crawl surviving, watches on the shared source dirs register under the execroot alias, so file-change events surface as `root//bazel-<dir>/...` and the real paths never invalidate — every build silently uses stale sources, and `buck2 debug file-status` still reports no mismatch.

**How to apply:** run buck2 from a bazel-free jj workspace (`jj workspace add --revision @ ~/causes-buck2`); author code and run quality gates in the main workspace.
CI runs buck2 in a separate job on a fresh checkout that never invokes Bazel (the `buck2` job in `.github/workflows/build.yml`).
`tools/buck2/buck2.sh` refuses to start while `bazel-*` symlinks exist; delete them and run `buck2 kill` (full mechanism in its comment).
