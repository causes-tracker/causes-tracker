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
    }
}
