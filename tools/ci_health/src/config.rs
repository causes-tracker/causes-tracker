//! Threshold constants for the regression detector.
//! Plain Rust values — there is no runtime override, and any change is already a code change.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrThresholds {
    pub job_wall_seconds_ratio: f64,
    pub cache_hit_rate_drop_pp: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrendThresholds {
    pub median_wall_seconds_ratio: f64,
    pub median_cache_hit_rate_drop_pp: f64,
    pub median_remote_bytes_ratio: f64,
}

/// PR-comment regression thresholds.
/// Trip when this PR's run is materially slower or has materially worse cache behavior than the rolling baseline of recent successful master runs.
pub const PR: PrThresholds = PrThresholds {
    job_wall_seconds_ratio: 1.30,
    cache_hit_rate_drop_pp: 15.0,
};

/// Scheduled trend-job thresholds.
/// Compare the trailing N-day window of master runs against the prior N-day window; trip when the medians drift materially.
/// Remote download bytes ballooning is a sign that something stopped caching server-side, so the byte-ratio check has its own knob.
pub const TREND: TrendThresholds = TrendThresholds {
    median_wall_seconds_ratio: 1.20,
    median_cache_hit_rate_drop_pp: 10.0,
    median_remote_bytes_ratio: 1.50,
};
