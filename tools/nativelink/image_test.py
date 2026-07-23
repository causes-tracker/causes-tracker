"""Property checks for the worker OCI image tar.

argv[1] is the image tar, argv[2] the layer tar, argv[3] the crane
binary, argv[4] make_image.py, argv[5] the output to touch on success.
"""

import gzip
import hashlib
import importlib.util
import json
import os
import sys
import tarfile
import tempfile

image, layer, crane, make_image_path = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]

spec = importlib.util.spec_from_file_location("make_image", make_image_path)
make_image = importlib.util.module_from_spec(spec)
spec.loader.exec_module(make_image)

with open(layer, "rb") as f:
    layer_bytes = f.read()

with tarfile.open(image) as t:
    names = t.getnames()
    manifest = json.load(t.extractfile("manifest.json"))
    assert len(manifest) == 1, manifest
    entry = manifest[0]
    assert entry["Config"] in names, entry
    layers = entry["Layers"]
    assert len(layers) == 1, layers
    embedded = t.extractfile(layers[0]).read()
    tag = "causes-worker:" + hashlib.sha256(layer_bytes).hexdigest()[:12]
    assert entry["RepoTags"] == [tag], entry

assert gzip.decompress(embedded) == layer_bytes, "embedded layer differs from :layer"

# Reproducibility: rebuilding from the same layer yields identical bytes.
with tempfile.TemporaryDirectory() as td:
    again = os.path.join(td, "again.tar")
    make_image.make_image(crane, layer, again)
    with open(image, "rb") as a, open(again, "rb") as b:
        assert a.read() == b.read(), "image bytes are not reproducible"

open(sys.argv[5], "w", encoding="utf-8").write("ok")
