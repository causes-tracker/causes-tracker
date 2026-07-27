# Runs functional_check.sh as a cacheable build action (see that script for
# why it's a `build` target, not a `test`).
#
# GH_TOKEN is read from .buckconfig.local's [renovate] gh_token (gitignored,
# same pattern as [buck2_re_client] http_headers) rather than a declared
# attr, so the secret never lands in a BUCK file. When it's absent, no
# action is created at all — `buck2 build //...` produces a trivial no-op
# output for this target instead of failing, so an unconfigured checkout
# (a fresh clone, CI without the secret wired up) doesn't break the sweep.
def _renovate_functional_check_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("functional_check.stamp")
    token = read_config("renovate", "gh_token")
    if token == None:
        ctx.actions.write(out, "skipped: [renovate] gh_token not set in .buckconfig.local\n")
        return [DefaultInfo(default_output = out)]

    # exec_compatible_with routes this to //platforms:local_only, whose
    # remote_enabled = False makes this a real guarantee: GH_TOKEN's
    # effects (this action's log carries them) never reach a remote
    # executor, because there's no remote executor this platform can
    # dispatch to at all — not because the action asked not to.
    ctx.actions.run(
        cmd_args(
            "/bin/bash",
            ctx.attrs.script,
            ctx.attrs.crane[DefaultInfo].default_outputs[0],
            ctx.attrs.crun_install,
            ctx.attrs.nativelink_install,
            ctx.attrs.nativelink_buck,
            ctx.attrs.crane_buck,
            ctx.attrs.renovate_json,
            ctx.attrs.renovate_image,
            out.as_output(),
        ),
        env = {"GH_TOKEN": token},
        category = "renovate_functional_check",
    )
    return [DefaultInfo(default_output = out)]

renovate_functional_check = rule(
    impl = _renovate_functional_check_impl,
    attrs = {
        "crane": attrs.dep(providers = [DefaultInfo]),
        "crane_buck": attrs.source(),
        "crun_install": attrs.source(doc = "Fixture text only, regex-scanned by Renovate — never executed."),
        "nativelink_buck": attrs.source(),
        "nativelink_install": attrs.source(),
        "renovate_image": attrs.string(doc = "renovate/renovate pinned by digest"),
        "renovate_json": attrs.source(),
        "script": attrs.source(),
    },
)
