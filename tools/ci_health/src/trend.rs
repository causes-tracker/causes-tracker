//! Rolling-window trend analysis. Compares the trailing N-day window
//! against the prior N-day window; opens or updates a single tracking
//! issue on regression, posts a recovery note and closes the issue when
//! things return to normal. Driven by the scheduled
//! `ci-health-trend.yml` workflow.

use crate::config::TrendThresholds;
use crate::metrics::RunMetrics;

pub const ISSUE_TITLE: &str = "CI health trend";
pub const ISSUE_MARKER: &str = "<!-- ci-health-trend-bot -->";

/// Aggregate statistics over a single time window of master CI runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowStats {
    pub sample_count: usize,
    pub median_wall_seconds: f64,
    pub p95_wall_seconds: f64,
    pub median_cache_hit_rate: f64,
    pub median_remote_bytes_downloaded: f64,
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
        Self {
            sample_count: runs.len(),
            median_wall_seconds: median(&mut walls).unwrap_or(0.0),
            p95_wall_seconds: percentile(&mut walls.clone(), 0.95).unwrap_or(0.0),
            median_cache_hit_rate: median(&mut hit_rates).unwrap_or(0.0),
            median_remote_bytes_downloaded: median(&mut bytes).unwrap_or(0.0),
        }
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
    use crate::metrics::{BazelStats, CommitSha, RunId, StepTimings};

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
}
