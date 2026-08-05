"""Rules over the rootfs.manifest golden: a `buck2 run` target that regenerates
it in the source tree after a re-pin, and a target whose output is the diff
between it and the freshly built tar (buck2.sh prints that diff when a bootstrap
digest mismatch means the built rootfs no longer matches the pinned one).
"""

def _rootfs_manifest_diff_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("rootfs.manifest.diff")
    ctx.actions.run(
        cmd_args(
            ctx.attrs.py[RunInfo],
            ctx.attrs.script,
            "diff",
            ctx.attrs.golden,
            ctx.attrs.tar[DefaultInfo].default_outputs[0],
            out.as_output(),
        ),
        category = "rootfs_manifest_diff",
    )
    return [DefaultInfo(default_output = out)]

rootfs_manifest_diff = rule(
    impl = _rootfs_manifest_diff_impl,
    attrs = {
        "golden": attrs.source(),
        "py": attrs.exec_dep(providers = [RunInfo]),
        "script": attrs.source(),
        "tar": attrs.dep(providers = [DefaultInfo]),
    },
)

def _rootfs_manifest_update_impl(ctx: AnalysisContext) -> list[Provider]:
    # buck2 run's cwd is the project root, so `dest` is a source-relative path
    # (see lib/rust sqlx copyback for the same idiom).
    return [
        DefaultInfo(),
        RunInfo(args = cmd_args(
            ctx.attrs.py[RunInfo],
            ctx.attrs.script,
            "build",
            ctx.attrs.tar[DefaultInfo].default_outputs[0],
            ctx.attrs.dest,
        )),
    ]

rootfs_manifest_update = rule(
    impl = _rootfs_manifest_update_impl,
    attrs = {
        "dest": attrs.string(doc = "Source-relative path to write the manifest to."),
        "py": attrs.exec_dep(providers = [RunInfo]),
        "script": attrs.source(),
        "tar": attrs.dep(providers = [DefaultInfo]),
    },
)
