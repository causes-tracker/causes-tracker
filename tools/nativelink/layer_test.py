"""Property checks for the worker layer tar and its digest validation.

One permutation per input dimension the function must be invariant to,
asserted at the python interface; uid/gid cannot be varied on disk
without privileges, so normalize() is checked on fabricated TarInfos.
Digest properties cover build_layer's digest output; the pin's pass/fail
stamp is validated in rootfs_manifest_unit_test.py.

argv[1] is the built layer tar, argv[2] the rootfs tree,
argv[3] make_layer.py, argv[4] the output to touch on success.
"""

import hashlib
import importlib.util
import os
import sys
import tarfile
import tempfile

layer, rootfs, make_layer_path = sys.argv[1], sys.argv[2], sys.argv[3]

spec = importlib.util.spec_from_file_location("make_layer", make_layer_path)
make_layer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(make_layer)


def build_tree(base, entries):
    for kind, rel, arg in entries:
        p = os.path.join(base, rel)
        if kind == "dir":
            os.mkdir(p)
        elif kind == "file":
            with open(p, "w", encoding="utf-8") as f:
                f.write(arg)
        elif kind == "symlink":
            os.symlink(arg, p)
        elif kind == "hardlink":
            os.link(os.path.join(base, arg), p)


def set_all_mtimes(base, stamp):
    for dirpath, dirnames, filenames in os.walk(base):
        for n in dirnames + filenames:
            os.utime(os.path.join(dirpath, n), (stamp, stamp), follow_symlinks=False)


def layer_bytes(entries, stamp=None):
    with tempfile.TemporaryDirectory() as td:
        tree = os.path.join(td, "tree")
        os.mkdir(tree)
        build_tree(tree, entries)
        if stamp is not None:
            set_all_mtimes(tree, stamp)
        out = os.path.join(td, "layer.tar")
        make_layer.write_layer(tree, out)
        with open(out, "rb") as f:
            return f.read()


entries = [
    ("dir", "bin", None),
    ("file", "bin/tool", "content"),
    ("symlink", "bin/alias", "tool"),
    ("dir", "etc", None),
]

# Creation-order permutation, including sibling order within a directory.
reordered = [entries[3], entries[0], entries[2], entries[1]]
assert layer_bytes(entries) == layer_bytes(reordered), "creation order leaked"

# mtime permutation.
assert layer_bytes(entries, stamp=12345) == layer_bytes(entries, stamp=67890), "mtime leaked"

# umask permutation: mode bits must not ride the builder's umask.
old_umask = os.umask(0o022)
try:
    strict = layer_bytes(entries)
    os.umask(0o077)
    loose = layer_bytes(entries)
finally:
    os.umask(old_umask)
assert strict == loose, "umask leaked into modes"

# hardlink permutation: REAPI directories cannot carry hardlinks, so a tree
# reaches the builder either linked or with the links exploded into copies; the
# bytes must match either way.
linked = entries + [("hardlink", "bin/tool2", "bin/tool")]
copied = entries + [("file", "bin/tool2", "content")]
assert layer_bytes(linked) == layer_bytes(copied), "link count leaked into bytes"
with tempfile.TemporaryDirectory() as td:
    tree = os.path.join(td, "tree")
    os.mkdir(tree)
    build_tree(tree, linked)
    out = os.path.join(td, "layer.tar")
    make_layer.write_layer(tree, out)
    with tarfile.open(out) as t:
        assert not any(m.islnk() for m in t), "hardlink node in layer tar"

# uid/gid/uname/gname/mtime: normalize zeroes arbitrary values.
ti = tarfile.TarInfo("x")
ti.uid, ti.gid, ti.uname, ti.gname, ti.mtime = 1234, 5678, "who", "grp", 99
ti = make_layer.normalize(ti)
assert (ti.uid, ti.gid, ti.uname, ti.gname, ti.mtime) == (0, 0, "", "", 0)

# Type fidelity and sorted members.
with tempfile.TemporaryDirectory() as td:
    tree = os.path.join(td, "tree")
    os.mkdir(tree)
    build_tree(tree, entries)
    out = os.path.join(td, "layer.tar")
    make_layer.write_layer(tree, out)
    with tarfile.open(out) as t:
        by_name = {m.name: m for m in t.getmembers()}
    assert by_name["bin/tool"].isfile()
    assert by_name["bin/alias"].issym() and by_name["bin/alias"].linkname == "tool"
    assert by_name["etc"].isdir()
    names = list(by_name)
    assert names == sorted(names), "members not sorted"

# Wiring: the built artifact equals a recomputation from the same rootfs.
with tempfile.TemporaryDirectory() as td:
    again = os.path.join(td, "again.tar")
    make_layer.write_layer(rootfs, again)
    with open(layer, "rb") as a, open(again, "rb") as b:
        assert a.read() == b.read(), "built layer differs from recomputation"

# build_layer writes the tar's own sha256 to the digest output.
with tempfile.TemporaryDirectory() as td:
    out = os.path.join(td, "layer.tar")
    digest_out = os.path.join(td, "layer.digest")
    make_layer.build_layer(rootfs, out, digest_out)
    with open(out, "rb") as f:
        expected_digest = hashlib.sha256(f.read()).hexdigest()
    with open(digest_out, encoding="utf-8") as f:
        assert f.read().strip() == expected_digest, "digest output wrong"

open(sys.argv[4], "w", encoding="utf-8").write("ok")
