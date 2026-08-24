# A pinned dynamic runtime for prebuilt musl binaries: the musl loader/libc,
# libgcc_s, libstdc++, a set of C libraries, and bash, all staged from one
# pinned Alpine source into a single directory so their symbol versions agree.
# A consumer runs a binary as `loader --library-path <runtime> <binary>`, so
# the artifact carries its own runtime and runs on any host regardless of the
# host libc.
# ICU consumers also set ICU_DATA to the runtime directory, where icu-data's
# .dat is staged flat.

def _musl_runtime_impl(ctx: AnalysisContext) -> list[Provider]:
    # Staging flattens each list to basenames, so two entries in one list
    # sharing a basename would silently overwrite each other.
    for group in [ctx.attrs.files, ctx.attrs.bins]:
        basenames = [f.rsplit("/", 1)[-1] for f in group]
        if len({b: None for b in basenames}) != len(basenames):
            fail("duplicate basenames: {}".format(basenames))

    out = ctx.actions.declare_output("musl_runtime", dir = True)
    pkgs = []
    for i, pkg in enumerate(ctx.attrs.packages):
        downloaded = ctx.actions.declare_output("pkg_{}.apk".format(i))
        ctx.actions.download_file(
            downloaded.as_output(),
            pkg["url"],
            sha256 = pkg["sha256"],
            size_bytes = pkg["size_bytes"],
        )
        pkgs.append(downloaded)

    files = " ".join(ctx.attrs.files)
    bins = " ".join(ctx.attrs.bins)

    # An apk is a concatenation of gzip tars; `tar -xz` reads through it, and
    # `|| true` ignores extraction errors (the signature member always fails).
    # The sha256-pinned inputs and the `set -e` cp of every staged file
    # backstop what that swallows.
    # ld-musl is also libc; the libc.musl soname symlink is normally made by the
    # apk install script, so recreate it here.
    script = """
set -eu
out="$1"; shift
d="$(mktemp -d "$out.XXXXXX")"
for pkg in "$@"; do
  tar -xzf "$pkg" -C "$d" 2>/dev/null || true
done
mkdir -p "$out" "$out/bin"
for f in {files}; do
  cp "$d/$f" "$out/$(basename "$f")"
done
for b in {bins}; do
  cp "$d/$b" "$out/bin/$(basename "$b")"
  chmod 0755 "$out/bin/$(basename "$b")"
done
ln -sf ld-musl-x86_64.so.1 "$out/libc.musl-x86_64.so.1"
rm -rf "$d"
""".format(files = files, bins = bins)
    ctx.actions.run(
        cmd_args("/bin/sh", "-c", script, "musl_runtime", out.as_output(), *pkgs),
        category = "musl_runtime",
        allow_cache_upload = True,
    )
    return [DefaultInfo(default_output = out)]

musl_runtime = rule(
    impl = _musl_runtime_impl,
    attrs = {
        "bins": attrs.list(attrs.string(), doc = "Package-relative paths to stage into the runtime's bin/."),
        "files": attrs.list(attrs.string(), doc = "Package-relative paths to stage flat into the runtime."),
        "packages": attrs.list(attrs.dict(attrs.string(), attrs.one_of(attrs.string(), attrs.int())), doc = "[{url, sha256, size_bytes}] apks to stage from."),
    },
)
