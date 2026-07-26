# The worker layer tar: the rootfs tree serialized by make_layer.py, whose
# normalization makes the bytes a pure function of the tree.
def _worker_layer_impl(ctx: AnalysisContext) -> list[Provider]:
    rootfs = ctx.attrs.rootfs[DefaultInfo].default_outputs[0]
    out = ctx.actions.declare_output("layer.tar")
    digest_out = ctx.actions.declare_output("layer.digest")

    ctx.actions.run(
        cmd_args(
            ctx.attrs.py[RunInfo],
            ctx.attrs.script,
            "build",
            rootfs,
            out.as_output(),
            digest_out.as_output(),
        ),
        category = "worker_layer",
        identifier = "build",
    )
    return [DefaultInfo(default_output = out, sub_targets = {
        "digest": [DefaultInfo(default_output = digest_out)],
    })]

_worker_layer = rule(
    impl = _worker_layer_impl,
    attrs = {
        "py": attrs.exec_dep(providers = [RunInfo]),
        "rootfs": attrs.dep(providers = [DefaultInfo]),
        "script": attrs.source(),
    },
)

def _validate_layer_impl(ctx: AnalysisContext) -> list[Provider]:
    valid_stamp = ctx.actions.declare_output("layer.stamp")

    script = ctx.attrs.script
    layer_provider = ctx.attrs.layer[DefaultInfo]

    validation_spec = ctx.actions.run(
        cmd_args(
            ctx.attrs.py[RunInfo],
            script,
            "check",
            layer_provider.sub_targets["digest"][DefaultInfo].default_outputs,
            "--digest",
            ctx.attrs.digest,
            valid_stamp.as_output(),
        ),
        category = "worker_layer",
        identifier = "validate",
    )

    validation_spec = ValidationSpec(
        name = "layer_digest_validation",
        validation_result = valid_stamp,
    )

    return [
        DefaultInfo(
            default_output = layer_provider.default_outputs[0],
            # other_outputs = [valid_stamp],
            sub_targets = {
                "layer": [layer_provider],
            },
        ),
        ValidationInfo(validations = [validation_spec]),
    ]

_validate_layer = rule(
    impl = _validate_layer_impl,
    attrs = {
        "digest": attrs.string(doc = "Reference output digest to support validation without a hermetic builder"),
        "py": attrs.exec_dep(providers = [RunInfo]),
        "script": attrs.source(doc = "Path to the script that implements the layer validation"),
        "layer": attrs.dep(),
    },
)

# macro to keep the target lean
def worker_layer(name, digest, **kwargs):
    exec_compatible_with = kwargs.get("exec_compatible_with", [])

    _worker_layer(
        name = name + "_build",
        exec_compatible_with = exec_compatible_with,
        py = kwargs.get("py"),
        rootfs = kwargs.get("rootfs"),
        script = kwargs.get("script"),
        visibility = [],
    )

    # build the layer without validation to enable upgrading without special casing
    _validate_layer(
        name = name,
        digest = digest,
        exec_compatible_with = exec_compatible_with,
        layer = ":" + name + "_build",
        py = kwargs.get("py"),
        script = kwargs.get("script"),
    )
