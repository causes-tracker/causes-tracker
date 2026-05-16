//! Threshold constants for the regression detector.
//! Plain Rust values — there is no runtime override, and any change is already a code change.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrThresholds {
    pub job_wall_seconds_ratio: f64,
    pub cache_hit_rate_drop_pp: f64,
}

/// PR-comment regression thresholds.
/// Trip when this PR's run is materially slower or has materially worse cache behavior than the rolling baseline of recent successful master runs.
pub const PR: PrThresholds = PrThresholds {
    job_wall_seconds_ratio: 1.30,
    cache_hit_rate_drop_pp: 15.0,
};
