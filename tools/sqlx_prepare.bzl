"""sqlx_prepare macro — generates sqlx offline-metadata targets for a Rust crate."""

load("@rules_shell//shell:sh_binary.bzl", "sh_binary")
load("@rules_shell//shell:sh_test.bzl", "sh_test")

_IMPL = "//tools:sqlx_prepare_impl.sh"

_TOOL_DATA = [
    "//infra/postgres:postgres_extracted",
    "//infra/postgres:testfixture.sh",
    "//tools:sqlx_bin",
    "@bazel_tools//tools/bash/runfiles",
    "@rust_host_tools//:cargo",
    "@rust_host_tools//:rustc",
    "@rust_host_tools//:sysroot_path.txt",
]

def sqlx_prepare(
        name,
        migrations,
        srcs,
        test_migrations = None,
        sibling_deps = None,
        visibility = None):
    """Generates sqlx offline query-metadata targets for a Rust crate.

    Run from the calling package's directory so that sqlx writes .sqlx/ there.

    Targets produced:
      :{name}       — sh_binary: bazel run to regenerate .sqlx/ in the source tree
      :{name}_test  — sh_test:   fails if the committed .sqlx/ files are stale

    Args:
      name:            base name (conventionally "sqlx_prepare")
      migrations:      migration file labels — glob(["migrations/**"])
      srcs:            source + .sqlx labels for the check test —
                       glob(["src/**/*.rs"]) + glob([".sqlx/**"])
      test_migrations: optional test-only migration labels — glob(["migrations-test/**"]).
                       Applied after `migrations` so tests can use tables defined
                       only for tests.
      sibling_deps:    optional list of `path = "../foo"` sibling-crate file
                       labels that need to be present in the isolated workspace
                       (e.g. proc-macro crates this package depends on).
                       The check script auto-discovers their names from
                       Cargo.toml; these labels just make the source files
                       reachable from the test sandbox.
      visibility:      optional visibility for the sh_binary update target
    """
    pkg = native.package_name()  # e.g. "lib/rust/api_db"
    test_migrations = test_migrations or []
    sibling_deps = sibling_deps or []

    sh_binary(
        name = name,
        srcs = [_IMPL],
        args = [pkg],
        data = _TOOL_DATA + migrations + test_migrations,
        visibility = visibility,
    )

    sh_test(
        name = name + "_test",
        srcs = [_IMPL],
        args = ["--check", pkg],
        data = _TOOL_DATA + migrations + test_migrations + srcs + sibling_deps + [
            "//:Cargo.toml",
            "//:Cargo.lock",
            ":Cargo.toml",
        ],
    )
