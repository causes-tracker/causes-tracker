"""Module extension exposing hermetic Rust host-tool binaries as a direct dep.

The rules_rs toolchains extension creates platform-specific repos such as
`cargo_linux_x86_64_1_87_0` but classifies them as *indirect* dependencies,
so listing them in use_repo() triggers a MODULE.bazel warning.  This extension
creates a single `rust_host_tools` repo that symlinks the same binaries out of
the versioned repos (using their stable canonical names) and marks itself as a
*direct* dependency, silencing the warning.

Usage in MODULE.bazel:
    rust_host = use_extension("//tools:rust_host_tools.bzl", "rust_host")
    rust_host.config(version = "1.87.0")   # must match toolchains.toolchain(version=...)
    use_repo(rust_host, "rust_host_tools")

Then in BUILD files:
    data = ["@rust_host_tools//:cargo"]   # rlocation: rust_host_tools/bin/cargo
"""

def _normalize_os(os_name):
    os_name = os_name.lower()
    if os_name.startswith("mac os"):
        return "macos"
    if os_name.startswith("windows"):
        return "windows"
    return os_name

def _normalize_arch(arch):
    arch = arch.lower()
    if arch in ("amd64", "x86_64", "x64"):
        return "x86_64"
    if arch in ("aarch64", "arm64"):
        return "aarch64"
    return arch

def _rust_host_tools_repo_impl(rctx):
    # Sanitise "1.87.0" -> "1_87_0" to match rules_rs naming convention.
    version_key = rctx.attr.version.replace(".", "_")

    os_name = _normalize_os(rctx.os.name)
    arch = _normalize_arch(rctx.os.arch)
    triple_suffix = "{}_{}".format(os_name, arch)

    # Map os+arch to the Rust target triple used in stdlib repo names.
    triple_map = {
        "linux_x86_64": "x86_64_unknown_linux_gnu",
        "linux_aarch64": "aarch64_unknown_linux_gnu",
        "macos_x86_64": "x86_64_apple_darwin",
        "macos_aarch64": "aarch64_apple_darwin",
    }
    target_triple_key = triple_map.get(triple_suffix, "x86_64_unknown_linux_gnu")

    # Map os+arch to the hyphenated exec-triple used in llvm-tools paths.
    # e.g. lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-cov
    exec_triple_map = {
        "linux_x86_64": "x86_64-unknown-linux-gnu",
        "linux_aarch64": "aarch64-unknown-linux-gnu",
        "macos_x86_64": "x86_64-apple-darwin",
        "macos_aarch64": "aarch64-apple-darwin",
    }
    exec_triple = exec_triple_map.get(triple_suffix, "x86_64-unknown-linux-gnu")

    # Navigate to the Bazel external directory via a known file inside
    # @bazel_tools//tools/bash/runfiles (always present, no use_repo needed).
    # runfiles.bash lives at:
    #   <output_base>/external/bazel_tools/tools/bash/runfiles/runfiles.bash
    # so five .dirname calls reach <output_base>/external/.
    runfiles_lib = rctx.path(
        Label("@bazel_tools//tools/bash/runfiles:runfiles.bash"),
    )
    external_dir = str(runfiles_lib.dirname.dirname.dirname.dirname.dirname)

    # The canonical name prefix used by the rules_rs toolchains extension.
    # Format: <defining_module>++<extension_name>+<repo_name>
    ext_prefix = "rules_rs++toolchains+"

    def _bin(tool_repo, binary):
        return "{}/{}/bin/{}".format(external_dir, ext_prefix + tool_repo, binary)

    cargo_src = _bin("cargo_{}_{}".format(triple_suffix, version_key), "cargo")
    rustc_src = _bin("rustc_{}_{}".format(triple_suffix, version_key), "rustc")
    rustfmt_src = _bin("rustfmt_{}_{}".format(triple_suffix, version_key), "rustfmt")

    # llvm-tools binaries live in the llvm_tools repo's rustlib tree.
    # Use the filegroups defined by llvm_tools_repository (llvm_cov_bin /
    # llvm_profdata_bin) as the authoritative source; symlink the raw
    # executables here so callers can use the stable rlocation path
    # rust_host_tools/bin/llvm-{cov,profdata}.
    llvm_tools_repo_dir = "{}/{}".format(
        external_dir,
        ext_prefix + "llvm_tools_{}_{}".format(triple_suffix, version_key),
    )
    llvm_cov_src = "{}/lib/rustlib/{}/bin/llvm-cov".format(llvm_tools_repo_dir, exec_triple)
    llvm_profdata_src = "{}/lib/rustlib/{}/bin/llvm-profdata".format(llvm_tools_repo_dir, exec_triple)

    rctx.execute(["mkdir", "-p", "bin"])
    rctx.symlink(rctx.path(cargo_src), "bin/cargo")
    rctx.symlink(rctx.path(rustc_src), "bin/rustc")
    rctx.symlink(rctx.path(rustfmt_src), "bin/rustfmt")
    rctx.symlink(rctx.path(llvm_cov_src), "bin/llvm-cov")
    rctx.symlink(rctx.path(llvm_profdata_src), "bin/llvm-profdata")

    # Write the stdlib sysroot path to a text file so that hermetic cargo
    # invocations (e.g. sqlx prepare) can set RUSTFLAGS=--sysroot correctly.
    stdlib_dir = "{}/{}/".format(
        external_dir,
        ext_prefix + "rust_stdlib_{}_{}".format(target_triple_key, version_key),
    )
    rctx.file("sysroot_path.txt", stdlib_dir + "\n")

    rctx.file("BUILD.bazel", """\
filegroup(name = "cargo",        srcs = ["bin/cargo"],        visibility = ["//visibility:public"])
filegroup(name = "rustc",        srcs = ["bin/rustc"],        visibility = ["//visibility:public"])
filegroup(name = "rustfmt",      srcs = ["bin/rustfmt"],      visibility = ["//visibility:public"])
filegroup(name = "llvm_cov",     srcs = ["bin/llvm-cov"],     visibility = ["//visibility:public"])
filegroup(name = "llvm_profdata", srcs = ["bin/llvm-profdata"], visibility = ["//visibility:public"])
exports_files(["sysroot_path.txt"])
""")

_rust_host_tools_repo = repository_rule(
    implementation = _rust_host_tools_repo_impl,
    attrs = {
        "version": attr.string(
            mandatory = True,
            doc = "Rust toolchain version string, e.g. '1.87.0'.",
        ),
    },
)

_CONFIG_TAG = tag_class(attrs = {
    "version": attr.string(
        mandatory = True,
        doc = "Rust version; must match toolchains.toolchain(version = ...) in MODULE.bazel.",
    ),
})

def _rust_host_impl(mctx):
    version = None
    for mod in mctx.modules:
        for tag in mod.tags.config:
            if version == None:
                version = tag.version
            elif version != tag.version:
                fail("rust_host.config: conflicting version tags: {} vs {}".format(version, tag.version))

    if version == None:
        fail("rust_host.config(version = ...) must be set in MODULE.bazel")

    _rust_host_tools_repo(name = "rust_host_tools", version = version)

    return mctx.extension_metadata(
        root_module_direct_deps = ["rust_host_tools"],
        root_module_direct_dev_deps = [],
        reproducible = True,
    )

rust_host = module_extension(
    implementation = _rust_host_impl,
    tag_classes = {"config": _CONFIG_TAG},
)
