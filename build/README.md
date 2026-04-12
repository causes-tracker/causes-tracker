# build

Bazel build macros and project-wide defaults.
All Rust targets load from here instead of `@rules_rs` directly so that project-wide compiler policy is enforced in one place.
