"""Starlark rules for the infra/postgres package."""

def _extract_tarball_impl(ctx):
    """Extracts a tarball into a directory tree artifact (TreeArtifact).

    Uses ctx.actions.declare_directory so Bazel tracks the whole directory
    as a single artifact; individual files within are accessible at runtime
    via rlocation <repo>/<package>/<name>/<path-within-archive>.
    """
    out_dir = ctx.actions.declare_directory(ctx.attr.name)
    ctx.actions.run_shell(
        inputs = ctx.files.src,
        outputs = [out_dir],
        command = "mkdir -p {outdir} && tar -xzf {tarball} -C {outdir} --strip-components=1".format(
            outdir = out_dir.path,
            tarball = ctx.files.src[0].path,
        ),
        mnemonic = "ExtractTarball",
        progress_message = "Extracting %s" % ctx.files.src[0].basename,
    )
    return [DefaultInfo(files = depset([out_dir]))]

extract_tarball = rule(
    implementation = _extract_tarball_impl,
    attrs = {
        "src": attr.label(
            allow_single_file = True,
            mandatory = True,
            doc = "The tarball to extract.",
        ),
    },
)
