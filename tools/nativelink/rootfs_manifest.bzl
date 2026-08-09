"""A `buck2 run` target that regenerates the rootfs.manifest golden in the
source tree after a re-pin.
"""

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
