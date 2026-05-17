# tools/ci_health

CI cache and timing health analyzer for the GitHub Actions `build` workflow.

Subcommands:

- `record` — fetch GitHub Actions step timings and BuildBuddy cache stats for a given workflow run, then emit a typed metrics JSON file.
- `compare` — classify a metrics JSON against a baseline directory of recent successful master runs; exit code reflects the verdict (0 = ok, 1 = regressed). Thresholds are constants in `src/config.rs`.

Run with `bazel run //tools/ci_health -- <subcommand> [args...]`.
