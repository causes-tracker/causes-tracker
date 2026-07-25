"""Write a normalized tar of the tree argv[1] to argv[2].

uid, gid, uname, gname, and mtime are zeroed, modes are pinned to 0755/0644,
and entries are added in sorted order, so the bytes depend only on the
tree's content.
"""

import hashlib
import json
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


def build_layer(root, out, digest_out):
    write_layer(root, out)
    with open(out, "rb") as f:
        digest = hashlib.sha256(f.read()).hexdigest()
    with open(digest_out, "wt") as f:
        f.write(digest + "\n")


def check_digest(digest_out, digest, stamp_path):
    with open(digest_out, "rt") as f:
        actual_digest = f.read().strip()
    if actual_digest != digest:
        valid = {
            "version": 1,
            "data": {
                "status": "failure",
                "message": f"Layer digest {actual_digest} does not match expected {digest}",
            },
        }
    else:
        valid = {
            "version": 1,
            "data": {
                "status": "success",
                "message": f"Layer digest {actual_digest} validated",
            },
        }
    with open(stamp_path, "wt") as f:
        json.dump(valid, f)


if __name__ == "__main__":
    match sys.argv[1]:
        case "build":
            build_layer(sys.argv[2], sys.argv[3], sys.argv[4])
        case "check":
            assert sys.argv[3] == "--digest", "Expected --digest argument but got: " + sys.argv[3]
            check_digest(sys.argv[2], sys.argv[4], sys.argv[5])
        case _:
            raise RuntimeError("Expected 'build' or 'check' but got: " + sys.argv[1])
