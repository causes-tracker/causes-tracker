use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(pub u64);

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunMetrics {
    pub run_id: RunId,
    pub sha: CommitSha,
    pub pr: Option<u64>,
    pub branch: String,
    pub event: String,
    #[serde(flatten)]
    pub timings: StepTimings,
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
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: RunMetrics = serde_json::from_str(&s).unwrap();
        assert_eq!(m, back);
    }
}
