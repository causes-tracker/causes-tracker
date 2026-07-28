# Two platforms.
# `default` hosts every target not routed elsewhere: actions execute on
# the NativeLink worker ([buck2_re_client] in .buckconfig), inputs staged
# from the CAS, so an action sees exactly its declared inputs.
# `image_build` hosts only the worker-image subgraph: limited hybrid, so
# the launcher can build the image with no executor running.
#
# Invariant: the shared cache holds only executor-produced results.
# `default` cannot execute locally at all; `image_build` can, but never
# uploads (a cached result carries no record of how it was produced, so a
# locally-run action must not be able to launder an impure result into
# the shared cache).
# Everything in `//...` remains a cacheable pure function of its declared
# inputs; a target that cannot satisfy this must be redesigned or become
# a `run` target.

def _impl(ctx: AnalysisContext) -> list[Provider]:
    constraints = dict()
    constraints.update(ctx.attrs.cpu_configuration[ConfigurationInfo].constraints)
    constraints.update(ctx.attrs.os_configuration[ConfigurationInfo].constraints)
    cfg = ConfigurationInfo(constraints = constraints, values = {})

    name = ctx.label.raw_target()
    platform = ExecutionPlatformInfo(
        label = name,
        configuration = cfg,
        executor_config = CommandExecutorConfig(
            local_enabled = False,
            remote_enabled = True,
            remote_cache_enabled = True,
            allow_cache_uploads = True,
            remote_execution_properties = {},
            remote_execution_use_case = "buck2-default",
            use_windows_path_separators = False,
        ),
    )

    return [
        DefaultInfo(),
        platform,
        PlatformInfo(label = str(name), configuration = cfg),
        ExecutionPlatformRegistrationInfo(
            platforms = [platform],
        ),
    ]

remote_cache_platform = rule(
    impl = _impl,
    attrs = {
        "cpu_configuration": attrs.dep(providers = [ConfigurationInfo]),
        "os_configuration": attrs.dep(providers = [ConfigurationInfo]),
    },
)

# The worker layer subgraph (see tools/nativelink:layer) needs local
# execution so the buck2 launcher can verify the layer's bytes with
# `--local-only` before handing an executor a socket to serve them from.
# Hybrid: both local and remote stay enabled, so `--local-only` /
# `--remote-only` / cache flags keep selecting per invocation same as any
# other buck2 target; targets opt in via `exec_compatible_with =
# ["//platforms:image_build_enabled"]`.
def _image_build_impl(ctx: AnalysisContext) -> list[Provider]:
    constraints = dict()
    constraints.update(ctx.attrs.cpu_configuration[ConfigurationInfo].constraints)
    constraints.update(ctx.attrs.os_configuration[ConfigurationInfo].constraints)
    marker = ctx.attrs.image_build_marker[ConstraintValueInfo]
    constraints[marker.setting.label] = marker
    cfg = ConfigurationInfo(constraints = constraints, values = {})

    name = ctx.label.raw_target()

    # Reads gated on a key: keyless, the cache address is NativeLink, which
    # the launcher's --local-only cold start cannot assume is up.
    # The launcher separately points a cold start straight at BuildBuddy via
    # .buckconfig.prelaunch (see tools/buck2/buck2.sh) when a key is present.
    has_cache_key = read_config("buck2_re_client", "http_headers") != None

    # Overridable so a rootfs-digest-change validation build can target the
    # still-running worker from the *previous* pin (see the "Changing the
    # rootfs digest" section of tools/nativelink/README.md) while the
    # target being built reads the new pin from source.
    container_image_override = read_config("image_build", "container_image")
    container_image = container_image_override if container_image_override != None else ctx.attrs.container_image

    platform = ExecutionPlatformInfo(
        label = name,
        configuration = cfg,
        executor_config = CommandExecutorConfig(
            local_enabled = True,
            remote_enabled = True,
            use_limited_hybrid = True,
            remote_cache_enabled = has_cache_key,
            allow_cache_uploads = False,
            remote_execution_properties = {"container-image": container_image},
            remote_execution_use_case = "buck2-default",
            use_windows_path_separators = False,
        ),
    )

    return [
        DefaultInfo(),
        platform,
        PlatformInfo(label = str(name), configuration = cfg),
        ExecutionPlatformRegistrationInfo(
            platforms = [platform],
        ),
    ]

image_build_platform = rule(
    impl = _image_build_impl,
    attrs = {
        "container_image": attrs.string(doc = "Worker layer digest; bumping it invalidates the AC for image_build actions"),
        "cpu_configuration": attrs.dep(providers = [ConfigurationInfo]),
        "image_build_marker": attrs.dep(providers = [ConstraintValueInfo]),
        "os_configuration": attrs.dep(providers = [ConfigurationInfo]),
    },
)

# For actions that must never reach a remote executor at all (e.g. a
# credential baked into the command — remote dispatch requires uploading
# the Action/Command proto to CAS as a precondition of execution, which
# `allow_cache_uploads = False` does not prevent, since that only gates the
# *result* write, not the *request* upload needed just to dispatch it).
# remote_enabled = False removes the capability outright, the same shape as
# `image_build`'s allow_cache_uploads = False: a target opting in this way
# gets a real guarantee, not a scheduling preference like local_only on an
# individual action would.
def _local_only_impl(ctx: AnalysisContext) -> list[Provider]:
    constraints = dict()
    constraints.update(ctx.attrs.cpu_configuration[ConfigurationInfo].constraints)
    constraints.update(ctx.attrs.os_configuration[ConfigurationInfo].constraints)
    marker = ctx.attrs.local_only_marker[ConstraintValueInfo]
    constraints[marker.setting.label] = marker
    cfg = ConfigurationInfo(constraints = constraints, values = {})

    name = ctx.label.raw_target()
    platform = ExecutionPlatformInfo(
        label = name,
        configuration = cfg,
        executor_config = CommandExecutorConfig(
            local_enabled = True,
            remote_enabled = False,
            remote_cache_enabled = False,
            allow_cache_uploads = False,
            remote_execution_use_case = "buck2-default",
            use_windows_path_separators = False,
        ),
    )

    return [
        DefaultInfo(),
        platform,
        PlatformInfo(label = str(name), configuration = cfg),
        ExecutionPlatformRegistrationInfo(
            platforms = [platform],
        ),
    ]

local_only_platform = rule(
    impl = _local_only_impl,
    attrs = {
        "cpu_configuration": attrs.dep(providers = [ConfigurationInfo]),
        "local_only_marker": attrs.dep(providers = [ConstraintValueInfo]),
        "os_configuration": attrs.dep(providers = [ConfigurationInfo]),
    },
)

# `[build] execution_platforms` in .buckconfig names exactly one target, so
# the registered platforms are combined here.
# Order matters: a target with no exec_compatible_with matches the first
# entry, so :default (remote-only) must stay first.
def _platform_group_impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        DefaultInfo(),
        ExecutionPlatformRegistrationInfo(
            platforms = [dep[ExecutionPlatformInfo] for dep in ctx.attrs.platforms],
        ),
    ]

platform_group = rule(
    impl = _platform_group_impl,
    attrs = {
        "platforms": attrs.list(attrs.dep(providers = [ExecutionPlatformInfo])),
    },
)
