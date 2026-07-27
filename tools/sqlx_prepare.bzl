"""Golden targets for sqlx offline query metadata.

`sqlx_prepare` compiles a `rust_library` online (`SQLX_OFFLINE=false`)
against a hermetic PostgreSQL started inside the build action, so every
`sqlx::query!` macro introspects the live schema and writes its
`query-<sha>.json` into a declared TreeArtifact.
The `_test` target diffs that TreeArtifact against the crate's committed
`.sqlx/`; the runnable target copies it back into the source tree.
"""

load("@rules_rust//rust:defs.bzl", "rust_common")
load("@rules_shell//shell:sh_binary.bzl", "sh_binary")
load("@rules_shell//shell:sh_test.bzl", "sh_test")

def _shquote(s):
    return "'" + s.replace("'", "'\\''") + "'"

def _externs_and_closure(crate_info):
    """Direct `--extern` flags plus the transitive rlib/proc-macro closure.

    Returns (externs, dep_files, dep_dirs): `externs` are the direct
    `--extern name=path` pairs; `dep_files` every File to stage as an input;
    `dep_dirs` the unique dirs for `-Ldependency`.
    """
    externs = []
    dep_files = []
    dep_dirs = {}

    direct = crate_info.deps.to_list() + crate_info.proc_macro_deps.to_list()
    for dvi in direct:
        ci = dvi.crate_info
        if ci == None:
            continue
        externs.append("--extern")
        externs.append("{}={}".format(ci.name, ci.output.path))
        dep_files.append(ci.output)
        di = dvi.dep_info
        for f in di.transitive_crate_outputs.to_list():
            dep_files.append(f)
            dep_dirs[f.dirname] = True
        for f in di.transitive_proc_macro_data.to_list():
            dep_files.append(f)
            dep_dirs[f.dirname] = True

    return externs, dep_files, dep_dirs

def _sqlx_capture_impl(ctx):
    toolchain = ctx.toolchains["@rules_rust//rust:toolchain_type"]
    crate_info = ctx.attr.crate[rust_common.crate_info]

    externs, dep_files, dep_dirs = _externs_and_closure(crate_info)
    ldeps = ["-Ldependency={}".format(d) for d in dep_dirs.keys()]

    srcs = crate_info.srcs.to_list()
    root = crate_info.root
    pg_root = ctx.file.pg_extracted
    migrations = ctx.files.migrations
    if not migrations:
        fail("migrations attr must resolve to at least one file")

    # Migration files live at <pkg>/migrations/<n>.sql; sqlx wants the dir.
    migrations_dir = migrations[0].dirname

    out_dir = ctx.actions.declare_directory(ctx.label.name + ".sqlx")

    common_argv = [
        toolchain.rustc.path,
        "--edition",
        crate_info.edition,
        "--crate-name",
        crate_info.name,
        "--emit=metadata",
        "--sysroot",
        toolchain.sysroot,
    ] + externs + ldeps
    lib_cmd = " ".join([_shquote(a) for a in common_argv + ["--crate-type", "lib", root.path]])
    test_cmd = " ".join([_shquote(a) for a in common_argv + ["--test", root.path]])

    # Two compiles share one SQLX_OFFLINE_DIR: the library build captures
    # production queries, the `--test` harness build captures queries in
    # `#[cfg(test)]` code, matching `sqlx prepare -- --tests`.
    # Just-in-time postgres comes from the shared fixture; PGBIN and
    # TEST_TMPDIR are pre-set because build actions have no runfiles.
    script = r"""
set -euo pipefail
export PGBIN="{pg}/bin"
export TEST_TMPDIR="$(mktemp -d)"
source {fixture}
pg_start
export DATABASE_URL="$TEST_POSTGRES_URL"

"{sqlx_bin}" migrate run --source "{migrations_dir}"

mkdir -p "{out}"
compile() {{
  SQLX_OFFLINE=false \
  SQLX_OFFLINE_DIR="{out}" \
  CARGO_MANIFEST_DIR="$PWD/{manifest}" \
    "$@" --out-dir "$TEST_TMPDIR/meta"
}}
compile {lib_cmd}
compile {test_cmd}
""".format(
        pg = pg_root.path,
        fixture = ctx.file.pg_fixture.path,
        sqlx_bin = ctx.executable.sqlx_bin.path,
        migrations_dir = migrations_dir,
        out = out_dir.path,
        manifest = ctx.label.package,
        lib_cmd = lib_cmd,
        test_cmd = test_cmd,
    )

    inputs = depset(
        direct = [pg_root, ctx.file.pg_fixture, root] + srcs + migrations + dep_files,
        transitive = [toolchain.all_files],
    )

    ctx.actions.run_shell(
        outputs = [out_dir],
        inputs = inputs,
        tools = [ctx.executable.sqlx_bin],
        command = script,
        mnemonic = "SqlxCapture",
        progress_message = "sqlx online capture %{label}",
        use_default_shell_env = True,
    )

    return [DefaultInfo(files = depset([out_dir]))]

_sqlx_capture = rule(
    implementation = _sqlx_capture_impl,
    doc = "Captures sqlx offline metadata for `crate` into a `.sqlx` TreeArtifact.",
    attrs = {
        "crate": attr.label(
            mandatory = True,
            providers = [rust_common.crate_info],
            doc = "The `rust_library` whose `sqlx::query!` sites to capture. " +
                  "The rebuilt rustc invocation carries only srcs, edition, and " +
                  "deps; a crate using crate_features, dep aliases, or rustc_env " +
                  "would diverge from its real compile.",
        ),
        "migrations": attr.label_list(
            mandatory = True,
            allow_files = True,
            doc = "Migration `.sql` files applied before introspection.",
        ),
        "pg_extracted": attr.label(
            mandatory = True,
            allow_single_file = True,
            doc = "The extracted PostgreSQL distribution TreeArtifact.",
        ),
        "pg_fixture": attr.label(
            mandatory = True,
            allow_single_file = True,
            doc = "The shared postgres fixture script providing pg_start.",
        ),
        "sqlx_bin": attr.label(
            mandatory = True,
            executable = True,
            cfg = "exec",
            doc = "The hermetic sqlx-cli binary used to run migrations.",
        ),
    },
    toolchains = ["@rules_rust//rust:toolchain_type"],
)

def sqlx_prepare(name, crate, migrations):
    """Golden targets for `crate`'s committed `.sqlx/` files.

    Produces:
      :{name}          — `bazel run` copies a fresh capture into the source
                         tree's `.sqlx/`.
      :{name}_test     — fails on any add/remove/content drift between the
                         capture and the committed files.
      :{name}_capture  — the captured `.sqlx` TreeArtifact.
    """
    capture = name + "_capture"
    _sqlx_capture(
        name = capture,
        crate = crate,
        migrations = migrations,
        pg_extracted = "//infra/postgres:postgres_extracted",
        pg_fixture = "//infra/postgres:testfixture.sh",
        sqlx_bin = "//tools/sqlx-cli:sqlx_bin",
    )
    sh_test(
        name = name + "_test",
        size = "small",
        srcs = ["//tools:sqlx_prepare_check.sh"],
        args = [
            "$(rlocationpath :{})".format(capture),
            native.package_name(),
            name,
        ],
        data = [":" + capture] + native.glob(
            [".sqlx/**"],
            allow_empty = True,
        ) + [
            "@bazel_tools//tools/bash/runfiles",
        ],
    )
    sh_binary(
        name = name,
        srcs = ["//tools:sqlx_prepare_update.sh"],
        args = [
            "$(rootpath :{})".format(capture),
            native.package_name(),
        ],
        data = [":" + capture],
    )
