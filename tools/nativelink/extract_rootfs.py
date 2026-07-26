"""Extract the worker layer tar into a plain rootfs directory.

tools/nativelink:layer's output is already the busybox-musl rootfs tree
serialized as a plain tar, so this is a straight extraction; crun needs a
directory, not a tar, so tools/buck2/buck2.sh calls this once per layer
digest to unpack it.
"""

import sys
import tarfile


def extract(layer_tar, dest_dir):
    kwargs = {"filter": "data"} if hasattr(tarfile, "data_filter") else {}
    with tarfile.open(layer_tar) as layer:
        layer.extractall(dest_dir, **kwargs)


if __name__ == "__main__":
    extract(sys.argv[1], sys.argv[2])
