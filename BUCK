load("//tools:defs.bzl", "shellcheck_package")

export_file(
    name = "renovate_json",
    src = "renovate.json",
    visibility = ["//tools/renovate:"],
)

export_file(
    name = "shellcheckrc",
    # Reference mode keeps the source path, so shellcheck's ancestor walk
    # finds `.shellcheckrc` by name at the action root.
    mode = "reference",
    src = ".shellcheckrc",
    visibility = ["PUBLIC"],
)

shellcheck_package()
