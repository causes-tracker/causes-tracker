# buck2 cannot share a checkout with Bazel

buck2's file watcher follows the `bazel-*` convenience symlinks into the multi-GB output tree at daemon startup and hangs (>90s timeout); `project.ignore` does not prevent it (buck2#465, #474).

**How to apply:** run buck2 from a bazel-free jj workspace (`jj workspace add --revision @ ~/causes-buck2`); author code and run quality gates in the main workspace.
CI runs buck2 in a separate job on a fresh checkout that never invokes Bazel (the `buck2` job in `.github/workflows/build.yml`).
