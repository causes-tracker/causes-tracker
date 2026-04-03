"Rust build macros with project-wide defaults (e.g. -Dwarnings)."

load("@rules_rs//rs:rust_binary.bzl", _rust_binary = "rust_binary")
load("@rules_rs//rs:rust_library.bzl", _rust_library = "rust_library")
load("@rules_rs//rs:rust_test.bzl", _rust_test = "rust_test")

_DEFAULT_RUSTC_FLAGS = ["-Dwarnings"]

def rust_binary(rustc_flags = [], **kwargs):
    _rust_binary(rustc_flags = _DEFAULT_RUSTC_FLAGS + rustc_flags, **kwargs)

def rust_library(rustc_flags = [], **kwargs):
    _rust_library(rustc_flags = _DEFAULT_RUSTC_FLAGS + rustc_flags, **kwargs)

def rust_test(rustc_flags = [], **kwargs):
    _rust_test(rustc_flags = _DEFAULT_RUSTC_FLAGS + rustc_flags, **kwargs)
