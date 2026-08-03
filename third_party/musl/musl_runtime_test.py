"""Assert the assembled runtime matches its contract: exactly the staged file
set, every shared object a real ELF, and bash running through the staged
loader with the runtime first on its library path.

argv[1] is the runtime directory, argv[2] the output to touch on success.
"""

import os
import subprocess
import sys

root = sys.argv[1]

# Every file staged into the runtime, including the libc.musl symlink the
# rule itself creates.
expected = (
    "ld-musl-x86_64.so.1",
    "libc.musl-x86_64.so.1",
    "libgcc_s.so.1",
    "libstdc++.so.6",
    "libssl.so.3",
    "libcrypto.so.3",
    "libicui18n.so.74",
    "libicuuc.so.74",
    "libicudata.so.74",
    "libz.so.1",
    "liblzma.so.5",
    "libxml2.so.2",
    "libzstd.so.1",
    "liblz4.so.1",
    "libgssapi_krb5.so.2",
    "libkrb5.so.3",
    "libk5crypto.so.3",
    "libcom_err.so.2",
    "libkrb5support.so.0",
    "libkeyutils.so.1",
    "libreadline.so.8",
    "libncursesw.so.6",
    "icudt74l.dat",
)

# Exact set equality, so a file added to BUCK without a matching entry here
# fails instead of drifting past the list.
actual = set(os.listdir(root))
assert actual == set(expected) | {"bin"}, actual ^ (set(expected) | {"bin"})

# Every shared object is a real ELF, not a truncated or corrupt copy; the
# .dat is ICU data, not ELF, so presence suffices.
for name in expected:
    if ".so" not in name:
        continue
    with open(os.path.join(root, name), "rb") as fh:
        assert fh.read(4) == b"\x7fELF", name

assert set(os.listdir(os.path.join(root, "bin"))) == {"bash"}

bash = os.path.join(root, "bin", "bash")
assert os.path.isfile(bash) and os.access(bash, os.X_OK), bash

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
