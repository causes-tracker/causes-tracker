//! Thin wrapper over octocrab for the data the analyzer needs from
//! GitHub Actions: run metadata + per-step timings.

use crate::metrics::{CommitSha, RunId, StepTimings};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::Octocrab;
use serde::Deserialize;

const OWNER: &str = "causes-tracker";
const REPO: &str = "causes-tracker";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunMetadata {
    pub head_sha: CommitSha,
    pub branch: String,
    pub event: String,
    pub pr: Option<u64>,
}

pub struct GithubClient {
    octo: Octocrab,
}

impl GithubClient {
    pub fn new(token: String) -> Result<Self> {
        Self::with_base_uri(token, "https://api.github.com".into())
    }

    pub fn with_base_uri(token: String, base_uri: String) -> Result<Self> {
        let octo = Octocrab::builder()
            .personal_token(token)
            .base_uri(base_uri)
            .context("building octocrab client")?
            .build()
            .context("octocrab build")?;
        Ok(Self { octo })
    }

    pub async fn run_metadata(&self, run_id: RunId) -> Result<RunMetadata> {
        #[derive(Deserialize)]
        struct Run {
            head_sha: String,
            head_branch: Option<String>,
            event: String,
            pull_requests: Option<Vec<Pr>>,
        }
        #[derive(Deserialize)]
        struct Pr {
            number: u64,
        }
        let path = format!("/repos/{OWNER}/{REPO}/actions/runs/{}", run_id.0);
        let run: Run = self.octo.get(&path, None::<&()>).await.context("get run")?;
        Ok(RunMetadata {
            head_sha: CommitSha(run.head_sha),
            branch: run.head_branch.unwrap_or_default(),
            event: run.event,
            pr: run
                .pull_requests
                .and_then(|v| v.into_iter().next().map(|p| p.number)),
        })
    }

    pub async fn step_timings(&self, run_id: RunId, job_name: &str) -> Result<StepTimings> {
        #[derive(Deserialize)]
        struct JobsResp {
            jobs: Vec<Job>,
        }
        #[derive(Deserialize)]
        struct Job {
            name: String,
            started_at: Option<DateTime<Utc>>,
            completed_at: Option<DateTime<Utc>>,
            steps: Option<Vec<Step>>,
        }
        #[derive(Deserialize)]
        struct Step {
            name: String,
            started_at: Option<DateTime<Utc>>,
            completed_at: Option<DateTime<Utc>>,
        }
        let path = format!("/repos/{OWNER}/{REPO}/actions/runs/{}/jobs", run_id.0);
        let resp: JobsResp = self
            .octo
            .get(&path, None::<&()>)
            .await
            .context("list jobs")?;
        let job = resp
            .jobs
            .into_iter()
            .find(|j| j.name == job_name)
            .with_context(|| format!("job '{job_name}' not found in run {}", run_id.0))?;
        let job_wall = duration(job.started_at, job.completed_at);
        let steps = job.steps.unwrap_or_default();
        let mut t = StepTimings {
            job_wall_seconds: job_wall,
            cache_restore_seconds: 0.0,
            cache_save_seconds: 0.0,
            bazel_invocation_seconds: 0.0,
            other_seconds: 0.0,
        };
        for s in steps {
            let d = duration(s.started_at, s.completed_at);
            match classify_step(&s.name) {
                StepKind::CacheRestore => t.cache_restore_seconds += d,
                StepKind::CacheSave => t.cache_save_seconds += d,
                StepKind::Bazel => t.bazel_invocation_seconds += d,
                StepKind::Other => t.other_seconds += d,
            }
        }
        Ok(t)
    }
}

fn duration(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> f64 {
    match (start, end) {
        (Some(s), Some(e)) => (e - s).num_milliseconds() as f64 / 1000.0,
        _ => 0.0,
    }
}

enum StepKind {
    CacheRestore,
    CacheSave,
    Bazel,
    Other,
}

/// Classification is name-based because the GitHub Actions API does not
/// distinguish action lifecycle phases otherwise. setup-bazel emits its
/// cache restore work under the user-facing step name "Install Bazelisk"
/// and its save work as the "Post …" lifecycle step that GH labels with
/// the same base name. Bazel work is identified by the two step names
/// we control in build.yml.
fn classify_step(name: &str) -> StepKind {
    if name.starts_with("Post ") && name.contains("Bazelisk") {
        StepKind::CacheSave
    } else if name == "Install Bazelisk" {
        StepKind::CacheRestore
    } else if name == "Check formatting" || name.starts_with("Test, build docs") {
        StepKind::Bazel
    } else {
        StepKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// octocrab brings in hyper-rustls; rustls 0.23 refuses to pick a
    /// crypto provider implicitly when both `aws-lc-rs` and `ring` are
    /// present in the dep graph, so we install one explicitly for tests.
    /// install_default returns Err on subsequent calls, which we ignore.
    fn install_test_crypto_provider() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }

    #[test]
    fn step_classification() {
        assert!(matches!(
            classify_step("Install Bazelisk"),
            StepKind::CacheRestore
        ));
        assert!(matches!(
            classify_step("Post Install Bazelisk"),
            StepKind::CacheSave
        ));
        assert!(matches!(classify_step("Check formatting"), StepKind::Bazel));
        assert!(matches!(
            classify_step("Test, build docs, and check coverage"),
            StepKind::Bazel
        ));
        assert!(matches!(classify_step("Set up job"), StepKind::Other));
    }

    #[test]
    fn duration_handles_missing_timestamps() {
        assert_eq!(duration(None, None), 0.0);
        let t = "2026-05-16T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(duration(Some(t), None), 0.0);
    }

    #[tokio::test]
    async fn fetches_run_and_step_timings_from_mock_server() {
        install_test_crypto_provider();
        let mock = wiremock::MockServer::start().await;
        let run_id = 999;
        let run_body = serde_json::json!({
            "head_sha": "deadbeef",
            "head_branch": "feature-x",
            "event": "pull_request",
            "pull_requests": [{"number": 314}],
        });
        let jobs_body = serde_json::json!({
            "jobs": [{
                "name": "build",
                "started_at": "2026-05-16T12:00:00Z",
                "completed_at": "2026-05-16T12:03:00Z",
                "steps": [
                    {"name": "Set up job", "started_at": "2026-05-16T12:00:00Z",
                     "completed_at": "2026-05-16T12:00:02Z"},
                    {"name": "Install Bazelisk", "started_at": "2026-05-16T12:00:02Z",
                     "completed_at": "2026-05-16T12:00:14Z"},
                    {"name": "Test, build docs, and check coverage",
                     "started_at": "2026-05-16T12:00:14Z",
                     "completed_at": "2026-05-16T12:02:50Z"},
                    {"name": "Post Install Bazelisk",
                     "started_at": "2026-05-16T12:02:50Z",
                     "completed_at": "2026-05-16T12:03:00Z"},
                ]
            }]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!(
                "/repos/{OWNER}/{REPO}/actions/runs/{run_id}"
            )))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&run_body))
            .mount(&mock)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!(
                "/repos/{OWNER}/{REPO}/actions/runs/{run_id}/jobs"
            )))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&jobs_body))
            .mount(&mock)
            .await;

        let client = GithubClient::with_base_uri("test-token".into(), mock.uri()).unwrap();
        let meta = client.run_metadata(RunId(run_id)).await.unwrap();
        assert_eq!(meta.head_sha, CommitSha("deadbeef".into()));
        assert_eq!(meta.branch, "feature-x");
        assert_eq!(meta.pr, Some(314));
        let t = client.step_timings(RunId(run_id), "build").await.unwrap();
        assert_eq!(t.job_wall_seconds, 180.0);
        assert_eq!(t.cache_restore_seconds, 12.0);
        assert_eq!(t.cache_save_seconds, 10.0);
        assert_eq!(t.bazel_invocation_seconds, 156.0);
        assert_eq!(t.other_seconds, 2.0);
    }
}
