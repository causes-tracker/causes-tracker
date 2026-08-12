use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InvocationId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommitSha(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StepTimings {
    pub job_wall_seconds: f64,
    pub cache_restore_seconds: f64,
    pub cache_save_seconds: f64,
    pub bazel_invocation_seconds: f64,
    pub other_seconds: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BazelStats {
    pub actions_total: u64,
    pub local_cache_hits: u64,
    pub remote_cache_hits: u64,
    pub cache_misses: u64,
    pub remote_bytes_downloaded: u64,
    pub remote_bytes_uploaded: u64,
    pub critical_path_seconds: f64,
}

impl BazelStats {
    pub fn cache_hit_rate(&self) -> Option<f64> {
        if self.actions_total == 0 {
            return None;
        }
        let hits = self.local_cache_hits + self.remote_cache_hits;
        Some(hits as f64 / self.actions_total as f64)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Buck2BuildId(pub String);

/// One buck2 invocation's action counts and cache traffic, parsed from the
/// summary lines buck2 prints at the end of every build.
/// `bytes_*` come from buck2's humanized `Network:` line (absent in
/// local-only mode), so they carry display rounding, not exact counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Buck2Invocation {
    pub build_id: Buck2BuildId,
    pub commands_total: u64,
    pub commands_cached: u64,
    pub commands_remote: u64,
    pub commands_local: u64,
    pub bytes_uploaded: u64,
    pub bytes_downloaded: u64,
}

/// Timings and per-invocation stats for the CI `buck2` job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Buck2Stats {
    pub job_wall_seconds: f64,
    pub build_seconds: f64,
    pub round_trip_seconds: f64,
    pub invocations: Vec<Buck2Invocation>,
}

impl Buck2Stats {
    /// Aggregate cache-hit rate across the job's invocations.
    /// None when no invocation ran a command.
    pub fn cache_hit_rate(&self) -> Option<f64> {
        let total: u64 = self.invocations.iter().map(|i| i.commands_total).sum();
        if total == 0 {
            return None;
        }
        let cached: u64 = self.invocations.iter().map(|i| i.commands_cached).sum();
        Some(cached as f64 / total as f64)
    }
}

/// One entry from the JSONL file `tools/check.sh` writes when
/// `CHECK_TIMING_JSONL` is set.
/// `gate` is the gate name passed to `run_gate` (e.g. `format_check`),
/// `rc` is the gate's POSIX exit code (0-255), `seconds` is wall-clock duration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateTiming {
    pub gate: String,
    pub rc: u8,
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunMetrics {
    pub run_id: RunId,
    pub sha: CommitSha,
    pub pr: Option<u64>,
    pub branch: String,
    pub event: String,
    #[serde(flatten)]
    pub timings: StepTimings,
    pub bazel: BazelStats,
    pub bb_invocation_ids: Vec<InvocationId>,
    /// Wall-clock seconds spent inside the `record` invocation.
    pub metrics_collection_seconds: f64,
    /// Per-gate timings ingested from the JSONL file `tools/check.sh`
    /// writes when `CHECK_TIMING_JSONL` is set.
    /// Empty when `record` was invoked without `--check-timings`.
    #[serde(default)]
    pub gate_timings: Vec<GateTiming>,
    /// Stats for the CI `buck2` job.
    /// `None` in artifacts recorded without buck2 job data.
    #[serde(default)]
    pub buck2: Option<Buck2Stats>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let m = RunMetrics {
            run_id: RunId(42),
            sha: CommitSha("abc".into()),
            pr: Some(7),
            branch: "master".into(),
            event: "push".into(),
            timings: StepTimings {
                job_wall_seconds: 180.0,
                cache_restore_seconds: 12.5,
                cache_save_seconds: 4.0,
                bazel_invocation_seconds: 150.0,
                other_seconds: 13.5,
            },
            bazel: BazelStats {
                actions_total: 1000,
                local_cache_hits: 600,
                remote_cache_hits: 350,
                cache_misses: 50,
                remote_bytes_downloaded: 12345,
                remote_bytes_uploaded: 678,
                critical_path_seconds: 90.0,
            },
            bb_invocation_ids: vec![InvocationId("uuid-1".into())],
            metrics_collection_seconds: 2.75,
            gate_timings: vec![GateTiming {
                gate: "format_check".into(),
                rc: 0,
                seconds: 4.221,
            }],
            buck2: Some(Buck2Stats {
                job_wall_seconds: 24.0,
                build_seconds: 6.0,
                round_trip_seconds: 4.0,
                invocations: vec![Buck2Invocation {
                    build_id: Buck2BuildId("ac446c1c".into()),
                    commands_total: 2,
                    commands_cached: 2,
                    commands_remote: 0,
                    commands_local: 0,
                    bytes_uploaded: 276480,
                    bytes_downloaded: 94371840,
                }],
            }),
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: RunMetrics = serde_json::from_str(&s).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn deserialises_legacy_without_gate_timings() {
        let legacy = r#"{
            "run_id": 1, "sha": "x", "pr": null, "branch": "master", "event": "push",
            "job_wall_seconds": 0.0, "cache_restore_seconds": 0.0, "cache_save_seconds": 0.0,
            "bazel_invocation_seconds": 0.0, "other_seconds": 0.0,
            "bazel": {
                "actions_total": 0, "local_cache_hits": 0, "remote_cache_hits": 0,
                "cache_misses": 0, "remote_bytes_downloaded": 0, "remote_bytes_uploaded": 0,
                "critical_path_seconds": 0.0
            },
            "bb_invocation_ids": [], "metrics_collection_seconds": 0.0
        }"#;
        let m: RunMetrics = serde_json::from_str(legacy).unwrap();
        assert!(m.gate_timings.is_empty());
        assert!(m.buck2.is_none());
    }

    #[test]
    fn buck2_cache_hit_rate_sums_across_invocations() {
        let inv = |total: u64, cached: u64| Buck2Invocation {
            build_id: Buck2BuildId("b".into()),
            commands_total: total,
            commands_cached: cached,
            commands_remote: 0,
            commands_local: total - cached,
            bytes_uploaded: 0,
            bytes_downloaded: 0,
        };
        let stats = |invocations| Buck2Stats {
            job_wall_seconds: 0.0,
            build_seconds: 0.0,
            round_trip_seconds: 0.0,
            invocations,
        };
        assert_eq!(
            stats(vec![inv(100, 90), inv(100, 70)]).cache_hit_rate(),
            Some(0.8)
        );
        assert_eq!(stats(vec![]).cache_hit_rate(), None);
        assert_eq!(stats(vec![inv(0, 0)]).cache_hit_rate(), None);
    }
}
