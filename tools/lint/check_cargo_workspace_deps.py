"""Guardrail: workspace members take every dependency from the workspace.

A member Cargo.toml may not pin a dependency version.
Registry dependencies are declared once in the root [workspace.dependencies]
and referenced with `workspace = true`; intra-workspace `path` dependencies
carry no version and are allowed.

Invocation: pass the root Cargo.toml and every member Cargo.toml as
positional arguments (files not named Cargo.toml are ignored).
The root is recognised by its [workspace] table, and every entry in its
[workspace.members] must have its manifest among the arguments, so a new
crate cannot silently escape the check.
"""

from pathlib import Path
import sys
import tomllib

_DEP_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def dep_violation(spec):
    """The reason this dependency spec is not a workspace reference, or None."""
    if isinstance(spec, str):
        return f'inline version "{spec}"'
    if spec.get("workspace") is True:
        return None
    if "version" in spec:
        return f'inline version "{spec["version"]}"'
    if "path" in spec:
        return None
    return "not a workspace reference"


def _table_violations(table_name, table):
    return [
        f"{table_name}: {name} — {reason}"
        for name, spec in sorted(table.items())
        if (reason := dep_violation(spec)) is not None
    ]


def check_manifest(manifest):
    """All dependency violations in one parsed member manifest."""
    violations = []
    for table_name in _DEP_TABLES:
        violations += _table_violations(table_name, manifest.get(table_name, {}))
    for cfg, tables in manifest.get("target", {}).items():
        for table_name in _DEP_TABLES:
            violations += _table_violations(
                f"target.'{cfg}'.{table_name}", tables.get(table_name, {})
            )
    return violations


def main(argv):
    manifests = {}
    for arg in argv:
        path = Path(arg)
        if path.name == "Cargo.toml":
            manifests[path] = tomllib.loads(path.read_text())

    roots = [(p, m) for p, m in manifests.items() if "workspace" in m]
    if len(roots) != 1:
        sys.stderr.write(
            "expected exactly one Cargo.toml with a [workspace] table among the "
            f"arguments, got {len(roots)} — wiring bug\n"
        )
        return 1
    root_path, root = roots[0]

    members = root["workspace"]["members"]
    missing = [m for m in members if root_path.parent / m / "Cargo.toml" not in manifests]
    if missing:
        sys.stderr.write(
            "workspace member manifests missing from the arguments — wiring bug; "
            "add each member's toml_format_srcs (or its exported Cargo.toml) to "
            "cargo_workspace_deps_check in tools/lint/BUILD.bazel:\n"
        )
        for m in missing:
            sys.stderr.write(f"  - {m}\n")
        return 1

    failures = []
    for member in sorted(members):
        path = root_path.parent / member / "Cargo.toml"
        violations = check_manifest(manifests[path])
        if violations:
            failures.append((path, violations))

    if failures:
        sys.stderr.write("Cargo.toml dependencies must be workspace references:\n\n")
        for path, violations in failures:
            sys.stderr.write(f"  {path}\n")
            for violation in violations:
                sys.stderr.write(f"    - {violation}\n")
        sys.stderr.write(
            "\nDeclare each version once under [workspace.dependencies] in the "
            "root Cargo.toml and reference it from the member, e.g.:\n"
            "    console = { workspace = true }\n"
        )
        return 1

    print(f"checked {len(members)} member manifest(s); all deps are workspace refs")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
