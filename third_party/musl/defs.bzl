# A pinned dynamic runtime for prebuilt musl binaries: whatever shared
# libraries and ICU data the pinned apks ship, flattened into a single
# directory with the requested bins, so the staged set tracks the pins and a
# package bump edits nothing but the pin.
# The artifact carries its own runtime and runs on any host regardless of the
# host libc.
# ICU consumers also set ICU_DATA to the runtime directory, where the icu-data
# .dat is staged flat.

# loader: the dynamic loader; see _musl_invocation for how it runs a binary.
MuslRuntimeInfo = provider(fields = {
    "loader": provider_field(Artifact),
    "runtime": provider_field(Artifact),
})

def _musl_runtime_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("musl_runtime", dir = True)
    pkgs = [pkg[DefaultInfo].default_outputs[0] for pkg in ctx.attrs.packages]
    ctx.actions.run(
        cmd_args("/bin/sh", ctx.attrs.script, out.as_output(), ctx.attrs.bins, "--", *pkgs),
        category = "musl_runtime",
        allow_cache_upload = True,
    )
    return [
        DefaultInfo(default_output = out),
        MuslRuntimeInfo(loader = out.project("ld-musl-x86_64.so.1"), runtime = out),
    ]

musl_runtime = rule(
    impl = _musl_runtime_impl,
    attrs = {
        "bins": attrs.list(attrs.string(), doc = "Package-relative paths to stage into the runtime's bin/."),
        "packages": attrs.list(attrs.dep(providers = [DefaultInfo]), doc = "pinned_file targets to stage from."),
        "script": attrs.source(),
    },
)

# The one place that spells how a runtime binary is run; musl_tool renders it
# as a RunInfo, musl_shims as scripts.
# `path_format` wraps each path element as it is rendered.
# `argv0`, when given, is what the binary sees as its own name; the loader
# otherwise passes the binary's path.
def _musl_invocation(info: MuslRuntimeInfo, library_dirs: list[Artifact], binary: Artifact, path_format: str = "{}", argv0: str | None = None) -> cmd_args:
    return cmd_args(
        cmd_args(info.loader, format = path_format),
        "--library-path",
        cmd_args([info.runtime] + library_dirs, format = path_format, delimiter = ":"),
        ["--argv0", argv0] if argv0 != None else [],
        cmd_args(binary, format = path_format),
    )

# A runtime binary as a RunInfo tool.
def _musl_tool_impl(ctx: AnalysisContext) -> list[Provider]:
    info = ctx.attrs.runtime[MuslRuntimeInfo]
    return [
        DefaultInfo(),
        RunInfo(args = _musl_invocation(info, [], info.runtime.project(ctx.attrs.path))),
    ]

musl_tool = rule(
    impl = _musl_tool_impl,
    attrs = {
        "path": attrs.string(doc = "Runtime-relative path of the binary."),
        "runtime": attrs.dep(providers = [MuslRuntimeInfo]),
    },
)

# A bin/ of loader scripts, one per named binary of `dist`, each running it
# through the loader with the runtime and the dist's lib/ on the library path
# and the script's own path as argv[0], so a program that locates itself or a
# sibling from argv[0] (pg_ctl -> postgres) finds the scripts.
# Each of `dirs` links to the dist's, for programs that locate their data
# relative to the directory holding argv[0]: PostgreSQL relocates lib/ and
# share/ that way, only when that directory is named bin.
# Every path in a script is relative to the script's own directory, so it
# works from any cwd.
def _musl_shims_impl(ctx: AnalysisContext) -> list[Provider]:
    info = ctx.attrs.runtime[MuslRuntimeInfo]
    dist = ctx.attrs.dist[DefaultInfo].default_outputs[0]
    bin = ctx.actions.declare_output("bin", dir = True)
    dirs = []
    for dir in ctx.attrs.dirs:
        link = ctx.actions.declare_output(dir)
        ctx.actions.symlink_file(link, dist.project(dir))
        dirs.append(link)

    bins = {}
    for name in ctx.attrs.bins:
        inv = _musl_invocation(info, [dist.project("lib")], dist.project("bin/" + name), path_format = '"$here/{}"', argv0 = '"$0"')
        body = cmd_args(
            "#!/bin/sh",
            'here="$(dirname "$0")"',
            cmd_args("exec", inv, '"$@"', delimiter = " "),
            delimiter = "\n",
            relative_to = bin,
        )
        bins[name] = ctx.actions.write("shim/" + name, body, is_executable = True, allow_args = True)[0]
    ctx.actions.copied_dir(bin, bins)
    return [DefaultInfo(default_output = bin.with_associated_artifacts(dirs + [info.runtime, dist]))]

musl_shims = rule(
    impl = _musl_shims_impl,
    attrs = {
        "bins": attrs.list(attrs.string(), doc = "Names under the dist's bin/ to shim."),
        "dirs": attrs.list(attrs.string(), default = [], doc = "Dist directories linked beside bin/."),
        "dist": attrs.dep(providers = [DefaultInfo], doc = "Directory artifact holding bin/ and the dirs to link."),
        "runtime": attrs.dep(providers = [MuslRuntimeInfo]),
    },
)
