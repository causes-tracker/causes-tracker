"""Unit checks for rootfs_manifest.py's tar-to-manifest projection.

Fabricated tars exercise each member kind, hardlink dedup, the anomaly
path, and the diff/check modes at the python interface, so a change that
alters the projection or the mode dispatch fails here.

argv[1] is rootfs_manifest.py, argv[2] the output to touch on success.
"""

import hashlib
import importlib.util
import io
import os
import sys
import tarfile
import tempfile

mod_path, out = sys.argv[1], sys.argv[2]

spec = importlib.util.spec_from_file_location("rootfs_manifest", mod_path)
rm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rm)

BODY = b"hello\n"
DIGEST = hashlib.sha256(BODY).hexdigest()


def _member(name, type_, mode, linkname="", **attrs):
    ti = tarfile.TarInfo(name)
    ti.type = type_
    ti.mode = mode
    ti.linkname = linkname
    for key, value in attrs.items():
        setattr(ti, key, value)
    return ti


def write_tar(path, members):
    with tarfile.open(path, mode="w", format=tarfile.GNU_FORMAT) as tar:
        for ti, data in members:
            if data is None:
                tar.addfile(ti)
            else:
                ti.size = len(data)
                tar.addfile(ti, io.BytesIO(data))


def manifest_of(members):
    with tempfile.TemporaryDirectory() as td:
        path = os.path.join(td, "t.tar")
        write_tar(path, members)
        return rm._manifest(path)


# Every member kind, in tar order, with hardlink dedup: z/Accra shares
# z/Abidjan's inode, so tarfile emits it as a link with no content.
lines = manifest_of(
    [
        (_member("bin", tarfile.DIRTYPE, 0o755), None),
        (_member("bin/sh", tarfile.SYMTYPE, 0o755, linkname="busybox"), None),
        (_member("bin/tool", tarfile.REGTYPE, 0o755), BODY),
        (_member("etc/hosts", tarfile.REGTYPE, 0o644), BODY),
        (_member("z/Abidjan", tarfile.REGTYPE, 0o644), BODY),
        (_member("z/Accra", tarfile.LNKTYPE, 0o644, linkname="z/Abidjan"), None),
    ]
).splitlines()
assert lines == [
    rm._row("d", 0o755, "-", "bin/"),
    rm._row("l", 0o755, "-", "bin/sh", " -> busybox"),
    rm._row("-", 0o755, len(BODY), "bin/tool", "  sha256:" + DIGEST),
    rm._row("-", 0o644, len(BODY), "etc/hosts", "  sha256:" + DIGEST),
    rm._row("-", 0o644, len(BODY), "z/Abidjan", "  sha256:" + DIGEST),
    rm._row("h", 0o644, "-", "z/Accra", " => z/Abidjan"),
], lines

# A device node has no content; the fallback records its typeflag.
special = manifest_of([(_member("dev/null", tarfile.CHRTYPE, 0o644), None)])
assert special == rm._row("?", 0o644, "-", "dev/null", " type=3") + "\n", special

# Anomalies: a fully normalized member carries nothing; a member with a
# non-constant field carries just that field, in _NORMALIZED order.
assert rm._anomalies(tarfile.TarInfo("x")) == ""
assert rm._anomalies(_member("x", tarfile.REGTYPE, 0o644, uid=1000, mtime=5)) == " uid=1000 mtime=5"
anomalous = manifest_of([(_member("f", tarfile.REGTYPE, 0o644, uid=1000, gname="dev"), BODY)])
assert anomalous.rstrip("\n").endswith(" uid=1000 gname='dev'"), anomalous

with tempfile.TemporaryDirectory() as td:
    tar = os.path.join(td, "t.tar")
    write_tar(tar, [(_member("f", tarfile.REGTYPE, 0o644), BODY)])
    golden = os.path.join(td, "golden")
    stamp = os.path.join(td, "stamp")

    # build writes the manifest; that manifest is the matching golden.
    assert rm.main(["prog", "build", tar, golden]) == 0
    with open(golden, encoding="utf-8") as fh:
        assert fh.read() == rm._manifest(tar)

    # check: matching golden writes the stamp and returns 0.
    assert rm.main(["prog", tar, golden, stamp]) == 0
    with open(stamp, encoding="utf-8") as fh:
        assert fh.read() == "ok"

    # check: a golden the tar no longer matches returns 1 and does not stamp.
    stale = os.path.join(td, "stale")
    other = os.path.join(td, "u.tar")
    write_tar(other, [(_member("f", tarfile.REGTYPE, 0o755), BODY)])
    rm.main(["prog", "build", other, stale])
    missing = os.path.join(td, "missing_stamp")
    assert rm.main(["prog", tar, stale, missing]) == 1
    assert not os.path.exists(missing)

    # diff: golden vs a changed tar names the changed member; identical is empty.
    changed = os.path.join(td, "changed.diff")
    assert rm.main(["prog", "diff", stale, tar, changed]) == 0
    with open(changed, encoding="utf-8") as fh:
        diff_text = fh.read()
    assert "-- 0755" in diff_text and "+- 0644" in diff_text, diff_text
    same = os.path.join(td, "same.diff")
    rm.main(["prog", "diff", golden, tar, same])
    with open(same, encoding="utf-8") as fh:
        assert fh.read() == "", "identical tar must diff empty"

with open(out, "w", encoding="utf-8") as fh:
    fh.write("ok")
