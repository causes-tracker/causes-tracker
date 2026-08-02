# Provide a real bash (plus its musl runtime deps) from the pinned docker bash
# image, laid out as a rootfs overlay. buck2's rust/cxx build-script shims
# (cc_shim.sh) are `#!/usr/bin/env bash` and use bash-only features
# (BASH_SOURCE), so a busybox-sh shim is not sufficient.
def _bash_bundle_impl(ctx: AnalysisContext) -> list[Provider]:
    crane = ctx.attrs.crane[DefaultInfo].default_outputs[0]
    out = ctx.actions.declare_output("bash_root", dir = True)
    script = """
set -eu
crane="$1"; image="$2"; root="$3"
d="$(mktemp -d)"
"$crane" export "$image" "$d/rootfs.tar"
tar -x -C "$d" -f "$d/rootfs.tar"
mkdir -p "$root/bin" "$root/lib" "$root/usr/lib"
cp "$d/usr/local/bin/bash" "$root/bin/bash"
chmod 0755 "$root/bin/bash"
# musl loader + libc (libc.musl-* is a symlink to ld-musl) and ncurses.
cp -a "$d/lib/ld-musl-x86_64.so.1" "$root/lib/"
cp -a "$d/lib/libc.musl-x86_64.so.1" "$root/lib/"
cp -a "$d"/usr/lib/libncursesw.so.6* "$root/usr/lib/"
"""
    ctx.actions.run(
        cmd_args("/bin/sh", "-c", script, "bash_bundle", crane, ctx.attrs.image, out.as_output()),
        category = "bash_bundle",
    )
    return [DefaultInfo(default_output = out)]

bash_bundle = rule(
    impl = _bash_bundle_impl,
    attrs = {
        "crane": attrs.dep(providers = [DefaultInfo]),
        "image": attrs.string(),
    },
)
