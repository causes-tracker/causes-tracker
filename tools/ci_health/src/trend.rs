//! Rolling-window trend analysis. Compares the trailing N-day window
//! against the prior N-day window; opens or updates a single tracking
//! issue on regression, posts a recovery note and closes the issue when
//! things return to normal. Driven by the scheduled
//! `ci-health-trend.yml` workflow.

use crate::config::TrendThresholds;
use crate::metrics::RunMetrics;
use std::collections::BTreeMap;

pub const ISSUE_TITLE: &str = "CI health trend";
pub const ISSUE_MARKER: &str = "<!-- ci-health-trend-bot -->";

/// Median time observed for a single `tools/check.sh` gate across a window.
#[derive(Debug, Clone, PartialEq)]
pub struct GateMedian {
    pub gate: String,
    pub sample_count: usize,
    pub median_seconds: f64,
}

/// Aggregate statistics over a single time window of master CI runs.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowStats {
    pub sample_count: usize,
    pub median_wall_seconds: f64,
    pub p95_wall_seconds: f64,
    pub median_cache_hit_rate: f64,
    pub median_remote_bytes_downloaded: f64,
    /// Per-gate median wall time, sorted alphabetically by gate name.
    /// Populated from `RunMetrics.gate_timings`; gates with `rc != 0`
    /// are excluded so failures don't skew the timing distribution.
    pub gate_medians: Vec<GateMedian>,
}

impl WindowStats {
    pub fn from_runs(runs: &[RunMetrics]) -> Self {
        let mut walls: Vec<f64> = runs.iter().map(|r| r.timings.job_wall_seconds).collect();
        let mut hit_rates: Vec<f64> = runs
            .iter()
            .filter_map(|r| r.bazel.cache_hit_rate())
            .collect();
        let mut bytes: Vec<f64> = runs
            .iter()
            .map(|r| r.bazel.remote_bytes_downloaded as f64)
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
            .map(|(gate, mut samples)| GateMedian {
                gate: gate.to_string(),
                sample_count: samples.len(),
                median_seconds: median(&mut samples).unwrap_or(0.0),
            })
            .collect();
        Self {
            sample_count: runs.len(),
            median_wall_seconds: median(&mut walls).unwrap_or(0.0),
            p95_wall_seconds: percentile(&mut walls.clone(), 0.95).unwrap_or(0.0),
            median_cache_hit_rate: median(&mut hit_rates).unwrap_or(0.0),
            median_remote_bytes_downloaded: median(&mut bytes).unwrap_or(0.0),
            gate_medians,
        }
    }

    /// Find a gate's median in this window by name.
    pub fn gate(&self, name: &str) -> Option<&GateMedian> {
        self.gate_medians.iter().find(|g| g.gate == name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrendVerdict {
    Healthy,
    Regressed { reasons: Vec<String> },
    InsufficientData,
}

pub fn classify(
    trailing: &WindowStats,
    prior: &WindowStats,
    thresholds: &TrendThresholds,
) -> TrendVerdict {
    if trailing.sample_count < 3 || prior.sample_count < 3 {
        return TrendVerdict::InsufficientData;
    }
    let mut reasons = Vec::new();

    if prior.median_wall_seconds > 0.0 {
        let cap = prior.median_wall_seconds * thresholds.median_wall_seconds_ratio;
        if trailing.median_wall_seconds > cap {
            reasons.push(format!(
                "median wall time {:.0}s rose above {:.0}s ({}× prior window median {:.0}s)",
                trailing.median_wall_seconds,
                cap,
                thresholds.median_wall_seconds_ratio,
                prior.median_wall_seconds,
            ));
        }
    }

    let hit_drop = (prior.median_cache_hit_rate - trailing.median_cache_hit_rate) * 100.0;
    if hit_drop > thresholds.median_cache_hit_rate_drop_pp {
        reasons.push(format!(
            "median cache hit rate dropped {:.1}pp ({:.1}% → {:.1}%)",
            hit_drop,
            prior.median_cache_hit_rate * 100.0,
            trailing.median_cache_hit_rate * 100.0,
        ));
    }

    if prior.median_remote_bytes_downloaded > 0.0 {
        let cap = prior.median_remote_bytes_downloaded * thresholds.median_remote_bytes_ratio;
        if trailing.median_remote_bytes_downloaded > cap {
            reasons.push(format!(
                "median remote-cache download bytes {:.0} ballooned past {:.0} ({}× prior {:.0})",
                trailing.median_remote_bytes_downloaded,
                cap,
                thresholds.median_remote_bytes_ratio,
                prior.median_remote_bytes_downloaded,
            ));
        }
    }

    // Per-gate regression: a gate's median seconds rose past
    // `prior * median_wall_seconds_ratio` while both windows have at
    // least 3 samples for that gate. Catches single-gate slowdowns the
    // aggregate `median_wall_seconds` check misses when other gates
    // sped up enough to keep the total flat.
    for trailing_gate in &trailing.gate_medians {
        if trailing_gate.sample_count < 3 {
            continue;
        }
        let Some(prior_gate) = prior.gate(&trailing_gate.gate) else {
            continue;
        };
        if prior_gate.sample_count < 3 || prior_gate.median_seconds <= 0.0 {
            continue;
        }
        let cap = prior_gate.median_seconds * thresholds.median_wall_seconds_ratio;
        if trailing_gate.median_seconds > cap {
            reasons.push(format!(
                "gate `{}` median {:.1}s rose above {:.1}s ({}× prior median {:.1}s)",
                trailing_gate.gate,
                trailing_gate.median_seconds,
                cap,
                thresholds.median_wall_seconds_ratio,
                prior_gate.median_seconds,
            ));
        }
    }

    if reasons.is_empty() {
        TrendVerdict::Healthy
    } else {
        TrendVerdict::Regressed { reasons }
    }
}

pub fn render_issue_body(
    window_days: u32,
    trailing: &WindowStats,
    prior: &WindowStats,
    reasons: &[String],
) -> String {
    let mut out = String::new();
    out.push_str(ISSUE_MARKER);
    out.push('\n');
    out.push_str(&format!(
        "CI health has regressed in the last {window_days} days against the prior {window_days}-day window.\n\n"
    ));
    out.push_str("**Regressions:**\n\n");
    for r in reasons {
        out.push_str(&format!("- {r}\n"));
    }
    out.push('\n');
    out.push_str(&format!(
        "| metric | trailing {window_days}d (n={}) | prior {window_days}d (n={}) |\n",
        trailing.sample_count, prior.sample_count,
    ));
    out.push_str("|---|---|---|\n");
    out.push_str(&format!(
        "| median wall time | {:.0}s | {:.0}s |\n",
        trailing.median_wall_seconds, prior.median_wall_seconds,
    ));
    out.push_str(&format!(
        "| p95 wall time | {:.0}s | {:.0}s |\n",
        trailing.p95_wall_seconds, prior.p95_wall_seconds,
    ));
    out.push_str(&format!(
        "| median cache hit rate | {:.1}% | {:.1}% |\n",
        trailing.median_cache_hit_rate * 100.0,
        prior.median_cache_hit_rate * 100.0,
    ));
    out.push_str(&format!(
        "| median remote bytes down | {:.0} | {:.0} |\n",
        trailing.median_remote_bytes_downloaded, prior.median_remote_bytes_downloaded,
    ));
    if !trailing.gate_medians.is_empty() || !prior.gate_medians.is_empty() {
        out.push_str("\n**Per-gate medians:**\n\n");
        out.push_str("| gate | trailing (n) | prior (n) |\n");
        out.push_str("|---|---|---|\n");
        let mut names: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for g in &trailing.gate_medians {
            names.insert(&g.gate);
        }
        for g in &prior.gate_medians {
            names.insert(&g.gate);
        }
        for name in names {
            let t = trailing.gate(name);
            let p = prior.gate(name);
            let cell = |g: Option<&GateMedian>| match g {
                Some(g) => format!("{:.1}s ({})", g.median_seconds, g.sample_count),
                None => "—".to_string(),
            };
            out.push_str(&format!("| {} | {} | {} |\n", name, cell(t), cell(p)));
        }
    }
    out.push_str("\nThis issue is updated daily; the bot will close it with a recovery note when the trend reverses.\n");
    out
}

pub fn render_recovery_comment(trailing: &WindowStats, prior: &WindowStats) -> String {
    format!(
        "{ISSUE_MARKER}\n\nCI health has recovered. Trailing median wall {:.0}s vs prior {:.0}s; \
         trailing hit rate {:.1}% vs prior {:.1}%. Closing.\n",
        trailing.median_wall_seconds,
        prior.median_wall_seconds,
        trailing.median_cache_hit_rate * 100.0,
        prior.median_cache_hit_rate * 100.0,
    )
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

fn percentile(xs: &mut [f64], p: f64) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((xs.len() as f64 - 1.0) * p).round() as usize;
    Some(xs[idx.min(xs.len() - 1)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{BazelStats, CommitSha, GateTiming, RunId, StepTimings};

    fn run_with_gates(
        wall: f64,
        hits: u64,
        total: u64,
        bytes: u64,
        gates: Vec<GateTiming>,
    ) -> RunMetrics {
        let mut m = run(wall, hits, total, bytes);
        m.gate_timings = gates;
        m
    }

    fn gate(name: &str, seconds: f64) -> GateTiming {
        GateTiming {
            gate: name.into(),
            rc: 0,
            seconds,
        }
    }

    fn run(wall: f64, hits: u64, total: u64, bytes: u64) -> RunMetrics {
        RunMetrics {
            run_id: RunId(0),
            sha: CommitSha("x".into()),
            pr: None,
            branch: "master".into(),
            event: "push".into(),
            timings: StepTimings {
                job_wall_seconds: wall,
                cache_restore_seconds: 0.0,
                cache_save_seconds: 0.0,
                bazel_invocation_seconds: wall,
                other_seconds: 0.0,
            },
            bazel: BazelStats {
                actions_total: total,
                remote_cache_hits: hits,
                cache_misses: total - hits,
                remote_bytes_downloaded: bytes,
                ..Default::default()
            },
            bb_invocation_ids: vec![],
            metrics_collection_seconds: 0.0,
            gate_timings: vec![],
        }
    }

    fn thresholds() -> TrendThresholds {
        TrendThresholds {
            median_wall_seconds_ratio: 1.20,
            median_cache_hit_rate_drop_pp: 10.0,
            median_remote_bytes_ratio: 1.50,
        }
    }

    #[test]
    fn insufficient_when_window_too_small() {
        let stats = WindowStats::from_runs(&[run(180.0, 900, 1000, 1000)]);
        assert_eq!(
            classify(&stats, &stats, &thresholds()),
            TrendVerdict::InsufficientData
        );
    }

    #[test]
    fn healthy_when_windows_match() {
        let runs: Vec<_> = (0..5).map(|_| run(180.0, 900, 1000, 1000)).collect();
        let s = WindowStats::from_runs(&runs);
        assert_eq!(classify(&s, &s, &thresholds()), TrendVerdict::Healthy);
    }

    #[test]
    fn flags_wall_time_regression() {
        let prior: Vec<_> = (0..5).map(|_| run(180.0, 900, 1000, 1000)).collect();
        let trailing: Vec<_> = (0..5).map(|_| run(250.0, 900, 1000, 1000)).collect();
        let v = classify(
            &WindowStats::from_runs(&trailing),
            &WindowStats::from_runs(&prior),
            &thresholds(),
        );
        let TrendVerdict::Regressed { reasons } = v else {
            panic!("expected regression");
        };
        assert!(reasons.iter().any(|r| r.contains("wall time")));
    }

    #[test]
    fn flags_hit_rate_and_bytes_independently() {
        let prior: Vec<_> = (0..5).map(|_| run(180.0, 900, 1000, 1000)).collect();
        // hit rate drops 50pp AND bytes downloaded 5×
        let trailing: Vec<_> = (0..5).map(|_| run(180.0, 400, 1000, 5000)).collect();
        let v = classify(
            &WindowStats::from_runs(&trailing),
            &WindowStats::from_runs(&prior),
            &thresholds(),
        );
        let TrendVerdict::Regressed { reasons } = v else {
            panic!("expected regression");
        };
        assert!(reasons.iter().any(|r| r.contains("cache hit rate")));
        assert!(reasons.iter().any(|r| r.contains("remote-cache download")));
    }

    #[test]
    fn percentile_basic() {
        let mut xs = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0];
        assert_eq!(percentile(&mut xs, 0.95), Some(100.0));
        let mut xs = vec![100.0, 200.0, 300.0];
        assert_eq!(percentile(&mut xs, 0.5), Some(200.0));
    }

    #[test]
    fn issue_body_contains_marker_and_table() {
        let prior: Vec<_> = (0..5).map(|_| run(180.0, 900, 1000, 1000)).collect();
        let trailing: Vec<_> = (0..5).map(|_| run(250.0, 900, 1000, 1000)).collect();
        let t = WindowStats::from_runs(&trailing);
        let p = WindowStats::from_runs(&prior);
        let body = render_issue_body(7, &t, &p, &["wall went up".into()]);
        assert!(body.starts_with(ISSUE_MARKER));
        assert!(body.contains("trailing 7d"));
        assert!(body.contains("prior 7d"));
        assert!(body.contains("wall went up"));
    }

    #[test]
    fn from_runs_aggregates_per_gate_medians() {
        let runs = vec![
            run_with_gates(
                180.0,
                900,
                1000,
                1000,
                vec![gate("format_check", 4.0), gate("bazel_coverage", 40.0)],
            ),
            run_with_gates(
                180.0,
                900,
                1000,
                1000,
                vec![gate("format_check", 5.0), gate("bazel_coverage", 42.0)],
            ),
            run_with_gates(
                180.0,
                900,
                1000,
                1000,
                vec![gate("format_check", 6.0), gate("bazel_coverage", 44.0)],
            ),
        ];
        let s = WindowStats::from_runs(&runs);
        assert_eq!(s.gate("format_check").unwrap().median_seconds, 5.0);
        assert_eq!(s.gate("bazel_coverage").unwrap().median_seconds, 42.0);
        assert_eq!(s.gate("format_check").unwrap().sample_count, 3);
        // alphabetical
        assert_eq!(
            s.gate_medians
                .iter()
                .map(|g| g.gate.as_str())
                .collect::<Vec<_>>(),
            vec!["bazel_coverage", "format_check"]
        );
    }

    #[test]
    fn from_runs_excludes_failed_gate_runs() {
        let mut failed = gate("format_check", 99.0);
        failed.rc = 1;
        let runs = vec![
            run_with_gates(180.0, 900, 1000, 1000, vec![gate("format_check", 4.0)]),
            run_with_gates(180.0, 900, 1000, 1000, vec![gate("format_check", 5.0)]),
            run_with_gates(180.0, 900, 1000, 1000, vec![gate("format_check", 6.0)]),
            run_with_gates(180.0, 900, 1000, 1000, vec![failed]),
        ];
        let s = WindowStats::from_runs(&runs);
        let g = s.gate("format_check").unwrap();
        assert_eq!(g.sample_count, 3);
        assert_eq!(g.median_seconds, 5.0);
    }

    #[test]
    fn flags_per_gate_regression_when_aggregate_is_flat() {
        // Aggregate wall stays at 180s in both windows; format_check
        // doubles while bazel_coverage drops to compensate. Only the
        // per-gate detector should fire.
        let prior: Vec<_> = (0..5)
            .map(|_| {
                run_with_gates(
                    180.0,
                    900,
                    1000,
                    1000,
                    vec![gate("format_check", 4.0), gate("bazel_coverage", 50.0)],
                )
            })
            .collect();
        let trailing: Vec<_> = (0..5)
            .map(|_| {
                run_with_gates(
                    180.0,
                    900,
                    1000,
                    1000,
                    vec![gate("format_check", 12.0), gate("bazel_coverage", 30.0)],
                )
            })
            .collect();
        let v = classify(
            &WindowStats::from_runs(&trailing),
            &WindowStats::from_runs(&prior),
            &thresholds(),
        );
        let TrendVerdict::Regressed { reasons } = v else {
            panic!("expected regression, got {v:?}");
        };
        assert!(reasons.iter().any(|r| r.contains("gate `format_check`")));
        assert!(!reasons.iter().any(|r| r.contains("gate `bazel_coverage`")));
        // aggregate wall didn't trip
        assert!(!reasons.iter().any(|r| r.contains("median wall time")));
    }

    #[test]
    fn per_gate_skipped_when_insufficient_samples() {
        let prior: Vec<_> = (0..5)
            .map(|_| run_with_gates(180.0, 900, 1000, 1000, vec![gate("format_check", 4.0)]))
            .collect();
        // Only 2 trailing samples for format_check (other 3 runs lack
        // the gate entirely); per-gate detector should skip.
        let trailing: Vec<_> = vec![
            run_with_gates(180.0, 900, 1000, 1000, vec![gate("format_check", 99.0)]),
            run_with_gates(180.0, 900, 1000, 1000, vec![gate("format_check", 99.0)]),
            run_with_gates(180.0, 900, 1000, 1000, vec![]),
            run_with_gates(180.0, 900, 1000, 1000, vec![]),
            run_with_gates(180.0, 900, 1000, 1000, vec![]),
        ];
        let v = classify(
            &WindowStats::from_runs(&trailing),
            &WindowStats::from_runs(&prior),
            &thresholds(),
        );
        assert_eq!(v, TrendVerdict::Healthy);
    }

    #[test]
    fn issue_body_renders_per_gate_table() {
        let prior: Vec<_> = (0..5)
            .map(|_| run_with_gates(180.0, 900, 1000, 1000, vec![gate("format_check", 4.0)]))
            .collect();
        let trailing: Vec<_> = (0..5)
            .map(|_| run_with_gates(180.0, 900, 1000, 1000, vec![gate("format_check", 12.0)]))
            .collect();
        let t = WindowStats::from_runs(&trailing);
        let p = WindowStats::from_runs(&prior);
        let body = render_issue_body(7, &t, &p, &["something".into()]);
        assert!(body.contains("Per-gate medians"));
        assert!(body.contains("| format_check |"));
        assert!(body.contains("12.0s (5)"));
        assert!(body.contains("4.0s (5)"));
    }
}
