# An action that reads a repo file it never declared as an input; under
# input staging the file is absent from the exec root, so the build MUST
# fail.
# This target exists to validate that the reference configuration
# exercised in CI is isolating actions; it makes no claim about other
# environments or other sandbox dimensions.
def _undeclared_read_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("out.txt")
    ctx.actions.run(
        cmd_args("/bin/sh", "-c", 'cat README.md > "$1"', "probe", out.as_output()),
        category = "undeclared_read",
    )
    return [DefaultInfo(default_output = out)]

undeclared_read = rule(impl = _undeclared_read_impl, attrs = {})
