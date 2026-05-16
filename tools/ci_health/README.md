# tools/ci_health

CI cache and timing health analyzer for the GitHub Actions `build` workflow.

Currently provides one subcommand:

- `record` — fetch GitHub Actions step timings and BuildBuddy cache stats for a given workflow run, then emit a typed metrics JSON file.

Run with `bazel run //tools/ci_health -- <subcommand> [args...]`.
