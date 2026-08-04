# The worker rootfs tree: bin/busybox, one symlink per applet, the base-OS
# files (/etc/passwd, /etc/hosts, zoneinfo), and the runtime mount points.
def _busybox_rootfs_impl(ctx: AnalysisContext) -> list[Provider]:
    busybox = ctx.attrs.busybox[DefaultInfo].default_outputs[0]
    zoneinfo = ctx.attrs.zoneinfo[DefaultInfo].default_outputs[0]
    out = ctx.actions.declare_output("rootfs", dir = True)
    script = """
set -eu
bb="$1"
root="$2"
zoneinfo="$3"
mkdir -p "$root/bin" "$root/usr/bin" "$root/usr/share" "$root/tmp" "$root/proc" "$root/dev" "$root/etc"
cp "$bb" "$root/bin/busybox"
chmod 0755 "$root/bin/busybox"
"$bb" --list | while read -r applet; do
  [ "$applet" = busybox ] && continue
  # env also at /usr/bin, where #!/usr/bin/env shebangs look.
  [ "$applet" = env ] && ln -s /bin/busybox "$root/usr/bin/env"
  ln -s busybox "$root/bin/$applet"
done
# passwd entry for the crun action uid (12021).
printf 'root:x:0:0:root:/root:/bin/sh\\npostgres:x:12021:0:PostgreSQL:/tmp:/bin/sh\\n' >"$root/etc/passwd"
printf '127.0.0.1 localhost\\n::1 localhost\\n' >"$root/etc/hosts"
cp -a "$zoneinfo" "$root/usr/share/zoneinfo"
"""
    ctx.actions.run(
        cmd_args("/bin/sh", "-c", script, "rootfs", busybox, out.as_output(), zoneinfo),
        category = "busybox_rootfs",
    )
    return [DefaultInfo(default_output = out)]

busybox_rootfs = rule(
    impl = _busybox_rootfs_impl,
    attrs = {
        "busybox": attrs.dep(providers = [DefaultInfo]),
        "zoneinfo": attrs.dep(providers = [DefaultInfo]),
    },
)
