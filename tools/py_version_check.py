"""Assert //tools:py is the cpython pinned in third_party/python/BUCK, running
from the hermetic tree rather than a system python.

argv[1] is the pin file (read for the version, so it never drifts on a bump);
argv[2] is the output to touch on success.
"""

import re
import sys

pin = open(sys.argv[1], encoding="utf-8").read()
m = re.search(r"cpython-(\d+)\.(\d+)\.(\d+)\+", pin)
assert m, "no cpython version pinned in " + sys.argv[1]
want = tuple(int(g) for g in m.groups())
assert sys.version_info[:3] == want, "%s != %s" % (sys.version_info[:3], want)
assert "cpython" in sys.executable, "not the pinned interpreter: " + sys.executable
open(sys.argv[2], "w", encoding="utf-8").write("ok")
