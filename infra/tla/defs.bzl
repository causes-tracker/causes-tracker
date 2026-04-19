"""Macros for running TLC against TLA+ specs hermetically under Bazel."""

load("@rules_shell//shell:sh_test.bzl", "sh_test")

def tla_test(name, srcs, entry, config, tags = None, **kwargs):
    """Run TLC against a TLA+ specification as a bazel test.

    Args:
        name: test target name.
        srcs: all .tla files needed (entry + EXTENDed modules).
        entry: the .tla file to model-check.
        config: the .cfg file describing constants and invariants.
        tags: extra tags (e.g. ["manual"] for slow specs).
        **kwargs: additional sh_test kwargs.
    """
    if entry not in srcs:
        fail("entry %r must be in srcs" % entry)
    if not config.endswith(".cfg"):
        fail("config %r must end in .cfg" % config)

    # Compute repo-prefixed paths so the runner can rlocation() them.
    # `_main` is the workspace name in bzlmod.
    pkg = native.package_name()
    cfg_rlocation = "_main/{}/{}".format(pkg, config)
    tla_rlocation = "_main/{}/{}".format(pkg, entry)

    sh_test(
        name = name,
        srcs = ["//infra/tla:run_tlc.sh"],
        args = [cfg_rlocation, tla_rlocation],
        data = srcs + [
            config,
            "@bazel_tools//tools/bash/runfiles",
            "@temurin_jre_linux_amd64//:files",
            "@tla2tools//file",
        ],
        tags = tags,
        **kwargs
    )
