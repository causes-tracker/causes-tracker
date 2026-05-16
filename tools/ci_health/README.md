# tools/ci_health

CI cache and timing health analyzer for the GitHub Actions `build` workflow.

Subcommands:

- `record` — fetch GitHub Actions step timings and BuildBuddy cache stats for a given workflow run, then emit a typed metrics JSON file.
- `compare` — classify a metrics JSON against a baseline directory of recent successful master runs; exit code reflects the verdict (0 = ok, 1 = regressed). Thresholds are constants in `src/config.rs`.
- `pr-comment` — auto-fetch the rolling-baseline window of master metrics artifacts, classify, and upsert a single regression-report comment on a pull request when the run is materially slower or has worse cache behaviour. Silent on healthy runs.
- `query` — developer-facing inspection of CI metrics from a local terminal. Pick one of `--branch X --last N` (recent runs table), `--baseline` (rolling baseline summary), `--run-id N` (single run breakdown). `--json` for scripting.
- `trend` — rolling-window trend analysis: compares the trailing N-day window of master runs against the prior N-day window. Opens or updates a single `CI health trend` issue on regression and closes it on recovery. Driven by the scheduled `ci-health-trend` workflow.

Run with `bazel run //tools/ci_health -- <subcommand> [args...]`.
