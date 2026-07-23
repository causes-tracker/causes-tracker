"""Wrap the layer tar in an OCI image tar using crane and validates the digest.

The tag embeds the layer digest so a loaded image is self-identifying;
the bytes stay a pure function of the layer.
"""

import hashlib
import json
import subprocess
import sys


def build_image(crane, layer, out, digest_out):
    with open(layer, "rb") as f:
        layer_digest = hashlib.sha256(f.read()).hexdigest()
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
            "causes-worker:" + layer_digest[:12],
        ],
        check=True,
    )
    with open(out, "rb") as f:
        image_digest = hashlib.sha256(f.read()).hexdigest()
    with open(digest_out, "wt") as f:
        f.write(image_digest + "\n")


def check_digest(digest_out, digest, stamp_path):
    with open(digest_out, "rt") as f:
        actual_digest = f.read().strip()
    if actual_digest != digest:
        valid = {
            "version": 1,
            "data": {
                "status": "failure",
                "message": f"Image digest {actual_digest} does not match expected {digest}",
            },
        }
    else:
        valid = {
            "version": 1,
            "data": {
                "status": "success",
                "message": f"Image digest {actual_digest} validated",
            },
        }
    with open(stamp_path, "wt") as f:
        json.dump(valid, f)


if __name__ == "__main__":
    match sys.argv[1]:
        case "build":
            build_image(sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5])
        case "check":
            assert sys.argv[3] == "--digest", "Expected --digest argument but got: " + sys.argv[3]
            check_digest(sys.argv[2], sys.argv[4], sys.argv[5])
        case _:
            raise RuntimeError("Expected 'build' or 'check' but got: " + sys.argv[1])
