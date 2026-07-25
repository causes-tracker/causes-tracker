"""Property checks for the pinned cpython tree.

argv[1] is the :cpython output directory, argv[2] the pin file (read for
the version, so a renovate bump never makes this fail spuriously), argv[3]
the output to touch on success.
"""

import os
import re
import sys

root, pin_path = sys.argv[1], sys.argv[2]

pin = open(pin_path, encoding="utf-8").read()
m = re.search(r"cpython-(\d+)\.(\d+)\.\d+\+", pin)
assert m, "no cpython version pinned in " + pin_path
binary = "python" + m.group(1) + "." + m.group(2)

assert os.path.isfile(os.path.join(root, "bin", binary)), "no interpreter at bin/" + binary
assert not os.path.exists(os.path.join(root, "Modules")), (
    "build-tree artifacts leaked into the pinned interpreter tree"
)

open(sys.argv[3], "w", encoding="utf-8").write("ok")
