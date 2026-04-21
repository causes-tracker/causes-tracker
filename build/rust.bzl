"""Rust build macros with project-wide defaults (e.g. -Dwarnings).

Each Rust target also gets a sibling <name>_rustfmt_check sh_test that caches
its inputs, so bazel test short-circuits on an unformatted-file-free tree
instead of re-running rustfmt from scratch every time.
"""

load("@rules_rs//rs:rust_binary.bzl", _rust_binary = "rust_binary")
load("@rules_rs//rs:rust_library.bzl", _rust_library = "rust_library")
load("@rules_rs//rs:rust_test.bzl", _rust_test = "rust_test")
load("//tools/lint:format_check.bzl", "rustfmt_check")
load("//tools/lint:linters.bzl", "clippy_lint_test")

_DEFAULT_RUSTC_FLAGS = ["-Dwarnings"]

def rust_binary(name, srcs = [], rustc_flags = [], **kwargs):
    _rust_binary(
        name = name,
        srcs = srcs,
        rustc_flags = _DEFAULT_RUSTC_FLAGS + rustc_flags,
        **kwargs
    )
    rustfmt_check(name = name + "_rustfmt_check", srcs = srcs)
    clippy_lint_test(name = name + "_clippy", srcs = [":" + name])

def rust_library(name, srcs = [], rustc_flags = [], **kwargs):
    _rust_library(
        name = name,
        srcs = srcs,
        rustc_flags = _DEFAULT_RUSTC_FLAGS + rustc_flags,
        **kwargs
    )
    rustfmt_check(name = name + "_rustfmt_check", srcs = srcs)
    clippy_lint_test(name = name + "_clippy", srcs = [":" + name])

def rust_test(name, srcs = [], rustc_flags = [], **kwargs):
    _rust_test(
        name = name,
        srcs = srcs,
        rustc_flags = _DEFAULT_RUSTC_FLAGS + rustc_flags,
        **kwargs
    )

    # When rust_test uses `crate = ":foo"` it inherits srcs from that crate and
    # `srcs` here is []; the library's own _rustfmt_check already covers them,
    # so rustfmt_check() is a no-op.
    rustfmt_check(name = name + "_rustfmt_check", srcs = srcs)
    clippy_lint_test(name = name + "_clippy", srcs = [":" + name])
