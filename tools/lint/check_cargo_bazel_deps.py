"""Guardrail: every @crates//:NAME reference in BUILD.bazel must appear as a
key in the sibling Cargo.toml's [dependencies] or [dev-dependencies].

Bazel is the build of record; this check exists only so `cargo` (used for
metadata, IDE integration, and one-off cargo invocations) doesn't spuriously
break when someone adds a Bazel-only crate.  The reverse direction (deps in
Cargo.toml but not BUILD.bazel) is naturally enforced by rustc — Bazel won't
compile a `use foo` without `@crates//:foo`.

Invocation: pass every Cargo.toml and BUILD.bazel from each Rust crate's
package as positional arguments.  The script pairs them by directory.
"""

from pathlib import Path
import re
import sys
import tomllib

CRATE_RE = re.compile(r"@crates//:([A-Za-z0-9_-]+)")


def check(cargo_toml: Path, build_bazel: Path) -> list[str]:
    bazel_crates = set(CRATE_RE.findall(build_bazel.read_text()))
    manifest = tomllib.loads(cargo_toml.read_text())
    deps = set(manifest.get("dependencies", {}).keys())
    deps |= set(manifest.get("dev-dependencies", {}).keys())
    return sorted(bazel_crates - deps)


def main(argv: list[str]) -> int:
    by_dir: dict[Path, dict[str, Path]] = {}
    for arg in argv:
        path = Path(arg)
        if path.name in ("Cargo.toml", "BUILD.bazel"):
            by_dir.setdefault(path.parent, {})[path.name] = path

    failures: list[tuple[Path, list[str]]] = []
    checked = 0
    for directory, files in sorted(by_dir.items()):
        cargo = files.get("Cargo.toml")
        build = files.get("BUILD.bazel")
        if cargo is None or build is None:
            continue
        checked += 1
        missing = check(cargo, build)
        if missing:
            failures.append((cargo, missing))

    if failures:
        sys.stderr.write(
            "Cargo.toml is missing dependencies that BUILD.bazel references via @crates//:\n\n"
        )
        for path, missing in failures:
            sys.stderr.write(f"  {path}\n")
            for m in missing:
                sys.stderr.write(f"    - {m}\n")
        sys.stderr.write(
            "\nAdd each missing crate to [dependencies] or [dev-dependencies] "
            "in the listed Cargo.toml, e.g.:\n"
            "    futures.workspace = true\n"
        )
        return 1

    if checked == 0:
        sys.stderr.write("no crate manifests checked — wiring bug\n")
        return 1

    print(f"checked {checked} crate(s); Cargo.toml ⊇ BUILD.bazel @crates// refs")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
