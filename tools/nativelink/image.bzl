# Build the NativeLink worker's from-scratch OCI image as a cacheable action.
# crane is a pinned cached_http_archive; busybox is extracted from the Docker
# busybox image pinned by digest. The image is a pure function of them, so its
# output digest is stable.
def _worker_image_impl(ctx: AnalysisContext) -> list[Provider]:
    crane = ctx.attrs.crane[DefaultInfo].default_outputs[0]
    busybox = ctx.actions.declare_output("busybox")
    extract = 'set -eu; d="$(mktemp -d)"; "$1" export "$3" - | tar -x -C "$d"; cp "$d/bin/busybox" "$2"; chmod 0755 "$2"'
    ctx.actions.run(
        cmd_args("/bin/sh", "-c", extract, "extract", crane, busybox.as_output(), ctx.attrs.busybox_image),
        category = "busybox_extract",
    )
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
        "busybox_image": attrs.string(),
        "crane": attrs.dep(providers = [DefaultInfo]),
    },
)
