# Check target that runs an executable and caches the result remotely.
# The executable receives `srcs` then `args`, then the output path as its
# final argument, and must write the output file on success.
# `allow_cache_upload` opts the action into action-cache writes; the
# OSS prelude's genrule hard-codes uploads off, so genrules cannot be used
# for cache-exercising checks.
def _cached_check_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("out.txt")
    ctx.actions.run(
        cmd_args(ctx.attrs.exe[RunInfo], ctx.attrs.srcs, ctx.attrs.args, out.as_output()),
        category = "cached_check",
        allow_cache_upload = True,
    )
    return [DefaultInfo(default_output = out)]

cached_check = rule(
    impl = _cached_check_impl,
    attrs = {
        "args": attrs.list(attrs.string(), default = []),
        "exe": attrs.exec_dep(providers = [RunInfo]),
        "srcs": attrs.list(attrs.source(), default = []),
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

# A single sha256-pinned executable, runnable as an exec_dep.
def _downloaded_tool_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output(ctx.attrs.out)
    ctx.actions.download_file(out.as_output(), ctx.attrs.url, sha256 = ctx.attrs.sha256, is_executable = True)
    return [DefaultInfo(default_output = out), RunInfo(args = cmd_args(out))]

downloaded_tool = rule(
    impl = _downloaded_tool_impl,
    attrs = {
        "out": attrs.string(),
        "sha256": attrs.string(),
        "url": attrs.string(),
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
    if ctx.attrs.zstd:
        # tar has no zstd support on the platforms this runs on; decompress
        # through the pinned zstd tool first, to a temp file (its own
        # statement) so set -e catches a zstd failure.
        extract = '"$zstd" -dc "$archive" >"$zstd_tmp"; tar -x -f "$zstd_tmp" -C "{dest}"'
        tool_args = [ctx.attrs.zstd[RunInfo]]
        arg_prefix = 'set -eu; zstd="$1"; archive="$2"; out="$3"; shift 3; zstd_tmp="$(mktemp)"'
    else:
        extract = 'tar -x -f "$archive" -C "{dest}"'
        tool_args = []
        arg_prefix = 'set -eu; archive="$1"; out="$2"; shift 2'

    if ctx.attrs.paths:
        # tar's own path-selection argument is broken for directory
        # entries on the platforms this runs on: it extracts them fine but
        # still reports "not found" and exits nonzero. Extract everything
        # to a scratch dir and move the wanted (already
        # `strip_components`-deep) subtrees' entries out by hand instead.
        # paths must be disjoint (no path nested under another), since
        # entries from each land directly in the shared "$out".
        # The scratch dir sits next to "$out", on the same filesystem, so the
        # moves below are renames that keep hardlinks; a scratch under a tmpfs
        # $TMPDIR would make them cross-fs copies and break the links.
        script = arg_prefix + '; mkdir -p "$out"; tmp="$(mktemp -d "$out.XXXXXX")"; ' + \
                 extract.format(dest = "$tmp") + \
                 '; for p in "$@"; do for e in "$tmp/$p"/* "$tmp/$p"/.[!.]* "$tmp/$p"/..?*; do ' + \
                 'if [ -e "$e" ]; then mv "$e" "$out/"; fi; done; done'
    else:
        script = arg_prefix + '; mkdir -p "$out"; ' + \
                 extract.format(dest = "$out") + \
                 ' --strip-components={} "$@"'.format(ctx.attrs.strip_components)

    cmd = cmd_args(*(["/bin/sh", "-c", script, "unpack"] + tool_args + [archive, out.as_output()] + ctx.attrs.paths))
    ctx.actions.run(
        cmd,
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
        # Top-level archive paths to extract; empty extracts everything.
        "paths": attrs.list(attrs.string(), default = []),
        "sha256": attrs.string(),
        "strip_components": attrs.int(default = 0),
        "sub_targets": attrs.list(attrs.string(), default = []),
        "urls": attrs.list(attrs.string()),
        # Set when the archive needs zstd decompression tar can't do itself.
        "zstd": attrs.option(attrs.exec_dep(providers = [RunInfo]), default = None),
    },
)
