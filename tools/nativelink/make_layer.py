"""Write a normalized tar of the tree argv[1] to argv[2].

uid, gid, uname, gname, and mtime are zeroed, modes are pinned to 0755/0644,
and entries are added in sorted order, so the bytes depend only on the
tree's content.
"""

import os
import sys
import tarfile


def normalize(ti):
    ti.uid = ti.gid = 0
    ti.uname = ti.gname = ""
    ti.mtime = 0
    # Modes otherwise ride the builder's umask; only executability is content.
    if ti.isdir() or ti.mode & 0o100:
        ti.mode = 0o755
    else:
        ti.mode = 0o644
    return ti


def write_layer(root, out):
    paths = []
    for dirpath, dirnames, filenames in os.walk(root):
        for n in dirnames + filenames:
            p = os.path.join(dirpath, n)
            paths.append((os.path.relpath(p, root), p))
    with tarfile.open(out, "w", format=tarfile.GNU_FORMAT) as tar:
        for arcname, p in sorted(paths):
            tar.add(p, arcname=arcname, filter=normalize, recursive=False)


if __name__ == "__main__":
    write_layer(sys.argv[1], sys.argv[2])
