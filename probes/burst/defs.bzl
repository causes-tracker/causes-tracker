# One target declaring `actions` remote actions that all read the same
# `inputs` files.
# `action_salt` is in every command, so a new one makes every action cold
# while the inputs stay in the CAS; `input_salt` is in every input, so a new
# one also makes the inputs cold, which is the load that
# tools/nativelink/config.json5's existence_cache answers.
def _burst_impl(ctx: AnalysisContext) -> list[Provider]:
    inputs = [
        ctx.actions.write("in_{}.txt".format(i), "input {} {}\n".format(i, ctx.attrs.input_salt) * 64)
        for i in range(ctx.attrs.inputs)
    ]
    outs = []
    for i in range(ctx.attrs.actions):
        out = ctx.actions.declare_output("out_{}.txt".format(i))
        ctx.actions.run(
            cmd_args("/bin/sh", "-c", 'cat "$@" > "$0" # ' + ctx.attrs.action_salt, out.as_output(), inputs),
            category = "burst",
            identifier = str(i),
        )
        outs.append(out)
    return [DefaultInfo(default_outputs = outs)]

burst = rule(
    impl = _burst_impl,
    attrs = {
        "action_salt": attrs.string(),
        "actions": attrs.int(),
        "input_salt": attrs.string(),
        "inputs": attrs.int(),
    },
)
