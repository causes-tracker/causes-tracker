# tools/lint

Lint rule definitions for the Bazel build.

Clippy is applied as an **aspect** (configured in `.bazelrc`), so it runs automatically on every Rust target without per-package opt-in.
Shellcheck runs under buck2 (`shellcheck_package()` in `tools/defs.bzl`), not Bazel.
Yamllint, pymarkdown, and buf are **test macros** — packages opt in by declaring a `*_lint_test` target.

`taplo` (TOML formatter) is distributed as a gzipped binary and extracted via genrule because there is no native Bazel rule for it.
