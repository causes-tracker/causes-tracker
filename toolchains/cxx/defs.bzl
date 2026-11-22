# A C/C++ toolchain on zig cc, targeting musl so its binaries run on the
# from-scratch worker.
# objcopy and strip, which zig does not ship, are the llvm tools from the Rust
# sysroot: the prelude's placeholder variables need objcopy, and its rust and
# cxx rules declare a strip action for every library.

load(
    "@prelude//cxx:cxx_toolchain_types.bzl",
    "BinaryUtilitiesInfo",
    "CCompilerInfo",
    "CxxCompilerInfo",
    "CxxInternalTools",
    "LinkerInfo",
    "LinkerType",
    "ShlibInterfacesMode",
    "cxx_toolchain_infos",
)
load("@prelude//cxx:headers.bzl", "HeaderMode")
load("@prelude//cxx:linker.bzl", "is_pdb_generated")
load("@prelude//linking:link_info.bzl", "LinkStyle")

def _zig_cxx_toolchain_impl(ctx: AnalysisContext) -> list[Provider]:
    zig = cmd_args("/bin/sh", ctx.attrs.flatten, ctx.attrs.zig[RunInfo])
    llvm = ctx.attrs.llvm_tools[DefaultInfo].default_outputs

    def llvm_tool(name: str) -> RunInfo:
        return RunInfo(args = cmd_args(cmd_args(llvm, format = "{}/bin/" + name), hidden = llvm))

    target = ["-target", ctx.attrs.target]
    linker_type = LinkerType("gnu")
    return [DefaultInfo()] + cxx_toolchain_infos(
        internal_tools = ctx.attrs._cxx_internal_tools[CxxInternalTools],
        platform_name = ctx.attrs.target.split("-")[0],
        c_compiler_info = CCompilerInfo(
            compiler = RunInfo(args = cmd_args(zig, "cc")),
            compiler_type = "clang",
            compiler_flags = cmd_args(target),
            preprocessor_flags = cmd_args(),
        ),
        cxx_compiler_info = CxxCompilerInfo(
            compiler = RunInfo(args = cmd_args(zig, "c++")),
            compiler_type = "clang",
            compiler_flags = cmd_args(target),
            preprocessor_flags = cmd_args(),
        ),
        linker_info = LinkerInfo(
            archiver = RunInfo(args = cmd_args(zig, "ar")),
            archiver_type = "gnu",
            archiver_supports_argfiles = True,
            binary_extension = "",
            link_binaries_locally = False,
            link_style = LinkStyle("static"),
            linker = RunInfo(args = cmd_args(zig, "c++")),
            linker_flags = cmd_args(target),
            object_file_extension = "o",
            shlib_interfaces = ShlibInterfacesMode("disabled"),
            shared_dep_runtime_ld_flags = [],
            shared_library_name_default_prefix = "lib",
            shared_library_name_format = "{}.so",
            shared_library_versioned_name_format = "{}.so.{}",
            static_dep_runtime_ld_flags = [],
            static_library_extension = "a",
            static_pic_dep_runtime_ld_flags = [],
            independent_shlib_interface_linker_flags = [],
            type = linker_type,
            use_archiver_flags = True,
            is_pdb_generated = is_pdb_generated(linker_type, []),
        ),
        binary_utilities_info = BinaryUtilitiesInfo(
            objcopy = llvm_tool("llvm-objcopy"),
            strip = llvm_tool("llvm-strip"),
        ),
        header_mode = HeaderMode("symlink_tree_only"),
    )

zig_cxx_toolchain = rule(
    impl = _zig_cxx_toolchain_impl,
    attrs = {
        "flatten": attrs.default_only(attrs.source(default = "//cxx:zig_flatten")),
        "llvm_tools": attrs.exec_dep(providers = [DefaultInfo], doc = "Tree whose bin/ runs llvm-objcopy and llvm-strip."),
        "target": attrs.string(doc = "zig target triple, e.g. x86_64-linux-musl."),
        "zig": attrs.exec_dep(providers = [RunInfo], doc = "The zig binary, with its lib/ beside it."),
        "_cxx_internal_tools": attrs.default_only(attrs.dep(providers = [CxxInternalTools], default = "prelude//cxx/tools:internal_tools")),
    },
    is_toolchain_rule = True,
)
