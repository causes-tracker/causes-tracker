# TLA+ toolchain

Hermetic Bazel integration for the [TLA+ TLC model checker](https://lamport.azurewebsites.net/tla/tla.html).
Fetches a Temurin JRE and `tla2tools.jar` at build time so `bazel test` runs TLC with no local Java install.

## Using the `tla_test` macro

```starlark
load("//infra/tla:defs.bzl", "tla_test")

tla_test(
    name = "my_spec_tlc_test",
    srcs = [
        "MySpec.tla",
        "MySpecMC.tla",
    ],
    config = "MySpec.cfg",
    entry = "MySpecMC.tla",
)
```

`srcs` lists the `.tla` modules TLC needs (the entry module and anything it `EXTENDS`).
`config` is the `.cfg` file specifying the spec, constants, and invariants.
`entry` is the `.tla` file TLC starts from — typically the MC module that binds concrete values.

## Ad-hoc runs

For iteration outside Bazel, `run_tlc.sh` is a standalone wrapper that caches `tla2tools.jar` in `~/.cache/tla` and exec's TLC directly.
Requires Java locally (`sudo apt install openjdk-21-jre-headless`).
