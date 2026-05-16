use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

mod github;
mod metrics;

use crate::github::GithubClient;
use crate::metrics::{RunId, RunMetrics};

#[derive(Parser, Debug)]
#[command(
    name = "ci_health",
    about = "CI cache and timing health analyzer for the GitHub Actions build workflow"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Fetch GitHub Actions step timings for a workflow run and emit a typed metrics JSON file.
    Record {
        #[arg(long)]
        run_id: u64,
        #[arg(long)]
        out: std::path::PathBuf,
        /// Name of the job inside the run to read step timings from.
        #[arg(long, default_value = "build")]
        job: String,
        /// Pull request number, if the run was triggered by a PR.
        /// The workflow YAML passes `${{ github.event.pull_request.number }}` (empty for push/merge_group), which we record so downstream `compare` / `pr-comment` know which PR to act on.
        /// Carried as a CLI arg rather than read from the workflow run because octocrab's `models::workflows::Run` does not expose `pull_requests` (the schema in the wild does not match what octocrab declared).
        #[arg(long)]
        pr: Option<u64>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Record {
            run_id,
            out,
            job,
            pr,
        } => {
            let token = std::env::var("GH_TOKEN")
                .or_else(|_| std::env::var("GITHUB_TOKEN"))
                .context("GH_TOKEN or GITHUB_TOKEN must be set for the record subcommand")?;
            let gh = GithubClient::new(token)?;
            record(&gh, RunId(run_id), &job, pr, &out).await
        }
    }
}

async fn record(
    gh: &GithubClient,
    run_id: RunId,
    job: &str,
    pr: Option<u64>,
    out: &std::path::Path,
) -> Result<()> {
    let meta = gh.run_metadata(run_id).await?;
    let timings = gh.step_timings(run_id, job).await?;
    let metrics = RunMetrics {
        run_id,
        sha: meta.head_sha,
        pr,
        branch: meta.branch,
        event: meta.event,
        timings,
    };
    let json = serde_json::to_string_pretty(&metrics).context("serialize metrics")?;
    std::fs::write(out, json).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses() {
        Cli::command().debug_assert();
    }

    #[test]
    fn record_subcommand_parses() {
        let cli = Cli::try_parse_from([
            "ci_health",
            "record",
            "--run-id",
            "123",
            "--out",
            "/tmp/m.json",
            "--pr",
            "42",
        ])
        .expect("record args parse");
        assert!(matches!(
            cli.command,
            Command::Record {
                run_id: 123,
                pr: Some(42),
                ..
            }
        ));
    }

    /// End-to-end test of the `record` subcommand body: a `GithubClient` pointed at a wiremock server returning a canned workflow run and jobs response produces the expected metrics JSON on disk.
    #[tokio::test]
    async fn record_writes_expected_metrics_json() {
        let mock = wiremock::MockServer::start().await;
        let run_id = 7777u64;
        let run_body = serde_json::json!({
            "id": run_id,
            "workflow_id": 1,
            "node_id": "n",
            "name": "build",
            "head_branch": "feature/x",
            "head_sha": "feedface",
            "run_number": 1,
            "event": "pull_request",
            "status": "completed",
            "conclusion": "success",
            "created_at": "2026-05-17T00:00:00Z",
            "updated_at": "2026-05-17T00:03:00Z",
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
                "id": "feedface",
                "tree_id": "tree",
                "message": "msg",
                "timestamp": "2026-05-17T00:00:00Z",
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
                "head_branch": "feature/x",
                "run_url": "https://api.github.com/r/x",
                "run_attempt": 1,
                "node_id": "j",
                "head_sha": "feedface",
                "url": "https://api.github.com/j/1",
                "html_url": "https://github.com/j/1",
                "status": "completed",
                "conclusion": "success",
                "created_at": "2026-05-16T23:59:30Z",
                "started_at": "2026-05-17T00:00:00Z",
                "completed_at": "2026-05-17T00:03:00Z",
                "name": "build",
                "steps": [
                    {"name": "Install Bazelisk", "status": "completed", "conclusion": "success",
                     "number": 1,
                     "started_at": "2026-05-17T00:00:00Z",
                     "completed_at": "2026-05-17T00:00:10Z"},
                    {"name": "Test, build docs, and check coverage",
                     "status": "completed", "conclusion": "success", "number": 2,
                     "started_at": "2026-05-17T00:00:10Z",
                     "completed_at": "2026-05-17T00:02:55Z"},
                    {"name": "Post Install Bazelisk",
                     "status": "completed", "conclusion": "success", "number": 3,
                     "started_at": "2026-05-17T00:02:55Z",
                     "completed_at": "2026-05-17T00:03:00Z"},
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
                "/repos/causes-tracker/causes-tracker/actions/runs/{run_id}"
            )))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&run_body))
            .mount(&mock)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!(
                "/repos/causes-tracker/causes-tracker/actions/runs/{run_id}/jobs"
            )))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&jobs_body))
            .mount(&mock)
            .await;

        let gh = GithubClient::with_base_uri("tk".into(), mock.uri()).unwrap();
        let out = tempfile::NamedTempFile::new().unwrap();
        record(&gh, RunId(run_id), "build", Some(99), out.path())
            .await
            .unwrap();

        let parsed: RunMetrics =
            serde_json::from_str(&std::fs::read_to_string(out.path()).unwrap()).unwrap();
        assert_eq!(parsed.run_id, RunId(run_id));
        assert_eq!(parsed.sha.0, "feedface");
        assert_eq!(parsed.pr, Some(99));
        assert_eq!(parsed.branch, "feature/x");
        assert_eq!(parsed.event, "pull_request");
        assert_eq!(parsed.timings.job_wall_seconds, 180.0);
        assert_eq!(parsed.timings.cache_restore_seconds, 10.0);
        assert_eq!(parsed.timings.cache_save_seconds, 5.0);
        assert_eq!(parsed.timings.bazel_invocation_seconds, 165.0);
    }

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
