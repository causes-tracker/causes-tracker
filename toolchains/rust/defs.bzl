# A Rust toolchain from the musl dists.
# rustc links the musl loader, so it runs on the from-scratch worker through
# the staged runtime like the other prebuilt musl binaries, under
# rustc_wrapper.sh, which lands its outputs on their declared paths.
# The prelude's rust rules build a clippy wrapper for every crate and fail
# analysis when clippy_driver is None.

load("@prelude//rust:cargo_buildscript.bzl", prelude_buildscript_run = "buildscript_run")
load("@prelude//rust:rust_toolchain.bzl", "PanicRuntime", "RustToolchainInfo")

# target: the rustc target triple the sysroot holds std for.
RustSysrootInfo = provider(fields = {
    "target": provider_field(str),
})

# The rustc component tree with the std component's lib/rustlib/<target>/lib
# laid over it, where rustc looks for core, alloc and std, and the cargo,
# clippy and llvm-tools binaries added to bin/, so one shim tree serves every
# tool.
def _rust_sysroot_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("sysroot", dir = True)
    rustlib = "lib/rustlib/{}".format(ctx.attrs.target)
    ctx.actions.run(
        cmd_args(
            "/bin/sh",
            "-c",
            'set -eu; cp -a "$1/." "$6/"; mkdir -p "$6/$7/lib"; cp -a "$2/$7/lib/." "$6/$7/lib/"; ' +
            'cp -a "$3/bin/." "$6/bin/"; cp -a "$4/$7/bin/." "$6/bin/"; cp -a "$5/bin/." "$6/bin/"',
            "sysroot",
            ctx.attrs.rustc[DefaultInfo].default_outputs[0],
            ctx.attrs.std[DefaultInfo].default_outputs[0],
            ctx.attrs.clippy[DefaultInfo].default_outputs[0],
            ctx.attrs.llvm_tools[DefaultInfo].default_outputs[0],
            ctx.attrs.cargo[DefaultInfo].default_outputs[0],
            out.as_output(),
            rustlib,
        ),
        category = "rust_sysroot",
        allow_cache_upload = True,
    )
    return [
        DefaultInfo(default_output = out),
        RustSysrootInfo(target = ctx.attrs.target),
    ]

rust_sysroot = rule(
    impl = _rust_sysroot_impl,
    attrs = {
        "cargo": attrs.dep(providers = [DefaultInfo], doc = "The unpacked cargo component."),
        "clippy": attrs.dep(providers = [DefaultInfo], doc = "The unpacked clippy component."),
        "llvm_tools": attrs.dep(providers = [DefaultInfo], doc = "The unpacked llvm-tools component."),
        "rustc": attrs.dep(providers = [DefaultInfo], doc = "The unpacked rustc component."),
        "std": attrs.dep(providers = [DefaultInfo], doc = "The unpacked rust-std component."),
        "target": attrs.string(doc = "rustc target triple, e.g. x86_64-unknown-linux-musl."),
    },
)

def _musl_rust_toolchain_impl(ctx: AnalysisContext) -> list[Provider]:
    tools = ctx.attrs.rust_tools[DefaultInfo].default_outputs
    bash_bin = ctx.attrs.bash_shims[DefaultInfo].default_outputs
    compiler = cmd_args(
        "/bin/sh",
        ctx.attrs.wrapper,
        cmd_args(bash_bin, format = "{}/bin"),
        cmd_args(tools, format = "{}/bin/rustc"),
        hidden = tools + bash_bin,
    )
    rustdoc = cmd_args(cmd_args(tools, format = "{}/bin/rustdoc"), hidden = tools)
    clippy_driver = cmd_args(cmd_args(tools, format = "{}/bin/clippy-driver"), hidden = tools)
    return [
        DefaultInfo(),
        RustToolchainInfo(
            clippy_driver = RunInfo(args = clippy_driver),
            compiler = RunInfo(args = compiler),
            default_edition = ctx.attrs.default_edition,
            nightly_features = False,
            panic_runtime = PanicRuntime("unwind"),
            # zig cc supplies crt, libc and unwind.
            rustc_flags = ["-Clink-self-contained=no"],
            rustc_target_triple = ctx.attrs.sysroot[RustSysrootInfo].target,
            rustdoc = RunInfo(args = rustdoc),
            sysroot_path = ctx.attrs.sysroot[DefaultInfo].default_outputs[0],
        ),
    ]

musl_rust_toolchain = rule(
    impl = _musl_rust_toolchain_impl,
    attrs = {
        "bash_shims": attrs.exec_dep(providers = [DefaultInfo], doc = "musl_shims tree whose bin/bash runs the runtime's bash."),
        "default_edition": attrs.string(),
        "rust_tools": attrs.exec_dep(providers = [DefaultInfo], doc = "musl_shims tree whose bin/ runs the sysroot's binaries."),
        "sysroot": attrs.dep(providers = [DefaultInfo, RustSysrootInfo]),
        "wrapper": attrs.default_only(attrs.source(default = "//rust:rustc_wrapper")),
    },
    is_toolchain_rule = True,
)

# A build script behind a launcher that makes the relative entries of PATH
# absolute.
# The prelude renders PATH relative to the directory the build script starts
# in, and build scripts run the compiler shims from other directories.
def _absolute_path_buildscript_impl(ctx: AnalysisContext) -> list[Provider]:
    launcher = ctx.actions.declare_output("launcher.sh")
    buildscript = ctx.attrs.buildscript[RunInfo]
    ctx.actions.write(
        launcher,
        [
            "#!/bin/sh",
            "set -eu",
            'absolute=""',
            "IFS=:",
            "for dir in $PATH; do",
            '    case "$dir" in /*) ;; *) dir="$PWD/$dir" ;; esac',
            '    absolute="${absolute:+$absolute:}$dir"',
            "done",
            "unset IFS",
            'export PATH="$absolute"',
            cmd_args(buildscript, relative_to = (launcher, 1), format = 'exec "$(dirname "$0")/{}" "$@"'),
        ],
        is_executable = True,
        allow_args = True,
    )
    return [
        DefaultInfo(default_output = launcher),
        RunInfo(args = cmd_args(launcher, hidden = buildscript)),
    ]

_absolute_path_buildscript = rule(
    impl = _absolute_path_buildscript_impl,
    attrs = {
        "buildscript": attrs.dep(providers = [RunInfo]),
    },
)

# The prelude's buildscript_run with bash on PATH: the prelude hands a build
# script rustc and the C compiler as shims whose interpreter is
# `/usr/bin/env bash`.
# The staged bash is itself a shell script that needs the worker's /bin.
def buildscript_run(name, buildscript_rule, env = {}, **kwargs):
    launcher = name + "-absolute-path"
    _absolute_path_buildscript(
        name = launcher,
        buildscript = buildscript_rule,
        visibility = [],
    )
    prelude_buildscript_run(
        name = name,
        buildscript_rule = ":" + launcher,
        env = env | {"PATH": "$(location toolchains//rust:bash_shims)/bin:/bin"},
        **kwargs
    )
