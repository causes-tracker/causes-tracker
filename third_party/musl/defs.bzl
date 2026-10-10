# A pinned dynamic runtime for prebuilt musl binaries: whatever shared
# libraries and ICU data the pinned apks ship, flattened into a single
# directory with the requested bins, so the staged set tracks the pins and a
# package bump edits nothing but the pin.
# A consumer runs a binary as `loader --library-path <runtime> <binary>`, so
# the artifact carries its own runtime and runs on any host regardless of the
# host libc.
# ICU consumers also set ICU_DATA to the runtime directory, where the icu-data
# .dat is staged flat.

# runtime: directory holding the loader, the shared libraries, and bin/.
MuslRuntimeInfo = provider(fields = {"runtime": provider_field(Artifact)})

def _musl_runtime_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("musl_runtime", dir = True)
    pkgs = [pkg[DefaultInfo].default_outputs[0] for pkg in ctx.attrs.packages]
    ctx.actions.run(
        cmd_args("/bin/sh", ctx.attrs.script, out.as_output(), ctx.attrs.bins, "--", *pkgs),
        category = "musl_runtime",
        allow_cache_upload = True,
    )
    return [DefaultInfo(default_output = out), MuslRuntimeInfo(runtime = out)]

musl_runtime = rule(
    impl = _musl_runtime_impl,
    attrs = {
        "bins": attrs.list(attrs.string(), doc = "Package-relative paths to stage into the runtime's bin/."),
        "packages": attrs.list(attrs.dep(providers = [DefaultInfo]), doc = "pinned_file targets to stage from."),
        "script": attrs.source(),
    },
)
