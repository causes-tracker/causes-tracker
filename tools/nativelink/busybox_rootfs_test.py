"""Assert the rootfs tree is a runnable busybox layout.

argv[1] is the rootfs directory, argv[2] the output to touch on success.
"""

import os
import subprocess
import sys

root = sys.argv[1]

bb = os.path.join(root, "bin", "busybox")
assert os.path.isfile(bb) and os.access(bb, os.X_OK), bb

applets = set(
    subprocess.run([bb, "--list"], capture_output=True, text=True, check=True).stdout.split()
)
for applet in ("sh", "tar", "ls", "ln"):
    link = os.path.join(root, "bin", applet)
    assert os.path.islink(link) and os.readlink(link) == "busybox", link
# bash is a real musl binary staged next to the applet symlinks (buck2's
# prelude build-script and clippy wrappers are #!/usr/bin/env bash and use
# BASH_SOURCE), not a busybox applet, so it is excluded from the symlink set.
bash = os.path.join(root, "bin", "bash")
assert os.path.isfile(bash) and not os.path.islink(bash) and os.access(bash, os.X_OK), bash
links = {n for n in os.listdir(os.path.join(root, "bin")) if n not in ("busybox", "bash")}
assert links == applets - {"busybox"}, sorted(applets - {"busybox"} ^ links)

echoed = subprocess.run(
    [os.path.join(root, "bin", "echo"), "ok"], capture_output=True, text=True, check=True
).stdout.strip()
assert echoed == "ok", echoed

for d in ("tmp", "proc", "dev", "etc"):
    assert os.path.isdir(os.path.join(root, d)), d

env = os.path.join(root, "usr", "bin", "env")
assert os.path.islink(env), env

open(sys.argv[2], "w", encoding="utf-8").write("ok")
