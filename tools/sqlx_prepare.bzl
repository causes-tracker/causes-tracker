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

def sqlx_prepare(name, migrations, srcs, path_deps = None, visibility = None):
    """Generates sqlx offline query-metadata targets for a Rust crate.

    Run from the calling package's directory so that sqlx writes .sqlx/ there.

    Targets produced:
      :{name}       — sh_binary: bazel run to regenerate .sqlx/ in the source tree
      :{name}_test  — sh_test:   fails if the committed .sqlx/ files are stale

    Caches roughly 1 GiB per crate under ~/.cache/causes/sqlx-prepare-target/.
    See tools/sqlx_prepare_impl.sh.

    Args:
      name:       base name (conventionally "sqlx_prepare")
      migrations: migration file labels — glob(["migrations/**"])
      srcs:       source + .sqlx labels for the check test —
                  glob(["src/**/*.rs"]) + glob([".sqlx/**"])
      path_deps:  bazel labels of in-workspace crates this crate depends on
                  via path. Each must expose an `:all_srcs` filegroup. The
                  impl script stages them next to the prepared crate so
                  cargo can resolve the path-deps inside the isolated
                  workspace.
      visibility: optional visibility for the sh_binary update target
    """
    pkg = native.package_name()
    path_deps = path_deps or []

    extra_data = []
    extra_args = []
    for dep in path_deps:
        # "//lib/rust/db_pool" → bazel pkg "lib/rust/db_pool", crate name "db_pool"
        bazel_pkg = dep.lstrip("/").partition(":")[0]
        crate_name = bazel_pkg.rpartition("/")[2]
        extra_data.append(dep + ":all_srcs")
        extra_args.append("--path-dep=" + crate_name + "=" + bazel_pkg)

    sh_binary(
        name = name,
        srcs = [_IMPL],
        args = [pkg] + extra_args,
        data = _TOOL_DATA + migrations + extra_data,
        visibility = visibility,
    )

    sh_test(
        name = name + "_test",
        srcs = [_IMPL],
        args = ["--check", pkg] + extra_args,
        data = _TOOL_DATA + migrations + srcs + extra_data + [
            "//:Cargo.toml",
            "//:Cargo.lock",
            ":Cargo.toml",
        ],
        # This test keeps a persistent cargo cache in the real home directory
        # (outside the tree) so cargo's path-embedded fingerprints survive
        # across runs; it is inherently non-hermetic and cannot run under
        # mount isolation.
        tags = ["no-sandbox"],
    )
