# Check target that runs an executable and caches the result remotely.
# The executable receives `args` plus the output path as its final argument
# and must write the output file on success.
# `allow_cache_upload` opts the action into action-cache writes; the
# OSS prelude's genrule hard-codes uploads off, so genrules cannot be used
# for cache-exercising checks.
def _cached_check_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("out.txt")
    ctx.actions.run(
        cmd_args(ctx.attrs.exe[RunInfo], ctx.attrs.args, out.as_output()),
        category = "cached_check",
        allow_cache_upload = True,
    )
    return [DefaultInfo(default_output = out)]

cached_check = rule(
    impl = _cached_check_impl,
    attrs = {
        "args": attrs.list(attrs.string(), default = []),
        "exe": attrs.exec_dep(providers = [RunInfo]),
    },
)
