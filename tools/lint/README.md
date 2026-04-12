# tools/lint

Lint rule definitions for the Bazel build.

Clippy and Shellcheck are applied as **aspects** (configured in `.bazelrc`), so they run automatically on every Rust/Shell target without per-package opt-in.
Yamllint, pymarkdown, and buf are **test macros** — packages opt in by declaring a `*_lint_test` target.

`taplo` (TOML formatter) is distributed as a gzipped binary and extracted via genrule because there is no native Bazel rule for it.
