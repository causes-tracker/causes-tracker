"""Wrap the layer tar argv[2] in an OCI image tar argv[3] using crane argv[1].

The tag embeds the layer digest so a loaded image is self-identifying;
the bytes stay a pure function of the layer.
"""

import hashlib
import subprocess
import sys


def make_image(crane, layer, out):
    with open(layer, "rb") as f:
        digest = hashlib.sha256(f.read()).hexdigest()
    subprocess.run(
        [
            crane,
            "append",
            "--oci-empty-base",
            "-f",
            layer,
            "-o",
            out,
            "-t",
            "causes-worker:" + digest[:12],
        ],
        check=True,
    )


if __name__ == "__main__":
    make_image(sys.argv[1], sys.argv[2], sys.argv[3])
