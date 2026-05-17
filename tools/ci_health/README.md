# tools/ci_health

CI cache and timing health analyzer for the GitHub Actions `build` workflow.

Currently provides one subcommand:

- `record` — fetch GitHub Actions step timings for a given workflow run and emit a typed metrics JSON file.

Run with `bazel run //tools/ci_health -- <subcommand> [args...]`.
