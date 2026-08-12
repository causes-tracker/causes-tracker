//! Regression detection: given a current `RunMetrics` and a baseline built from recent successful master runs, decide whether the current run is healthy.
//! Threshold constants live in [`crate::config`].
//! Detection trips on multiple independent dimensions because a "cache hit" from BuildBuddy is a network fetch and is not necessarily fast — wall time and hit rate can diverge.

use crate::config::PrThresholds;
use crate::metrics::RunMetrics;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

/// Median seconds for a single `tools/check.sh` gate across the
/// baseline window. Populated by `Baseline::from_runs` from each
/// baseline run's `gate_timings`; failed-gate runs (rc != 0) are
/// excluded so they don't skew the median.
#[derive(Debug, Clone, PartialEq)]
pub struct BaselineGate {
    pub gate: String,
    pub sample_count: usize,
    pub median_seconds: f64,
}

/// The aggregate of a baseline window: medians of the metrics we use to
/// trip the regression detector. Computed from N successful master runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Baseline {
    pub sample_count: usize,
    pub median_job_wall_seconds: f64,
    pub median_cache_hit_rate: f64,
    /// Median buck2 job wall seconds over baseline runs carrying buck2
    /// data; None when none did.
    pub median_buck2_job_wall_seconds: Option<f64>,
    /// Median buck2 cache-hit rate over baseline runs carrying buck2
    /// data; None when none did.
    pub median_buck2_cache_hit_rate: Option<f64>,
    /// Per-gate median seconds, sorted alphabetically by gate name.
    pub gate_medians: Vec<BaselineGate>,
}

impl Baseline {
    pub fn from_runs(runs: &[RunMetrics]) -> Self {
        let mut walls: Vec<f64> = runs.iter().map(|r| r.timings.job_wall_seconds).collect();
        let mut hit_rates: Vec<f64> = runs
            .iter()
            .filter_map(|r| r.bazel.cache_hit_rate())
            .collect();
        let mut buck2_walls: Vec<f64> = runs
            .iter()
            .filter_map(|r| r.buck2.as_ref().map(|b| b.job_wall_seconds))
            .collect();
        let mut buck2_hit_rates: Vec<f64> = runs
            .iter()
            .filter_map(|r| r.buck2.as_ref().and_then(|b| b.cache_hit_rate()))
            .collect();
        let mut per_gate: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        for run in runs {
            for gt in &run.gate_timings {
                if gt.rc != 0 {
                    continue;
                }
                per_gate
                    .entry(gt.gate.as_str())
                    .or_default()
                    .push(gt.seconds);
            }
        }
        let gate_medians = per_gate
            .into_iter()
            .map(|(gate, mut samples)| BaselineGate {
                gate: gate.to_string(),
                sample_count: samples.len(),
                median_seconds: median(&mut samples).unwrap_or(0.0),
            })
            .collect();
        Self {
            sample_count: runs.len(),
            median_job_wall_seconds: median(&mut walls).unwrap_or(0.0),
            median_cache_hit_rate: median(&mut hit_rates).unwrap_or(0.0),
            median_buck2_job_wall_seconds: median(&mut buck2_walls),
            median_buck2_cache_hit_rate: median(&mut buck2_hit_rates),
            gate_medians,
        }
    }

    pub fn gate(&self, name: &str) -> Option<&BaselineGate> {
        self.gate_medians.iter().find(|g| g.gate == name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Ok,
    Regressed { reasons: Vec<String> },
}

pub fn classify(current: &RunMetrics, baseline: &Baseline, thresholds: PrThresholds) -> Verdict {
    let mut reasons = Vec::new();

    // Wall time: trip if current > baseline_median * ratio. Guard
    // against a zero/missing baseline (which can happen with very fresh
    // repos) by requiring a positive median before tripping.
    if baseline.median_job_wall_seconds > 0.0 {
        let cap = baseline.median_job_wall_seconds * thresholds.job_wall_seconds_ratio;
        if current.timings.job_wall_seconds > cap {
            reasons.push(format!(
                "job wall time {:.0}s exceeds {:.0}s ({}× baseline median {:.0}s)",
                current.timings.job_wall_seconds,
                cap,
                thresholds.job_wall_seconds_ratio,
                baseline.median_job_wall_seconds,
            ));
        }
    }

    // Cache hit rate: trip if current drops more than N percentage
    // points below the baseline median. Only meaningful when the
    // current run actually executed bazel actions.
    if let Some(cur_rate) = current.bazel.cache_hit_rate() {
        let drop_pp = (baseline.median_cache_hit_rate - cur_rate) * 100.0;
        if drop_pp > thresholds.cache_hit_rate_drop_pp {
            reasons.push(format!(
                "cache hit rate {:.1}% dropped {:.1}pp below baseline median {:.1}%",
                cur_rate * 100.0,
                drop_pp,
                baseline.median_cache_hit_rate * 100.0,
            ));
        }
    }

    // buck2 job wall time: same ratio as the build job.
    if let (Some(cur), Some(base)) = (
        current.buck2.as_ref(),
        baseline.median_buck2_job_wall_seconds,
    ) {
        if base > 0.0 {
            let cap = base * thresholds.job_wall_seconds_ratio;
            if cur.job_wall_seconds > cap {
                reasons.push(format!(
                    "buck2 job wall time {:.0}s exceeds {:.0}s ({}× baseline median {:.0}s)",
                    cur.job_wall_seconds, cap, thresholds.job_wall_seconds_ratio, base,
                ));
            }
        }
    }

    // buck2 cache hit rate: same drop threshold as bazel.
    if let (Some(cur_rate), Some(base_rate)) = (
        current.buck2.as_ref().and_then(|b| b.cache_hit_rate()),
        baseline.median_buck2_cache_hit_rate,
    ) {
        let drop_pp = (base_rate - cur_rate) * 100.0;
        if drop_pp > thresholds.cache_hit_rate_drop_pp {
            reasons.push(format!(
                "buck2 cache hit rate {:.1}% dropped {:.1}pp below baseline median {:.1}%",
                cur_rate * 100.0,
                drop_pp,
                base_rate * 100.0,
            ));
        }
    }

    // Per-gate regression: trip if a successful gate in the current run
    // took more than baseline.median × ratio, when the baseline has at
    // least 3 samples for that gate. Catches single-gate slowdowns the
    // aggregate `job_wall_seconds` check misses.
    for cur_gate in &current.gate_timings {
        if cur_gate.rc != 0 {
            continue;
        }
        let Some(baseline_gate) = baseline.gate(&cur_gate.gate) else {
            continue;
        };
        if baseline_gate.sample_count < 3 || baseline_gate.median_seconds <= 0.0 {
            continue;
        }
        let cap = baseline_gate.median_seconds * thresholds.job_wall_seconds_ratio;
        if cur_gate.seconds > cap {
            reasons.push(format!(
                "gate `{}` took {:.1}s, above {:.1}s ({}× baseline median {:.1}s)",
                cur_gate.gate,
                cur_gate.seconds,
                cap,
                thresholds.job_wall_seconds_ratio,
                baseline_gate.median_seconds,
            ));
        }
    }

    if reasons.is_empty() {
        Verdict::Ok
    } else {
        Verdict::Regressed { reasons }
    }
}

/// Marker embedded in the rendered PR comment so the pr-comment
/// subcommand can find and update its own previous comments instead of
/// stacking.
pub const COMMENT_MARKER: &str = "<!-- ci-health-bot -->";

pub fn render_pr_comment(current: &RunMetrics, baseline: &Baseline, verdict: &Verdict) -> String {
    let reasons = match verdict {
        Verdict::Ok => return String::new(),
        Verdict::Regressed { reasons } => reasons,
    };
    let hit_rate = current
        .bazel
        .cache_hit_rate()
        .map(|r| format!("{:.1}%", r * 100.0))
        .unwrap_or_else(|| "n/a".into());
    let mut out = String::new();
    out.push_str(COMMENT_MARKER);
    out.push('\n');
    out.push_str("**CI health regression detected**\n\n");
    for r in reasons {
        out.push_str(&format!("- {r}\n"));
    }
    out.push_str("\n<details><summary>This run vs baseline</summary>\n\n");
    out.push_str(&format!(
        "| metric | this run | baseline median (n={}) |\n",
        baseline.sample_count
    ));
    out.push_str("|---|---|---|\n");
    out.push_str(&format!(
        "| job wall time | {:.0}s | {:.0}s |\n",
        current.timings.job_wall_seconds, baseline.median_job_wall_seconds
    ));
    out.push_str(&format!(
        "| cache restore | {:.0}s | — |\n",
        current.timings.cache_restore_seconds
    ));
    out.push_str(&format!(
        "| cache save | {:.0}s | — |\n",
        current.timings.cache_save_seconds
    ));
    out.push_str(&format!(
        "| bazel | {:.0}s | — |\n",
        current.timings.bazel_invocation_seconds
    ));
    out.push_str(&format!(
        "| bazel cache hit rate | {} | {:.1}% |\n",
        hit_rate,
        baseline.median_cache_hit_rate * 100.0
    ));
    out.push_str(&format!(
        "| remote bytes downloaded | {} | — |\n",
        current.bazel.remote_bytes_downloaded
    ));
    if let Some(b) = &current.buck2 {
        let base_wall = baseline
            .median_buck2_job_wall_seconds
            .map(|s| format!("{s:.0}s"))
            .unwrap_or_else(|| "—".to_string());
        out.push_str(&format!(
            "| buck2 job wall time | {:.0}s | {} |\n",
            b.job_wall_seconds, base_wall
        ));
        out.push_str(&format!("| buck2 build | {:.0}s | — |\n", b.build_seconds));
        out.push_str(&format!(
            "| buck2 round trip | {:.0}s | — |\n",
            b.round_trip_seconds
        ));
        let cur_hr = b
            .cache_hit_rate()
            .map(|r| format!("{:.1}%", r * 100.0))
            .unwrap_or_else(|| "n/a".to_string());
        let base_hr = baseline
            .median_buck2_cache_hit_rate
            .map(|r| format!("{:.1}%", r * 100.0))
            .unwrap_or_else(|| "—".to_string());
        out.push_str(&format!(
            "| buck2 cache hit rate | {cur_hr} | {base_hr} |\n"
        ));
    }
    if !current.gate_timings.is_empty() || !baseline.gate_medians.is_empty() {
        out.push_str("\n**Per-gate timings:**\n\n");
        out.push_str("| gate | this run | baseline median (n) |\n");
        out.push_str("|---|---|---|\n");
        let mut names: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for g in &current.gate_timings {
            names.insert(&g.gate);
        }
        for g in &baseline.gate_medians {
            names.insert(&g.gate);
        }
        for name in names {
            let cur = current
                .gate_timings
                .iter()
                .find(|g| g.gate == name)
                .map(|g| format!("{:.1}s", g.seconds))
                .unwrap_or_else(|| "—".to_string());
            let base = baseline
                .gate(name)
                .map(|g| format!("{:.1}s ({})", g.median_seconds, g.sample_count))
                .unwrap_or_else(|| "—".to_string());
            out.push_str(&format!("| {name} | {cur} | {base} |\n"));
        }
    }
    out.push_str("\n</details>\n");
    out
}

pub fn load_baseline_dir(dir: &Path) -> Result<Vec<RunMetrics>> {
    let mut runs = Vec::new();
    let entries = std::fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let run: RunMetrics =
            serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        runs.push(run);
    }
    Ok(runs)
}

fn median(xs: &mut [f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n % 2 == 1 {
        Some(xs[n / 2])
    } else {
        Some((xs[n / 2 - 1] + xs[n / 2]) / 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{
        BazelStats, Buck2BuildId, Buck2Invocation, Buck2Stats, CommitSha, GateTiming, RunId,
        StepTimings,
    };

    fn gate(name: &str, seconds: f64) -> GateTiming {
        GateTiming {
            gate: name.into(),
            rc: 0,
            seconds,
        }
    }

    fn run_with_gates(wall: f64, hits: u64, total: u64, gates: Vec<GateTiming>) -> RunMetrics {
        let mut m = run(wall, hits, total);
        m.gate_timings = gates;
        m
    }

    fn run(wall: f64, hits: u64, total: u64) -> RunMetrics {
        RunMetrics {
            run_id: RunId(1),
            sha: CommitSha("x".into()),
            pr: None,
            branch: "master".into(),
            event: "push".into(),
            timings: StepTimings {
                job_wall_seconds: wall,
                cache_restore_seconds: 10.0,
                cache_save_seconds: 5.0,
                bazel_invocation_seconds: wall - 30.0,
                other_seconds: 15.0,
            },
            bazel: BazelStats {
                actions_total: total,
                local_cache_hits: 0,
                remote_cache_hits: hits,
                cache_misses: total - hits,
                ..Default::default()
            },
            bb_invocation_ids: vec![],
            metrics_collection_seconds: 0.0,
            gate_timings: vec![],
            buck2: None,
        }
    }

    fn buck2_stats(wall: f64, cached: u64, total: u64) -> Buck2Stats {
        Buck2Stats {
            job_wall_seconds: wall,
            build_seconds: wall * 0.3,
            round_trip_seconds: wall * 0.2,
            invocations: vec![Buck2Invocation {
                build_id: Buck2BuildId("bid".into()),
                commands_total: total,
                commands_cached: cached,
                commands_remote: 0,
                commands_local: total - cached,
                bytes_uploaded: 0,
                bytes_downloaded: 0,
            }],
        }
    }

    fn run_with_buck2(wall: f64, b: Buck2Stats) -> RunMetrics {
        let mut m = run(wall, 900, 1000);
        m.buck2 = Some(b);
        m
    }

    fn pr_thresholds() -> PrThresholds {
        PrThresholds {
            job_wall_seconds_ratio: 1.30,
            cache_hit_rate_drop_pp: 15.0,
        }
    }

    #[test]
    fn median_basic() {
        let mut xs = vec![3.0, 1.0, 4.0, 1.0, 5.0];
        assert_eq!(median(&mut xs), Some(3.0));
        let mut xs = vec![1.0, 2.0, 3.0, 4.0];
        assert_eq!(median(&mut xs), Some(2.5));
        let mut xs: Vec<f64> = vec![];
        assert_eq!(median(&mut xs), None);
    }

    #[test]
    fn ok_when_within_thresholds() {
        let baseline = Baseline::from_runs(&[
            run(180.0, 900, 1000),
            run(190.0, 950, 1000),
            run(170.0, 920, 1000),
        ]);
        let current = run(200.0, 920, 1000);
        assert_eq!(classify(&current, &baseline, pr_thresholds()), Verdict::Ok);
    }

    #[test]
    fn flags_wall_time_regression() {
        let baseline = Baseline::from_runs(&[run(180.0, 900, 1000)]);
        let current = run(300.0, 900, 1000); // 1.66× baseline
        let v = classify(&current, &baseline, pr_thresholds());
        let Verdict::Regressed { reasons } = v else {
            panic!("expected regression");
        };
        assert!(reasons.iter().any(|r| r.contains("wall time")));
    }

    #[test]
    fn flags_cache_hit_drop() {
        let baseline = Baseline::from_runs(&[run(180.0, 900, 1000)]); // 90%
        let current = run(180.0, 700, 1000); // 70% — 20pp drop
        let v = classify(&current, &baseline, pr_thresholds());
        let Verdict::Regressed { reasons } = v else {
            panic!("expected regression");
        };
        assert!(reasons.iter().any(|r| r.contains("cache hit rate")));
    }

    #[test]
    fn rendered_comment_contains_marker_and_table() {
        let baseline = Baseline::from_runs(&[run(180.0, 900, 1000)]);
        let current = run(300.0, 600, 1000);
        let v = classify(&current, &baseline, pr_thresholds());
        let md = render_pr_comment(&current, &baseline, &v);
        assert!(md.starts_with(COMMENT_MARKER));
        assert!(md.contains("CI health regression detected"));
        assert!(md.contains("this run"));
    }

    #[test]
    fn rendered_comment_empty_when_ok() {
        let baseline = Baseline::from_runs(&[run(180.0, 900, 1000)]);
        let current = run(190.0, 920, 1000);
        let v = classify(&current, &baseline, pr_thresholds());
        assert_eq!(render_pr_comment(&current, &baseline, &v), "");
    }

    #[test]
    fn baseline_aggregates_per_gate_medians() {
        let baseline = Baseline::from_runs(&[
            run_with_gates(180.0, 900, 1000, vec![gate("format_check", 4.0)]),
            run_with_gates(180.0, 900, 1000, vec![gate("format_check", 5.0)]),
            run_with_gates(180.0, 900, 1000, vec![gate("format_check", 6.0)]),
        ]);
        let g = baseline.gate("format_check").unwrap();
        assert_eq!(g.sample_count, 3);
        assert_eq!(g.median_seconds, 5.0);
    }

    #[test]
    fn flags_per_gate_regression_when_aggregate_is_flat() {
        // Same job wall (180s) as baseline → no wall regression.
        // But format_check tripled from 4s to 14s → per-gate regression.
        let baseline = Baseline::from_runs(&[
            run_with_gates(180.0, 900, 1000, vec![gate("format_check", 4.0)]),
            run_with_gates(180.0, 900, 1000, vec![gate("format_check", 4.0)]),
            run_with_gates(180.0, 900, 1000, vec![gate("format_check", 4.0)]),
        ]);
        let current = run_with_gates(180.0, 900, 1000, vec![gate("format_check", 14.0)]);
        let v = classify(&current, &baseline, pr_thresholds());
        let Verdict::Regressed { reasons } = v else {
            panic!("expected regression, got {v:?}");
        };
        assert!(reasons.iter().any(|r| r.contains("gate `format_check`")));
        assert!(!reasons.iter().any(|r| r.contains("wall time")));
    }

    #[test]
    fn per_gate_ignored_when_baseline_lacks_samples() {
        // Baseline has 2 samples for format_check (need 3). Skipped.
        let baseline = Baseline::from_runs(&[
            run_with_gates(180.0, 900, 1000, vec![gate("format_check", 4.0)]),
            run_with_gates(180.0, 900, 1000, vec![gate("format_check", 4.0)]),
            run_with_gates(180.0, 900, 1000, vec![]),
        ]);
        let current = run_with_gates(180.0, 900, 1000, vec![gate("format_check", 99.0)]);
        assert_eq!(classify(&current, &baseline, pr_thresholds()), Verdict::Ok);
    }

    #[test]
    fn rendered_comment_includes_per_gate_table() {
        let baseline = Baseline::from_runs(&[
            run_with_gates(180.0, 900, 1000, vec![gate("format_check", 4.0)]),
            run_with_gates(180.0, 900, 1000, vec![gate("format_check", 4.0)]),
            run_with_gates(180.0, 900, 1000, vec![gate("format_check", 4.0)]),
        ]);
        let current = run_with_gates(180.0, 900, 1000, vec![gate("format_check", 14.0)]);
        let v = classify(&current, &baseline, pr_thresholds());
        let md = render_pr_comment(&current, &baseline, &v);
        assert!(md.contains("Per-gate timings"));
        assert!(md.contains("| format_check | 14.0s | 4.0s (3) |"));
    }

    #[test]
    fn flags_buck2_wall_regression() {
        // bazel wall matches baseline, so only the buck2 job trips.
        let baseline = Baseline::from_runs(&[
            run_with_buck2(180.0, buck2_stats(20.0, 100, 100)),
            run_with_buck2(180.0, buck2_stats(20.0, 100, 100)),
            run_with_buck2(180.0, buck2_stats(20.0, 100, 100)),
        ]);
        let current = run_with_buck2(180.0, buck2_stats(40.0, 100, 100)); // 2× baseline
        let v = classify(&current, &baseline, pr_thresholds());
        let Verdict::Regressed { reasons } = v else {
            panic!("expected regression, got {v:?}");
        };
        assert!(reasons.iter().any(|r| r.contains("buck2 job wall time")));
        assert!(!reasons.iter().any(|r| r.contains("gate")));
    }

    #[test]
    fn flags_buck2_cache_hit_drop() {
        let baseline = Baseline::from_runs(&[
            run_with_buck2(180.0, buck2_stats(20.0, 90, 100)), // 90%
            run_with_buck2(180.0, buck2_stats(20.0, 90, 100)),
            run_with_buck2(180.0, buck2_stats(20.0, 90, 100)),
        ]);
        let current = run_with_buck2(180.0, buck2_stats(20.0, 70, 100)); // 70% — 20pp drop
        let v = classify(&current, &baseline, pr_thresholds());
        let Verdict::Regressed { reasons } = v else {
            panic!("expected regression, got {v:?}");
        };
        assert!(reasons.iter().any(|r| r.contains("buck2 cache hit rate")));
    }

    #[test]
    fn rendered_comment_includes_buck2_rows() {
        let baseline = Baseline::from_runs(&[run_with_buck2(180.0, buck2_stats(20.0, 90, 100))]);
        let current = run_with_buck2(300.0, buck2_stats(20.0, 90, 100));
        let v = classify(&current, &baseline, pr_thresholds());
        let md = render_pr_comment(&current, &baseline, &v);
        assert!(md.contains("| buck2 job wall time | 20s | 20s |"), "{md}");
        assert!(md.contains("buck2 cache hit rate"), "{md}");
    }

    #[test]
    fn baseline_aggregates_buck2_medians() {
        let baseline = Baseline::from_runs(&[
            run_with_buck2(180.0, buck2_stats(20.0, 90, 100)),
            run_with_buck2(180.0, buck2_stats(30.0, 90, 100)),
            run_with_buck2(180.0, buck2_stats(40.0, 90, 100)),
        ]);
        assert_eq!(baseline.median_buck2_job_wall_seconds, Some(30.0));
        assert_eq!(baseline.median_buck2_cache_hit_rate, Some(0.9));
    }

    #[test]
    fn buck2_medians_none_when_no_run_carries_buck2() {
        let baseline = Baseline::from_runs(&[run(180.0, 900, 1000)]);
        assert_eq!(baseline.median_buck2_job_wall_seconds, None);
        assert_eq!(baseline.median_buck2_cache_hit_rate, None);
    }
}
