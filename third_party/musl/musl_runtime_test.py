"""Assert the assembled runtime's invariants: the staged set derives from
the pinned apks, so a package bump never edits a list here.
Checks: only shared objects, ICU data, and bin/ are staged; the loader and its
libc.musl alias are present; every shared object is a real ELF; and bash runs
through the staged loader with the runtime first on its library path.

argv[1] is the runtime directory, argv[2] the output to touch on success.
"""

import os
import subprocess
import sys

root = sys.argv[1]

entries = sorted(os.listdir(root))
for e in entries:
    assert e == "bin" or ".so" in e or e.endswith(".dat"), e

assert any(e.endswith(".dat") for e in entries), entries

# The one filename consumers may hardcode: musl's loader name is per-arch
# ABI-stable.
assert os.path.isfile(os.path.join(root, "ld-musl-x86_64.so.1")), entries
assert os.path.islink(os.path.join(root, "libc.musl-x86_64.so.1")), entries

# Every shared object is a real ELF, not a truncated or corrupt copy; a
# symlink resolves to its staged target here.
for e in entries:
    if ".so" not in e:
        continue
    with open(os.path.join(root, e), "rb") as fh:
        assert fh.read(4) == b"\x7fELF", e

assert set(os.listdir(os.path.join(root, "bin"))) == {"bash"}

bash = os.path.join(root, "bin", "bash")
assert os.access(bash, os.X_OK), bash

# The documented invocation, resolving bash's libraries (libc.musl, readline,
# ncursesw) with the runtime first on the library path; a non-executable
# loader or mismatched symbol versions fail here where the per-file checks
# above pass.
proc = subprocess.run(
    [os.path.join(root, "ld-musl-x86_64.so.1"), "--library-path", root, bash, "-c", "echo ok"],
    capture_output=True,
)
assert proc.returncode == 0, proc.stderr
assert proc.stdout == b"ok\n", proc.stdout

open(sys.argv[2], "w", encoding="utf-8").write("ok")
