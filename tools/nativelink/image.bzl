"""The worker OCI image tar.

crane wraps the deterministic layer from an empty base, so the image digest is a
pure function of the layer bytes.
"""

def _worker_image_impl(ctx: AnalysisContext) -> list[Provider]:
    crane = ctx.attrs.crane[DefaultInfo].default_outputs[0]
    layer = ctx.attrs.layer[DefaultInfo].default_outputs[0]
    out = ctx.actions.declare_output("worker.tar")
    digest_out = ctx.actions.declare_output("worker.digest")
    script = ctx.attrs._script

    ctx.actions.run(
        cmd_args(
            ctx.attrs.py[RunInfo],
            script,
            "build",
            crane,
            layer,
            out.as_output(),
            digest_out.as_output(),
        ),
        category = "worker_image",
        identifier = "build",
    )
    return [DefaultInfo(default_output = out, sub_targets = {
        "digest": [DefaultInfo(default_output = digest_out)],
    })]

_worker_image = rule(
    impl = _worker_image_impl,
    attrs = {
        "crane": attrs.dep(providers = [DefaultInfo]),
        "layer": attrs.dep(providers = [DefaultInfo]),
        "py": attrs.exec_dep(providers = [RunInfo]),
        "_script": attrs.source(doc = "Path to the script that builds the image"),
    },
)

def _validate_image_impl(ctx: AnalysisContext) -> list[Provider]:
    valid_stamp = ctx.actions.declare_output("image.stamp")

    script = ctx.attrs._script
    image_provider = ctx.attrs.image[DefaultInfo]

    validation_spec = ctx.actions.run(
        cmd_args(
            ctx.attrs.py[RunInfo],
            script,
            "check",
            image_provider.sub_targets["digest"][DefaultInfo].default_outputs,
            "--digest",
            ctx.attrs.digest,
            valid_stamp.as_output(),
        ),
        category = "worker_image",
        identifier = "validate",
    )

    validation_spec = ValidationSpec(
        name = "image_digest_validation",
        validation_result = valid_stamp,
    )

    return [
        DefaultInfo(
            default_output = image_provider.default_outputs[0],
            # other_outputs = [valid_stamp],
            sub_targets = {
                "image": [image_provider],
            },
        ),
        ValidationInfo(validations = [validation_spec]),
    ]

_validate_image = rule(
    impl = _validate_image_impl,
    attrs = {
        "digest": attrs.string(doc = "Reference output digest to support validation without a hermetic builder"),
        "py": attrs.exec_dep(providers = [RunInfo]),
        "_script": attrs.source(doc = "Path to the script that implements the image validation"),
        "image": attrs.dep(),
    },
)

# macro to keep the target lean
def worker_image(name, **kwargs):
    _worker_image(
        name = name + "_build",
        _script = "make_image.py",
        crane = kwargs.get("crane"),
        layer = kwargs.get("layer"),
        py = kwargs.get("py"),
        visibility = [],
    )

    # build the image without validation to enable upgrading without special casing
    _validate_image(
        name = name,
        _script = "make_image.py",
        image = ":" + name + "_build",
        digest = kwargs.get("digest"),
        py = kwargs.get("py"),
    )
