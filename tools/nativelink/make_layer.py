"""Write a normalized tar of the tree argv[1] to argv[2].

uid, gid, uname, gname, and mtime are zeroed, modes are pinned to 0755/0644,
entries are added in sorted order, and hardlinks are written as full regular
files, so the bytes depend only on each path's content.
REAPI directories do not model hardlinks, so a worker may materialize a tree's
identical-content files either as links or as separate copies.
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


class _HashingWriter:
    """File-like wrapper that feeds every write into a running sha256."""

    def __init__(self, f):
        self._f = f
        self.digest = hashlib.sha256()

    def write(self, data):
        self.digest.update(data)
        return self._f.write(data)

    def tell(self):
        return self._f.tell()


def write_layer(root, out):
    paths = []
    for dirpath, dirnames, filenames in os.walk(root):
        for n in dirnames + filenames:
            p = os.path.join(dirpath, n)
            paths.append((os.path.relpath(p, root), p))
    with open(out, "wb") as raw:
        hasher = _HashingWriter(raw)
        with tarfile.open(fileobj=hasher, mode="w", format=tarfile.GNU_FORMAT) as tar:
            for arcname, p in sorted(paths):
                ti = normalize(tar.gettarinfo(p, arcname))
                if ti.islnk():
                    ti.type = tarfile.REGTYPE
                    ti.linkname = ""
                    ti.size = os.path.getsize(p)
                if ti.isreg():
                    with open(p, "rb") as f:
                        tar.addfile(ti, f)
                else:
                    tar.addfile(ti)
    return hasher.digest.hexdigest()


def build_layer(root, out, digest_out):
    digest = write_layer(root, out)
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
