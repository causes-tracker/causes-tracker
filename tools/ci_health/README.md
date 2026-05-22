# tools/ci_health

CI cache and timing health analyzer for the GitHub Actions `build` workflow.

Subcommands:

- `record` — fetch GitHub Actions step timings and BuildBuddy cache stats for a given workflow run, then emit a typed metrics JSON file.
- `compare` — classify a metrics JSON against a baseline directory of recent successful master runs; exit code reflects the verdict (0 = ok, 1 = regressed). Thresholds are constants in `src/config.rs`.
- `pr-comment` — auto-fetch the rolling-baseline window of master metrics artifacts, classify, and upsert a single regression-report comment on a pull request when the run is materially slower or has worse cache behaviour. Silent on healthy runs.

Run with `bazel run //tools/ci_health -- <subcommand> [args...]`.
