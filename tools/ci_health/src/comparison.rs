//! Regression detection: given a current `RunMetrics` and a baseline built from recent successful master runs, decide whether the current run is healthy.
//! Threshold constants live in [`crate::config`].
//! Detection trips on multiple independent dimensions because a "cache hit" from BuildBuddy is a network fetch and is not necessarily fast — wall time and hit rate can diverge.

use crate::config::PrThresholds;
use crate::metrics::RunMetrics;
use anyhow::{Context, Result};
use std::path::Path;

/// The aggregate of a baseline window: medians of the metrics we use to
/// trip the regression detector. Computed from N successful master runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Baseline {
    pub sample_count: usize,
    pub median_job_wall_seconds: f64,
    pub median_cache_hit_rate: f64,
}

impl Baseline {
    pub fn from_runs(runs: &[RunMetrics]) -> Self {
        let mut walls: Vec<f64> = runs.iter().map(|r| r.timings.job_wall_seconds).collect();
        let mut hit_rates: Vec<f64> = runs
            .iter()
            .filter_map(|r| r.bazel.cache_hit_rate())
            .collect();
        Self {
            sample_count: runs.len(),
            median_job_wall_seconds: median(&mut walls).unwrap_or(0.0),
            median_cache_hit_rate: median(&mut hit_rates).unwrap_or(0.0),
        }
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
    use crate::metrics::{BazelStats, CommitSha, RunId, StepTimings};

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
        }
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
}
