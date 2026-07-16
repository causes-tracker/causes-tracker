# The one platform every target and action uses. Every action executes on
# the local NativeLink worker ([buck2_re_client] in .buckconfig): inputs
# are staged from the CAS, so an action sees exactly its declared inputs.
# Unsandboxed local execution is not a path — a cached result carries no
# record of how it was produced, so allowing both would launder impure
# results into the shared cache.
#
# Invariant: everything in `//...` is a cacheable pure function of its
# declared inputs, so caching is platform-wide with no per-target opt-out.
# A target that cannot satisfy this must be redesigned or become a `run`
# target.

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
