"""Unit checks for rootfs_manifest.py's tar-to-manifest projection.

Fabricated tars exercise each member kind, hardlink dedup, the anomaly
path, and the validate/check modes at the python interface, so a change that
alters the projection or the mode dispatch fails here.

argv[1] is rootfs_manifest.py, argv[2] the output to touch on success.
"""

import hashlib
import importlib.util
import io
import json
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

    # validate: a matching digest stamps success.
    dg = os.path.join(td, "digest")
    with open(dg, "w", encoding="utf-8") as fh:
        fh.write("abc123\n")
    ok_stamp = os.path.join(td, "ok.stamp")
    assert rm.main(["prog", "validate", dg, "abc123", tar, golden, ok_stamp]) == 0
    with open(ok_stamp, encoding="utf-8") as fh:
        assert json.load(fh)["data"]["status"] == "success"

    # validate: a digest mismatch whose manifest still matches the golden is a
    # serialization-only difference, not a content change.
    ser_stamp = os.path.join(td, "ser.stamp")
    assert rm.main(["prog", "validate", dg, "wrong", tar, golden, ser_stamp]) == 0
    with open(ser_stamp, encoding="utf-8") as fh:
        ser = json.load(fh)["data"]
    assert ser["status"] == "failure", ser
    assert "abc123" in ser["message"] and "wrong" in ser["message"], ser
    assert "only the tar serialization differs" in ser["message"], ser

    # validate: a mismatch against a manifest the tar no longer matches carries
    # the diff naming the changed member.
    diff_stamp = os.path.join(td, "diff.stamp")
    assert rm.main(["prog", "validate", dg, "wrong", tar, stale, diff_stamp]) == 0
    with open(diff_stamp, encoding="utf-8") as fh:
        differ = json.load(fh)["data"]
    assert differ["status"] == "failure", differ
    assert "-- 0755" in differ["message"] and "+- 0644" in differ["message"], differ

with open(out, "w", encoding="utf-8") as fh:
    fh.write("ok")
