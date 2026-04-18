"""Cacheable format-check primitives.

Unlike aspect_rules_lint's format_multirun (which runs uncached every time via
bazel run), these macros emit sh_test targets whose inputs — source files,
formatter binary, formatter config — are declared. Bazel's action cache keys
on those inputs, so repeat runs on an unchanged tree hit the cache instantly.
"""

load("@rules_shell//shell:sh_test.bzl", "sh_test")

_COMMON_DATA = [
    "@bazel_tools//tools/bash/runfiles",
    "@rules_shell//shell/runfiles",
]
_FORMAT_TAG = "format_check_auto"

def rustfmt_check(name, srcs, tags = [], **kwargs):
    """sh_test that runs rustfmt --check over the .rs files in srcs.

    Declared inputs:
    - the .rs files themselves
    - //:rustfmt.toml
    - @rust_host_tools//:rustfmt
    - the driver script

    No-op when srcs contains no .rs files (e.g. rust_test with crate = ":foo").
    """
    rs_srcs = [s for s in srcs if type(s) == "string" and s.endswith(".rs")]
    if not rs_srcs:
        return
    sh_test(
        name = name,
        srcs = ["//tools/lint:run_rustfmt_check.sh"],
        args = ["$(rootpath %s)" % s for s in rs_srcs],
        data = rs_srcs + [
            "//:rustfmt.toml",
            "@rust_host_tools//:rustfmt",
        ] + _COMMON_DATA,
        tags = tags + [_FORMAT_TAG],
        **kwargs
    )

def format_srcs():
    """Per-language filegroups aggregating this package's formattable files.

    Call once at the end of each BUILD.bazel. The root-level format-check
    targets concatenate each package's filegroups into one aggregated
    filegroup per language.
    """
    native.filegroup(
        name = "starlark_format_srcs",
        srcs = native.glob(
            ["*.bzl", "BUILD.bazel", "*.bazel"],
            allow_empty = True,
        ),
        visibility = ["//visibility:public"],
    )
    native.filegroup(
        name = "toml_format_srcs",
        srcs = native.glob(["*.toml"], allow_empty = True),
        visibility = ["//visibility:public"],
    )
    native.filegroup(
        name = "shell_format_srcs",
        srcs = native.glob(["*.sh"], allow_empty = True),
        visibility = ["//visibility:public"],
    )
    native.filegroup(
        name = "yaml_format_srcs",
        srcs = native.glob(
            ["*.yml", "*.yaml"],
            allow_empty = True,
        ),
        visibility = ["//visibility:public"],
    )

def _formatter_check(name, driver, formatter_data, srcs, tags = [], **kwargs):
    sh_test(
        name = name,
        srcs = [driver],
        args = ["$(rootpaths %s)" % s for s in srcs],
        data = srcs + formatter_data + _COMMON_DATA,
        tags = tags + [_FORMAT_TAG],
        **kwargs
    )

def buildifier_check(name, srcs, **kwargs):
    """sh_test that runs buildifier -mode=check over the Starlark files in srcs."""
    _formatter_check(
        name = name,
        driver = "//tools/lint:run_buildifier_check.sh",
        formatter_data = ["@buildifier_prebuilt//:buildifier"],
        srcs = srcs,
        **kwargs
    )

def taplo_check(name, srcs, **kwargs):
    """sh_test that runs `taplo format --check` over the TOML files in srcs."""
    _formatter_check(
        name = name,
        driver = "//tools/lint:run_taplo_check.sh",
        formatter_data = ["//tools/lint:taplo_extracted"],
        srcs = srcs,
        **kwargs
    )

def shfmt_check(name, srcs, **kwargs):
    """sh_test that runs `shfmt -d` (diff mode) over the shell files in srcs."""
    _formatter_check(
        name = name,
        driver = "//tools/lint:run_shfmt_check.sh",
        formatter_data = ["@aspect_rules_lint//format:shfmt"],
        srcs = srcs,
        **kwargs
    )

def yamlfmt_check(name, srcs, **kwargs):
    """sh_test that runs `yamlfmt -lint -conf //:.yamlfmt` over the YAML files in srcs."""
    _formatter_check(
        name = name,
        driver = "//tools/lint:run_yamlfmt_check.sh",
        formatter_data = [
            "@aspect_rules_lint//format:yamlfmt",
            "//:.yamlfmt",
        ],
        srcs = srcs,
        **kwargs
    )
