"""Canonical, human-readable manifest of the worker-layer tar.

WORKER_LAYER_DIGEST is the sha256 of make_layer.py's normalized tar, so the
digest is a pure function of that tar's bytes.
This reads the generated tar back and lists, per member in tar order, the
header fields those bytes carry — type, mode, size, name, link target — with
file content reduced to a sha256.
uid, gid, mtime, uname, and gname are normalized to constants by make_layer.py;
a member carries one only where it deviates from its constant, surfacing a
normalization bug.

  rootfs_manifest.py build <tar> <out>          write <tar>'s manifest to <out>
  rootfs_manifest.py diff <golden> <tar> <out>  write <golden>-vs-<tar> diff to
      <out> (the telemetry buck2.sh prints when a bootstrap digest mismatch
      means the built rootfs no longer matches the pinned one)
  rootfs_manifest.py <tar> <golden> <stamp>     check <tar> matches <golden>; on
      mismatch print the diff and exit 1, else write <stamp> (cached_check form)
"""

import difflib
import hashlib
import sys
import tarfile

# make_layer.py zeroes these; the manifest annotates any member that carries
# a different value.
_NORMALIZED = {"uid": 0, "gid": 0, "mtime": 0, "uname": "", "gname": ""}


def _row(kind, mode, size, name, extra=""):
    return "{} {:04o} {:>10} {}{}".format(kind, mode, size, name, extra)


def _anomalies(m):
    return "".join(
        " {}={!r}".format(field, getattr(m, field))
        for field, want in _NORMALIZED.items()
        if getattr(m, field) != want
    )


def _manifest(tar_path):
    lines = []
    with tarfile.open(tar_path, mode="r:") as tar:
        for m in tar:
            if m.issym():
                line = _row("l", m.mode, "-", m.name, " -> " + m.linkname)
            elif m.islnk():
                line = _row("h", m.mode, "-", m.name, " => " + m.linkname)
            elif m.isdir():
                line = _row("d", m.mode, "-", m.name + "/")
            elif m.isreg():
                digest = hashlib.sha256(tar.extractfile(m).read()).hexdigest()
                line = _row("-", m.mode, m.size, m.name, "  sha256:" + digest)
            else:
                # A device or fifo would need devmajor/devminor to describe in
                # full; the rootfs has none, so this records the typeflag.
                line = _row("?", m.mode, "-", m.name, " type=" + m.type.decode())
            lines.append(line + _anomalies(m))
    return "".join(line + "\n" for line in lines)


def _diff(old_text, new_text, old_name, new_name):
    return "".join(
        difflib.unified_diff(
            old_text.splitlines(keepends=True),
            new_text.splitlines(keepends=True),
            fromfile=old_name,
            tofile=new_name,
        )
    )


def main(argv):
    if argv[1] == "build":
        tar, out = argv[2], argv[3]
        with open(out, "w", encoding="utf-8") as fh:
            fh.write(_manifest(tar))
        return 0
    if argv[1] == "diff":
        golden, tar, out = argv[2], argv[3], argv[4]
        with open(golden, encoding="utf-8") as fh:
            want = fh.read()
        with open(out, "w", encoding="utf-8") as fh:
            fh.write(_diff(want, _manifest(tar), "golden", "built"))
        return 0

    # cached_check form: <tar> <golden> <stamp>.
    tar, golden, stamp = argv[1], argv[2], argv[3]
    built = _manifest(tar)
    with open(golden, encoding="utf-8") as fh:
        want = fh.read()
    if built != want:
        sys.stderr.write(
            "rootfs manifest does not match {}; the worker rootfs changed. "
            "Regenerate it (buck2 run //tools/nativelink:rootfs_manifest_update) "
            "alongside WORKER_LAYER_DIGEST; the diff below describes the change:\n".format(
                golden.rsplit("/", 1)[-1]
            )
        )
        sys.stderr.write(_diff(want, built, "golden", "built"))
        return 1
    with open(stamp, "w", encoding="utf-8") as fh:
        fh.write("ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
