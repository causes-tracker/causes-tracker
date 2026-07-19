# Build the NativeLink worker's from-scratch OCI image as a cacheable action.
# busybox is downloaded by pinned sha256; crane is a pinned cached_http_archive.
# The image is a pure function of them, so its output digest is stable.
def _worker_image_impl(ctx: AnalysisContext) -> list[Provider]:
    busybox = ctx.actions.declare_output("busybox")
    ctx.actions.download_file(
        busybox.as_output(),
        ctx.attrs.busybox_url,
        sha256 = ctx.attrs.busybox_sha256,
        is_executable = True,
    )
    crane = ctx.attrs.crane[DefaultInfo].default_outputs[0]
    out = ctx.actions.declare_output("worker.tar")
    ctx.actions.run(
        cmd_args("/bin/sh", ctx.attrs.assemble, busybox, crane, out.as_output()),
        category = "worker_image",
        allow_cache_upload = True,
    )
    return [DefaultInfo(default_output = out)]

worker_image = rule(
    impl = _worker_image_impl,
    attrs = {
        "assemble": attrs.source(),
        "busybox_sha256": attrs.string(),
        "busybox_url": attrs.string(),
        "crane": attrs.dep(providers = [DefaultInfo]),
    },
)
