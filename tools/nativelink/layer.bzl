# The worker layer tar: the rootfs tree serialized by make_layer.py, whose
# normalization makes the bytes a pure function of the tree.
def _worker_layer_impl(ctx: AnalysisContext) -> list[Provider]:
    rootfs = ctx.attrs.rootfs[DefaultInfo].default_outputs[0]
    out = ctx.actions.declare_output("layer.tar")
    ctx.actions.run(
        cmd_args(ctx.attrs.py[RunInfo], ctx.attrs.script, rootfs, out.as_output()),
        category = "worker_layer",
    )
    return [DefaultInfo(default_output = out)]

worker_layer = rule(
    impl = _worker_layer_impl,
    attrs = {
        "py": attrs.exec_dep(providers = [RunInfo]),
        "rootfs": attrs.dep(providers = [DefaultInfo]),
        "script": attrs.source(),
    },
)
