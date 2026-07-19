#!/bin/sh
# Assemble a from-scratch OCI image (pinned static busybox rootfs) from the
# inputs staged by the worker_image rule and write it to $3.
# Args: $1 busybox binary, $2 crane binary, $3 output image tar.
# Deterministic: fixed rootfs layout, sorted/zeroed tar.
set -eu

busybox="$1"
crane="$2"
out="$3"

work="$(mktemp -d)"
root="$work/rootfs"
mkdir -p "$root/bin" "$root/tmp" "$root/proc" "$root/dev" "$root/etc"
install -m 0755 "$busybox" "$root/bin/busybox"
"$busybox" --list | while read -r applet; do
	[ "$applet" = busybox ] && continue
	ln -sf busybox "$root/bin/$applet"
done

tar --sort=name --mtime='UTC 1970-01-01' --owner=0 --group=0 --numeric-owner \
	--format=gnu -C "$root" -cf "$work/layer.tar" .

"$crane" append --oci-empty-base -f "$work/layer.tar" -o "$out" \
	-t causes-worker:local >&2
