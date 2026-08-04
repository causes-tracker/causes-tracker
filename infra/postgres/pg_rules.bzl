"""buck2 rules for the hermetic PostgreSQL fixture.

Shims the pg tree to run through the staged musl runtime and runs the in-action
smoke test.
`:postgres_extracted` (the pg tree-artifact) is a `cached_http_archive`; the
loader, libc, and the libraries the dist does not ship come from
`//third_party/musl:musl_runtime` - see BUCK.
"""

load("//third_party/musl:defs.bzl", "MuslRuntimeInfo")

# Wraps the pg tree so bin/postgres is a /bin/sh shim that runs the real binary
# through the staged musl loader.
# initdb and pg_ctl find_other_exec() `postgres` next to themselves and exec
# it; that child would otherwise resolve its baked-in ELF interpreter
# /lib/ld-musl, forcing the loader into the worker image.
# The shim's own interpreter is /bin/sh, so the loader stays a staged input and
# the fixture runs anywhere the runtime is present, not only a rootfs carrying
# it.
def _pg_shimmed_dist_impl(ctx: AnalysisContext) -> list[Provider]:
    dist = ctx.attrs.dist[DefaultInfo].default_outputs[0]
    out = ctx.actions.declare_output("pg_shimmed", dir = True)
    script = """
set -eu
dist="$1"; out="$2"
mkdir -p "$out"
cp -a "$dist/." "$out/"
mv "$out/bin/postgres" "$out/bin/postgres.real"
cat > "$out/bin/postgres" <<'SHIM'
#!/bin/sh
exec "$PG_MUSL_RUNTIME/ld-musl-x86_64.so.1" --library-path "$LD_LIBRARY_PATH" "$(dirname "$0")/postgres.real" "$@"
SHIM
chmod 0755 "$out/bin/postgres"
"""
    ctx.actions.run(
        cmd_args("/bin/sh", "-c", script, "pg_shimmed_dist", dist, out.as_output()),
        category = "pg_shimmed_dist",
        allow_cache_upload = True,
    )
    return [DefaultInfo(default_output = out)]

pg_shimmed_dist = rule(
    impl = _pg_shimmed_dist_impl,
    attrs = {"dist": attrs.dep(providers = [DefaultInfo])},
)

# Runs a smoke-test script inside the action with the pg tree-artifact and the
# musl runtime as inputs; the script starts PostgreSQL, runs a query, and must
# write `out` on success (mirrors the repo's cached_check convention - buck2 has
# no test-result cache).
def _pg_smoke_test_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("ok.txt")
    ctx.actions.run(
        cmd_args(
            "/bin/sh",
            ctx.attrs.script,
            ctx.attrs.fixture,
            ctx.attrs.pg_dist[DefaultInfo].default_outputs[0],
            ctx.attrs.musl_runtime[MuslRuntimeInfo].runtime,
            out.as_output(),
        ),
        category = "pg_smoke_test",
        allow_cache_upload = True,
    )
    return [DefaultInfo(default_output = out)]

pg_smoke_test = rule(
    impl = _pg_smoke_test_impl,
    attrs = {
        "fixture": attrs.source(doc = "Shell fixture sourced by script for pg_start/pg_stop."),
        "musl_runtime": attrs.dep(providers = [MuslRuntimeInfo]),
        "pg_dist": attrs.dep(providers = [DefaultInfo]),
        "script": attrs.source(),
    },
)
