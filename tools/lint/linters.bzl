"Linter aspects for the Causes monorepo."

# Each linter is defined once here and referenced from BUILD files via:
#
#   load("//tools/lint:linters.bzl", "buf_lint_test", "clippy_lint_test", "shellcheck_lint_test", ...)
#
# To add a lint test in a package:
#   clippy_lint_test(name = "clippy", srcs = [":some_rust_binary"])
#   shellcheck_lint_test(name = "shellcheck", srcs = [":some_sh_binary"])
#   yamllint_lint_test(name = "yamllint", srcs = [":some_yaml_filegroup"])
#   markdown_lint_test(name = "pymarkdown", srcs = ["README.md"])

load("@aspect_rules_lint//lint:buf.bzl", "lint_buf_aspect")
load("@aspect_rules_lint//lint:clippy.bzl", "lint_clippy_aspect")
load("@aspect_rules_lint//lint:lint_test.bzl", "lint_test")
load("@aspect_rules_lint//lint:shellcheck.bzl", "lint_shellcheck_aspect")
load("@aspect_rules_lint//lint:yamllint.bzl", "lint_yamllint_aspect")
load("@rules_shell//shell:sh_test.bzl", "sh_test")

# ── buf (Protocol Buffers) ────────────────────────────────────────────────────

buf = lint_buf_aspect(
    config = Label("//proto:buf.yaml"),
)

buf_lint_test = lint_test(aspect = buf)

# ── shellcheck (Shell scripts) ────────────────────────────────────────────────
# Applies to sh_binary, sh_library, sh_test targets.
# Binary: hermetic shellcheck provided by aspect_rules_lint via multitool.
# Config: //.shellcheckrc

shellcheck = lint_shellcheck_aspect(
    binary = Label("@aspect_rules_lint//lint:shellcheck_bin"),
    config = Label("//:.shellcheckrc"),
)

shellcheck_lint_test = lint_test(aspect = shellcheck)

# ── yamllint (YAML files) ─────────────────────────────────────────────────────
# Applies to filegroup targets tagged "lint-with-yamllint".
# Binary: hermetic yamllint installed via pip/rules_python.
# Config: //.yamllint

yamllint = lint_yamllint_aspect(
    binary = Label("//tools/lint:yamllint"),
    config = Label("//:.yamllint"),
)

yamllint_lint_test = lint_test(aspect = yamllint)

# ── clippy (Rust linter) ──────────────────────────────────────────────────────
# Applies to rust_binary, rust_library, rust_test targets.
# Binary: clippy driver from the registered Rust toolchain (via rules_rust).
# Config: //.clippy.toml

clippy = lint_clippy_aspect(
    config = Label("//:.clippy.toml"),
    clippy_flags = ["-Dwarnings"],
)

clippy_lint_test = lint_test(aspect = clippy)

# ── pymarkdown (Markdown linter) ──────────────────────────────────────────────
# Unlike the aspect-based linters above, markdown files have no native Bazel
# rule type, so this is a plain sh_test macro.
# Binary: hermetic pymarkdown installed via pip/rules_python.
# Config: //:.pymarkdown.json

def markdown_lint_test(name, srcs, **kwargs):
    """Run pymarkdown on a list of Markdown source files."""
    sh_test(
        name = name,
        srcs = ["//tools/lint:run_pymarkdown.sh"],
        args = ["$(location %s)" % s for s in srcs],
        data = srcs + [
            "//tools/lint:pymarkdown",
            "//:.pymarkdown.json",
            "@bazel_tools//tools/bash/runfiles",
            "@rules_shell//shell/runfiles:runfiles",
        ],
        **kwargs
    )
