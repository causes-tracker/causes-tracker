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

# Tool at a path inside a directory artifact.
# Projecting a path declares only that subtree as an action input, so a
# tool that reads sibling files at runtime (python3 finds its stdlib
# relative to the binary) breaks under input staging; the whole tree
# rides along as a hidden input instead.
def _tree_tool_impl(ctx: AnalysisContext) -> list[Provider]:
    tree = ctx.attrs.tree[DefaultInfo].default_outputs[0]
    exe = tree.project(ctx.attrs.path)
    return [DefaultInfo(), RunInfo(args = cmd_args(exe, hidden = tree))]

tree_tool = rule(
    impl = _tree_tool_impl,
    attrs = {
        "path": attrs.string(),
        "tree": attrs.dep(providers = [DefaultInfo]),
    },
)

# http_archive whose unpacked tree is remotely cached.
# The prelude's http_archive never sets `allow_cache_upload` on its unpack
# action, so its output can be read from the cache but never repopulates it.
# The tree is a pure function of the sha256-pinned archive, so publishing it
# is safe, and a warm cache serves the tree without contacting the origin.
def _cached_http_archive_impl(ctx: AnalysisContext) -> list[Provider]:
    archive = ctx.actions.declare_output("archive")
    ctx.actions.download_file(
        archive.as_output(),
        ctx.attrs.urls[0],
        sha256 = ctx.attrs.sha256,
    )
    out = ctx.actions.declare_output("out", dir = True)
    script = 'mkdir -p "$2" && tar -x -f "$1" --strip-components={} -C "$2"'.format(
        ctx.attrs.strip_components,
    )
    ctx.actions.run(
        cmd_args("/bin/sh", "-c", script, "unpack", archive, out.as_output()),
        category = "cached_http_archive",
        allow_cache_upload = True,
    )
    sub_targets = {
        path: [DefaultInfo(default_output = out.project(path))]
        for path in ctx.attrs.sub_targets
    }
    return [DefaultInfo(default_output = out, sub_targets = sub_targets)]

cached_http_archive = rule(
    impl = _cached_http_archive_impl,
    attrs = {
        "sha256": attrs.string(),
        "strip_components": attrs.int(default = 0),
        "sub_targets": attrs.list(attrs.string(), default = []),
        "urls": attrs.list(attrs.string()),
    },
)
