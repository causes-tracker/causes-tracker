# The worker rootfs tree: bin/busybox, one symlink per applet, and the
# mount-point directories the runtime expects.
def _busybox_rootfs_impl(ctx: AnalysisContext) -> list[Provider]:
    busybox = ctx.attrs.busybox[DefaultInfo].default_outputs[0]
    out = ctx.actions.declare_output("rootfs", dir = True)
    script = """
set -eu
bb="$1"
root="$2"
mkdir -p "$root/bin" "$root/tmp" "$root/proc" "$root/dev" "$root/etc"
cp "$bb" "$root/bin/busybox"
chmod 0755 "$root/bin/busybox"
"$bb" --list | while read -r applet; do
  [ "$applet" = busybox ] && continue
  ln -s busybox "$root/bin/$applet"
done
"""
    ctx.actions.run(
        cmd_args("/bin/sh", "-c", script, "rootfs", busybox, out.as_output()),
        category = "busybox_rootfs",
    )
    return [DefaultInfo(default_output = out)]

busybox_rootfs = rule(
    impl = _busybox_rootfs_impl,
    attrs = {
        "busybox": attrs.dep(providers = [DefaultInfo]),
    },
)
