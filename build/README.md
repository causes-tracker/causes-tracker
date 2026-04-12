# build

Bazel build macros and project-wide defaults.

`rust.bzl` wraps `rules_rs` targets (`rust_binary`, `rust_library`, `rust_test`) to inject `-Dwarnings` globally.
All Rust targets in the repo load from `//build:rust.bzl` instead of `@rules_rs` directly, so compiler-warning policy is enforced in one place.
