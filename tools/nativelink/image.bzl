# The worker OCI image tar: crane wraps the deterministic layer from an
# empty base, so the image digest is a pure function of the layer bytes.
def _worker_image_impl(ctx: AnalysisContext) -> list[Provider]:
    crane = ctx.attrs.crane[DefaultInfo].default_outputs[0]
    layer = ctx.attrs.layer[DefaultInfo].default_outputs[0]
    out = ctx.actions.declare_output("worker.tar")
    ctx.actions.run(
        cmd_args(
            ctx.attrs.py[RunInfo],
            ctx.attrs.script,
            crane,
            layer,
            out.as_output(),
        ),
        category = "worker_image",
    )
    return [DefaultInfo(default_output = out)]

worker_image = rule(
    impl = _worker_image_impl,
    attrs = {
        "crane": attrs.dep(providers = [DefaultInfo]),
        "layer": attrs.dep(providers = [DefaultInfo]),
        "py": attrs.exec_dep(providers = [RunInfo]),
        "script": attrs.source(),
    },
)
