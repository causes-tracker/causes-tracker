# The one platform every target and action uses. The BuildBuddy remote cache
# is enabled iff `[buck2_re_client] engine_address` is configured (via the
# gitignored .buckconfig.local — see .buckconfig.local.template), so a
# checkout without credentials builds purely locally with the same platform
# and the whole graph keeps a single configuration in both modes.
#
# Invariant: everything in `//...` is a cacheable pure function of its
# declared inputs, so caching is platform-wide with no per-target opt-out.
# A target that cannot satisfy this must be redesigned or become a `run`
# target.
_REMOTE_CACHE = read_root_config("buck2_re_client", "engine_address") != None

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
            local_enabled = True,
            remote_enabled = False,
            remote_cache_enabled = _REMOTE_CACHE,
            allow_cache_uploads = _REMOTE_CACHE,
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
