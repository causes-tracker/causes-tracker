//! Thin wrapper over octocrab's typed `WorkflowsHandler` for the data the analyzer needs from GitHub Actions: run metadata, per-step timings, and the BuildBuddy invocation IDs that bazel printed into the job log.

use crate::metrics::{CommitSha, InvocationId, RunId, StepTimings};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::Octocrab;

pub const OWNER: &str = "causes-tracker";
pub const REPO: &str = "causes-tracker";

/// Read a GitHub PAT from `GH_TOKEN` (what `gh auth token` writes) or `GITHUB_TOKEN` (what GH Actions injects).
/// `subcommand` is folded into the error message so the user sees which call needed the credential.
pub fn token_from_env(subcommand: &str) -> Result<String> {
    std::env::var("GH_TOKEN")
        .or_else(|_| std::env::var("GITHUB_TOKEN"))
        .with_context(|| {
            format!(
                "GH_TOKEN or GITHUB_TOKEN must be set for the {subcommand} subcommand (try `export GH_TOKEN=$(gh auth token)`)"
            )
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunMetadata {
    pub head_sha: CommitSha,
    pub branch: String,
    pub event: String,
}

pub struct GithubClient {
    octo: Octocrab,
    http: reqwest::Client,
    token: String,
    base_uri: String,
}

impl GithubClient {
    pub fn new(token: String) -> Result<Self> {
        Self::with_base_uri(token, "https://api.github.com".into())
    }

    pub fn with_base_uri(token: String, base_uri: String) -> Result<Self> {
        let octo = Octocrab::builder()
            .personal_token(token.clone())
            .base_uri(base_uri.clone())
            .context("building octocrab client")?
            .build()
            .context("octocrab build")?;
        let http = reqwest::Client::builder()
            .user_agent("ci_health/0.1")
            .build()
            .context("building reqwest client")?;
        Ok(Self {
            octo,
            http,
            token,
            base_uri,
        })
    }

    pub async fn run_metadata(&self, run_id: RunId) -> Result<RunMetadata> {
        let run = self
            .octo
            .workflows(OWNER, REPO)
            .get(octocrab::models::RunId(run_id.0))
            .await
            .with_context(|| format!("get workflow run {}", run_id.0))?;
        Ok(RunMetadata {
            head_sha: CommitSha(run.head_sha),
            branch: run.head_branch,
            event: run.event,
        })
    }

    /// BuildBuddy invocation IDs that this job's bazel commands streamed to.
    /// Pulled by fetching the job's GH-captured stdout log and regexing out every `app.buildbuddy.io/invocation/<uuid>` URL bazel prints when `--bes_results_url` is configured.
    /// This avoids `SearchInvocation` entirely, which doesn't support filtering by GitHub run id and joins by `commit_sha` would miss `pull_request` runs (those use a synthesized merge commit different from the run's `head_sha`).
    pub async fn bazel_invocation_ids(
        &self,
        run_id: RunId,
        job_name: &str,
    ) -> Result<Vec<InvocationId>> {
        let log = self.job_log(run_id, job_name).await?;
        Ok(extract_invocation_ids(&log))
    }

    async fn find_job_id(&self, run_id: RunId, job_name: &str) -> Result<u64> {
        let page = self
            .octo
            .workflows(OWNER, REPO)
            .list_jobs(octocrab::models::RunId(run_id.0))
            .per_page(100)
            .send()
            .await
            .with_context(|| format!("list jobs for run {}", run_id.0))?;
        let job = page
            .items
            .into_iter()
            .find(|j| j.name == job_name)
            .with_context(|| format!("job '{job_name}' not found in run {}", run_id.0))?;
        Ok(job.id.0)
    }

    async fn fetch_job_log(&self, job_id: u64) -> Result<String> {
        let url = format!(
            "{}/repos/{OWNER}/{REPO}/actions/jobs/{job_id}/logs",
            self.base_uri.trim_end_matches('/')
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github.raw")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("GET {url} status"))?;
        resp.text().await.context("read job log body")
    }

    /// The GH-captured stdout log of the named job.
    pub async fn job_log(&self, run_id: RunId, job_name: &str) -> Result<String> {
        let job_id = self.find_job_id(run_id, job_name).await?;
        self.fetch_job_log(job_id).await
    }

    /// Wall-clock duration of the named job plus each of its steps,
    /// in workflow order.
    pub async fn job_steps(&self, run_id: RunId, job_name: &str) -> Result<JobSteps> {
        let page = self
            .octo
            .workflows(OWNER, REPO)
            .list_jobs(octocrab::models::RunId(run_id.0))
            .per_page(100)
            .send()
            .await
            .with_context(|| format!("list jobs for run {}", run_id.0))?;
        let job = page
            .items
            .into_iter()
            .find(|j| j.name == job_name)
            .with_context(|| format!("job '{job_name}' not found in run {}", run_id.0))?;
        Ok(JobSteps {
            job_wall_seconds: duration(Some(job.started_at), job.completed_at),
            steps: job
                .steps
                .into_iter()
                .map(|s| {
                    let d = duration(s.started_at, s.completed_at);
                    (s.name, d)
                })
                .collect(),
        })
    }

    pub async fn step_timings(&self, run_id: RunId, job_name: &str) -> Result<StepTimings> {
        let js = self.job_steps(run_id, job_name).await?;
        let mut t = StepTimings {
            job_wall_seconds: js.job_wall_seconds,
            cache_restore_seconds: 0.0,
            cache_save_seconds: 0.0,
            bazel_invocation_seconds: 0.0,
            other_seconds: 0.0,
        };
        for (name, d) in js.steps {
            match classify_step(&name) {
                StepKind::CacheRestore => t.cache_restore_seconds += d,
                StepKind::CacheSave => t.cache_save_seconds += d,
                StepKind::Bazel => t.bazel_invocation_seconds += d,
                StepKind::Other => t.other_seconds += d,
            }
        }
        Ok(t)
    }
}

/// Wall-clock duration of one job and its steps, by step name.
pub struct JobSteps {
    pub job_wall_seconds: f64,
    pub steps: Vec<(String, f64)>,
}

fn duration(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> f64 {
    match (start, end) {
        (Some(s), Some(e)) => (e - s).num_milliseconds() as f64 / 1000.0,
        _ => 0.0,
    }
}

/// Pulls every `app.buildbuddy.io/invocation/<uuid>` substring out of `log`, deduping while preserving the order of first occurrence.
/// Bazel prints these for every `--bes_results_url`-configured invocation, so the ordered set IS the invocation list for the job.
/// UUIDs are 36 chars of `[0-9a-f-]`; the matcher accepts that pattern.
fn extract_invocation_ids(log: &str) -> Vec<InvocationId> {
    const NEEDLE: &str = "app.buildbuddy.io/invocation/";
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (start, _) in log.match_indices(NEEDLE) {
        let after = &log[start + NEEDLE.len()..];
        let end = after
            .find(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
            .unwrap_or(after.len());
        let id = &after[..end];
        if id.len() >= 32 && seen.insert(id.to_owned()) {
            out.push(InvocationId(id.to_owned()));
        }
    }
    out
}

enum StepKind {
    CacheRestore,
    CacheSave,
    Bazel,
    Other,
}

/// Classification is name-based because the GitHub Actions API does not distinguish action lifecycle phases otherwise.
/// setup-bazel emits its cache restore work under the user-facing step name "Install Bazelisk" and its save work as the "Post …" lifecycle step that GH labels with the same base name.
/// Bazel work is identified by the two step names we control in build.yml.
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

    /// Real-shape log snippet (captured from a GH job log on 2026-05-17): two `bazel run` invocations each print one BuildBuddy URL to stdout, surrounded by other log noise that must not match.
    /// The extractor must dedupe and preserve invocation order.
    #[test]
    fn extracts_invocation_ids_from_log_text() {
        let log = "\
2026-05-17T18:42:30.123Z INFO: Streaming build results to: https://app.buildbuddy.io/invocation/913e785e-ef99-4211-a770-2479e0d1845e
2026-05-17T18:42:30.456Z INFO: Some other line that mentions invocation/not-a-uuid
2026-05-17T18:42:32.789Z Building target //...
2026-05-17T18:42:40.111Z INFO: Streaming build results to: https://app.buildbuddy.io/invocation/a8523067-f34f-4d9b-9f04-b26b1f9ff43e
2026-05-17T18:42:50.222Z INFO: Streaming build results to: https://app.buildbuddy.io/invocation/913e785e-ef99-4211-a770-2479e0d1845e
";
        let got = extract_invocation_ids(log);
        assert_eq!(
            got,
            vec![
                InvocationId("913e785e-ef99-4211-a770-2479e0d1845e".into()),
                InvocationId("a8523067-f34f-4d9b-9f04-b26b1f9ff43e".into()),
            ]
        );
    }

    /// Drives `run_metadata` and `step_timings` through the typed octocrab
    /// `WorkflowsHandler` against a wiremock-backed GitHub API.
    /// The fixture bodies mirror the real `GET /actions/runs/{id}` and `.../jobs` shapes that octocrab's `models::workflows::{Run, Job}` deserialize from.
    #[tokio::test]
    async fn fetches_run_and_step_timings_from_mock_server() {
        let mock = wiremock::MockServer::start().await;
        let run_id = 999;
        let run_body = serde_json::json!({
            "id": run_id,
            "workflow_id": 1,
            "node_id": "n",
            "name": "build",
            "head_branch": "feature-x",
            "head_sha": "deadbeef",
            "run_number": 1,
            "event": "pull_request",
            "status": "completed",
            "conclusion": "success",
            "created_at": "2026-05-16T11:59:00Z",
            "updated_at": "2026-05-16T12:03:00Z",
            "url": "https://api.github.com/r/x",
            "html_url": "https://github.com/x",
            "jobs_url": "https://api.github.com/r/x/jobs",
            "logs_url": "https://api.github.com/r/x/logs",
            "check_suite_url": "https://api.github.com/r/x/check",
            "artifacts_url": "https://api.github.com/r/x/art",
            "cancel_url": "https://api.github.com/r/x/cancel",
            "rerun_url": "https://api.github.com/r/x/rerun",
            "workflow_url": "https://api.github.com/r/x/wf",
            "head_commit": {
                "id": "deadbeef",
                "tree_id": "tree",
                "message": "msg",
                "timestamp": "2026-05-16T11:59:00Z",
                "author": {"name": "a", "email": "a@x"},
                "committer": {"name": "a", "email": "a@x"},
            },
            "repository": minimal_repo(),
        });
        let jobs_body = serde_json::json!({
            "total_count": 1,
            "jobs": [{
                "id": 1,
                "run_id": run_id,
                "workflow_name": "build",
                "head_branch": "feature-x",
                "run_url": "https://api.github.com/r/x",
                "run_attempt": 1,
                "node_id": "j",
                "head_sha": "deadbeef",
                "url": "https://api.github.com/j/1",
                "html_url": "https://github.com/j/1",
                "status": "completed",
                "conclusion": "success",
                "created_at": "2026-05-16T11:59:30Z",
                "started_at": "2026-05-16T12:00:00Z",
                "completed_at": "2026-05-16T12:03:00Z",
                "name": "build",
                "steps": [
                    {"name": "Set up job", "status": "completed", "conclusion": "success",
                     "number": 1,
                     "started_at": "2026-05-16T12:00:00Z",
                     "completed_at": "2026-05-16T12:00:02Z"},
                    {"name": "Install Bazelisk", "status": "completed", "conclusion": "success",
                     "number": 2,
                     "started_at": "2026-05-16T12:00:02Z",
                     "completed_at": "2026-05-16T12:00:14Z"},
                    {"name": "Test, build docs, and check coverage",
                     "status": "completed", "conclusion": "success", "number": 3,
                     "started_at": "2026-05-16T12:00:14Z",
                     "completed_at": "2026-05-16T12:02:50Z"},
                    {"name": "Post Install Bazelisk",
                     "status": "completed", "conclusion": "success", "number": 4,
                     "started_at": "2026-05-16T12:02:50Z",
                     "completed_at": "2026-05-16T12:03:00Z"},
                ],
                "check_run_url": "https://api.github.com/c/1",
                "labels": ["ubuntu-latest"],
                "runner_id": 1,
                "runner_name": "r",
                "runner_group_id": 1,
                "runner_group_name": "g",
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
        assert_eq!(meta.event, "pull_request");
        let t = client.step_timings(RunId(run_id), "build").await.unwrap();
        assert_eq!(t.job_wall_seconds, 180.0);
        assert_eq!(t.cache_restore_seconds, 12.0);
        assert_eq!(t.cache_save_seconds, 10.0);
        assert_eq!(t.bazel_invocation_seconds, 156.0);
        assert_eq!(t.other_seconds, 2.0);
        let js = client.job_steps(RunId(run_id), "build").await.unwrap();
        assert_eq!(js.job_wall_seconds, 180.0);
        assert_eq!(
            js.steps,
            vec![
                ("Set up job".to_string(), 2.0),
                ("Install Bazelisk".to_string(), 12.0),
                ("Test, build docs, and check coverage".to_string(), 156.0),
                ("Post Install Bazelisk".to_string(), 10.0),
            ]
        );
        assert!(
            client
                .job_steps(RunId(run_id), "no-such-job")
                .await
                .is_err(),
            "unknown job name must error"
        );
    }

    /// `job_log` resolves the job by name and returns the raw GH log body.
    #[tokio::test]
    async fn job_log_fetches_named_jobs_log() {
        let mock = wiremock::MockServer::start().await;
        let run_id = 998;
        let jobs_body = serde_json::json!({
            "total_count": 1,
            "jobs": [{
                "id": 42,
                "run_id": run_id,
                "workflow_name": "build",
                "head_branch": "feature-x",
                "run_url": "https://api.github.com/r/x",
                "run_attempt": 1,
                "node_id": "j",
                "head_sha": "deadbeef",
                "url": "https://api.github.com/j/42",
                "html_url": "https://github.com/j/42",
                "status": "completed",
                "conclusion": "success",
                "created_at": "2026-05-16T11:59:30Z",
                "started_at": "2026-05-16T12:00:00Z",
                "completed_at": "2026-05-16T12:03:00Z",
                "name": "buck2",
                "steps": [],
                "check_run_url": "https://api.github.com/c/42",
                "labels": ["ubuntu-latest"],
                "runner_id": 1,
                "runner_name": "r",
                "runner_group_id": 1,
                "runner_group_name": "g",
            }]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!(
                "/repos/{OWNER}/{REPO}/actions/runs/{run_id}/jobs"
            )))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&jobs_body))
            .mount(&mock)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!(
                "/repos/{OWNER}/{REPO}/actions/jobs/42/logs"
            )))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_string("first line\nsecond line\n"),
            )
            .mount(&mock)
            .await;

        let client = GithubClient::with_base_uri("test-token".into(), mock.uri()).unwrap();
        let log = client.job_log(RunId(run_id), "buck2").await.unwrap();
        assert_eq!(log, "first line\nsecond line\n");
    }

    /// Minimal `Repository` payload sufficient for octocrab's deserialize.
    fn minimal_repo() -> serde_json::Value {
        serde_json::json!({
            "id": 1,
            "node_id": "r",
            "name": "causes-tracker",
            "full_name": "causes-tracker/causes-tracker",
            "owner": {
                "login": "causes-tracker",
                "id": 1,
                "node_id": "o",
                "avatar_url": "https://x",
                "gravatar_id": "",
                "url": "https://api.github.com/u/1",
                "html_url": "https://github.com/causes-tracker",
                "followers_url": "https://x",
                "following_url": "https://x",
                "gists_url": "https://x",
                "starred_url": "https://x",
                "subscriptions_url": "https://x",
                "organizations_url": "https://x",
                "repos_url": "https://x",
                "events_url": "https://x",
                "received_events_url": "https://x",
                "type": "Organization",
                "site_admin": false,
            },
            "private": false,
            "html_url": "https://github.com/causes-tracker/causes-tracker",
            "url": "https://api.github.com/repos/causes-tracker/causes-tracker",
        })
    }
}
