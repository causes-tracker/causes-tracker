"""Extract worker.tar's single OCI layer into a plain rootfs directory.

worker.tar (tools/nativelink:worker_image) is a docker-style OCI image tar:
manifest.json names one gzip-compressed layer tar, which is the actual
busybox-musl rootfs. crun needs a plain directory, not the OCI wrapper, so
tools/buck2/buck2.sh calls this once per image digest to unpack it.
"""

import io
import json
import sys
import tarfile


def extract(image_tar, dest_dir):
    with tarfile.open(image_tar) as image:
        manifest = json.load(image.extractfile("manifest.json"))
        layers = manifest[0]["Layers"]
        assert len(layers) == 1, layers
        layer_bytes = image.extractfile(layers[0]).read()
    kwargs = {"filter": "data"} if hasattr(tarfile, "data_filter") else {}
    with tarfile.open(fileobj=io.BytesIO(layer_bytes)) as layer:
        layer.extractall(dest_dir, **kwargs)


if __name__ == "__main__":
    extract(sys.argv[1], sys.argv[2])
