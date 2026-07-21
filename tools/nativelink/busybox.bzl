# Provide /bin/busybox from the pinned docker busybox image as an output.
# crane exports the image rootfs; the applets are hardlinks to the one binary,
# so a plain copy of bin/busybox yields the whole multi-call binary.
def _busybox_impl(ctx: AnalysisContext) -> list[Provider]:
    crane = ctx.attrs.crane[DefaultInfo].default_outputs[0]
    out = ctx.actions.declare_output("busybox")
    script = 'set -eu; d="$(mktemp -d)"; "$1" export "$3" "$d/rootfs.tar"; tar -x -C "$d" -f "$d/rootfs.tar"; cp "$d/bin/busybox" "$2"; chmod 0755 "$2"'
    ctx.actions.run(
        cmd_args("/bin/sh", "-c", script, "extract", crane, out.as_output(), ctx.attrs.image),
        category = "busybox",
    )
    return [DefaultInfo(default_output = out)]

busybox = rule(
    impl = _busybox_impl,
    attrs = {
        "crane": attrs.dep(providers = [DefaultInfo]),
        "image": attrs.string(),
    },
)
