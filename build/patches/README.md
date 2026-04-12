# build/patches

Patches applied to `rules_rs` via `MODULE.bazel` `archive_override`.

These fix issues in the upstream `rules_rs` Bazel module that haven't been released yet — primarily around toolchain declaration, LLVM tools repository setup, and module extension behaviour.
Review and remove each patch when upgrading `rules_rs` to a version that includes the fix.
