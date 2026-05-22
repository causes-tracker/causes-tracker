use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

mod artifacts;
mod buildbuddy;
mod comparison;
mod config;
mod github;
mod metrics;

use crate::artifacts::ArtifactClient;
use crate::buildbuddy::BuildBuddyClient;
use crate::comparison::{
    Baseline, COMMENT_MARKER, Verdict, classify, load_baseline_dir, render_pr_comment,
};
use crate::github::{GithubClient, OWNER, REPO, token_from_env};
use crate::metrics::{RunId, RunMetrics};
use octocrab::Octocrab;
use octocrab::models::CommentId;

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
    /// Fetch GitHub Actions step timings + BuildBuddy cache stats for a workflow run and emit a typed metrics JSON file.
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
    /// Classify a run's metrics against a baseline directory of recent
    /// successful master runs. Exit code reflects the verdict (0 = ok,
    /// 1 = regressed). The baseline directory must contain one
    /// `<anything>.json` file per past run, in the same `RunMetrics`
    /// schema that `record` emits.
    Compare {
        #[arg(long)]
        current: std::path::PathBuf,
        #[arg(long)]
        baseline_dir: std::path::PathBuf,
    },
    /// Upsert a regression-report comment on a pull request when its
    /// build is materially slower or has worse cache behavior than
    /// baseline. Fetches recent successful master metrics artifacts
    /// automatically. Silent (no comment touched) when the run is healthy.
    PrComment {
        #[arg(long)]
        current: std::path::PathBuf,
        #[arg(long, default_value_t = 20)]
        baseline_window: usize,
        #[arg(long)]
        pr: u64,
    },
    /// Developer-facing inspection of CI health from a local terminal.
    /// Pick exactly one of --pr / --branch / --run-id / --baseline.
    Query {
        #[arg(long, conflicts_with_all = ["branch", "run_id", "baseline"])]
        pr: Option<u64>,
        #[arg(long, conflicts_with_all = ["pr", "run_id", "baseline"])]
        branch: Option<String>,
        #[arg(long, conflicts_with_all = ["pr", "branch", "baseline"])]
        run_id: Option<u64>,
        #[arg(long, conflicts_with_all = ["pr", "branch", "run_id"])]
        baseline: bool,
        #[arg(long, default_value_t = 1)]
        last: usize,
        #[arg(long)]
        verbose: bool,
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    causes_crypto::install_default_provider();
    let cli = Cli::parse();
    match cli.command {
        Command::Record {
            run_id,
            out,
            job,
            pr,
        } => {
            let token = token_from_env("record")?;
            let bb_key = safelog::Sensitive::new(
                std::env::var("BUILDBUDDY_API_KEY")
                    .context("BUILDBUDDY_API_KEY must be set for the record subcommand")?,
            );
            let gh = GithubClient::new(token)?;
            let bb = BuildBuddyClient::new(bb_key)?;
            record(&gh, &bb, RunId(run_id), &job, pr, &out).await
        }
        Command::Compare {
            current,
            baseline_dir,
        } => compare(&current, &baseline_dir),
        Command::PrComment {
            current,
            baseline_window,
            pr,
        } => pr_comment(&current, baseline_window, pr).await,
        Command::Query {
            pr,
            branch,
            run_id,
            baseline,
            last,
            verbose: _,
            json,
        } => {
            query(QueryArgs {
                pr,
                branch,
                run_id,
                baseline,
                last,
                json,
            })
            .await
        }
    }
}

fn compare(current: &std::path::Path, baseline_dir: &std::path::Path) -> Result<()> {
    let cur_text =
        std::fs::read_to_string(current).with_context(|| format!("read {}", current.display()))?;
    let cur: RunMetrics =
        serde_json::from_str(&cur_text).with_context(|| format!("parse {}", current.display()))?;
    let runs = load_baseline_dir(baseline_dir)?;
    let baseline = Baseline::from_runs(&runs);
    let thresholds = config::PR;
    let verdict = classify(&cur, &baseline, thresholds);
    match verdict {
        Verdict::Ok => {
            println!(
                "ok: job {:.0}s, cache hit rate {:.1}% (baseline n={}, median {:.0}s / {:.1}%)",
                cur.timings.job_wall_seconds,
                cur.bazel.cache_hit_rate().unwrap_or(0.0) * 100.0,
                baseline.sample_count,
                baseline.median_job_wall_seconds,
                baseline.median_cache_hit_rate * 100.0,
            );
            Ok(())
        }
        Verdict::Regressed { reasons } => {
            eprintln!("regressed:");
            for r in &reasons {
                eprintln!("  - {r}");
            }
            std::process::exit(1);
        }
    }
}

async fn pr_comment(current: &std::path::Path, window: usize, pr: u64) -> Result<()> {
    let token = token_from_env("pr-comment")?;

    let cur_text =
        std::fs::read_to_string(current).with_context(|| format!("read {}", current.display()))?;
    let cur: RunMetrics =
        serde_json::from_str(&cur_text).with_context(|| format!("parse {}", current.display()))?;

    let artifacts = ArtifactClient::new(token.clone(), OWNER.into(), REPO.into())?;
    let runs = artifacts.fetch_baseline_runs("master", window).await?;
    let baseline = Baseline::from_runs(&runs);
    let thresholds = config::PR;
    let verdict = classify(&cur, &baseline, thresholds);

    let Verdict::Regressed { .. } = &verdict else {
        println!(
            "ok: no regression vs baseline (n={})",
            baseline.sample_count
        );
        return Ok(());
    };

    let body = render_pr_comment(&cur, &baseline, &verdict);
    let octo = Octocrab::builder()
        .personal_token(token)
        .build()
        .context("build octocrab")?;
    upsert_marked_comment(&octo, pr, &body).await?;
    println!("posted regression comment on PR #{pr}");
    Ok(())
}

/// Find an existing comment matching our hidden marker and update it;
/// otherwise create a new one. The marker prevents the bot from stacking
/// comments across re-runs of the same PR.
async fn upsert_marked_comment(octo: &Octocrab, pr: u64, body: &str) -> Result<()> {
    let issues = octo.issues(OWNER, REPO);
    let mut page = issues
        .list_comments(pr)
        .per_page(100)
        .send()
        .await
        .context("list comments")?;
    loop {
        for c in &page.items {
            if c.body
                .as_deref()
                .map(|b| b.contains(COMMENT_MARKER))
                .unwrap_or(false)
            {
                issues
                    .update_comment(CommentId(c.id.0), body)
                    .await
                    .context("update comment")?;
                return Ok(());
            }
        }
        match octo
            .get_page::<octocrab::models::issues::Comment>(&page.next)
            .await
        {
            Ok(Some(next)) => page = next,
            _ => break,
        }
    }
    issues
        .create_comment(pr, body)
        .await
        .context("create comment")?;
    Ok(())
}

struct QueryArgs {
    pr: Option<u64>,
    branch: Option<String>,
    run_id: Option<u64>,
    baseline: bool,
    last: usize,
    json: bool,
}

async fn query(args: QueryArgs) -> Result<()> {
    let token = token_from_env("query")?;
    let artifacts = ArtifactClient::new(token.clone(), OWNER.into(), REPO.into())?;

    if args.baseline {
        let runs = artifacts.fetch_baseline_runs("master", 20).await?;
        let b = Baseline::from_runs(&runs);
        if args.json {
            println!("{}", serde_json::to_string_pretty(&BaselineJson::from(&b))?);
        } else {
            println!(
                "baseline (n={}): median wall {:.0}s, median cache hit rate {:.1}%",
                b.sample_count,
                b.median_job_wall_seconds,
                b.median_cache_hit_rate * 100.0,
            );
        }
        return Ok(());
    }

    if let Some(run_id) = args.run_id {
        let metrics = artifacts.fetch_run_metrics(run_id).await?;
        emit_one_or_none(metrics, args.json);
        return Ok(());
    }

    if let Some(pr) = args.pr {
        anyhow::bail!(
            "query --pr {pr} is not yet wired; pass --run-id <id> from the PR's checks tab",
        );
    }

    let branch = args.branch.unwrap_or_else(|| "master".into());
    let runs = artifacts.fetch_baseline_runs(&branch, args.last).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&runs)?);
    } else {
        print_run_table(&branch, &runs);
    }
    Ok(())
}

fn emit_one_or_none(metrics: Option<RunMetrics>, json: bool) {
    let Some(m) = metrics else {
        println!("no metrics artifact found for that run");
        return;
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&m).expect("serialize"));
    } else {
        let hit_rate = m
            .bazel
            .cache_hit_rate()
            .map(|r| format!("{:.1}%", r * 100.0))
            .unwrap_or_else(|| "n/a".into());
        println!(
            "run {} on {} ({}): wall {:.0}s = restore {:.0} + bazel {:.0} + save {:.0} + other {:.0}",
            m.run_id.0,
            m.branch,
            m.event,
            m.timings.job_wall_seconds,
            m.timings.cache_restore_seconds,
            m.timings.bazel_invocation_seconds,
            m.timings.cache_save_seconds,
            m.timings.other_seconds,
        );
        println!(
            "  bazel: {} actions, {} local hits, {} remote hits, {} misses, {} bytes down",
            m.bazel.actions_total,
            m.bazel.local_cache_hits,
            m.bazel.remote_cache_hits,
            m.bazel.cache_misses,
            m.bazel.remote_bytes_downloaded,
        );
        println!("  cache hit rate: {hit_rate}");
    }
}

fn print_run_table(branch: &str, runs: &[RunMetrics]) {
    if runs.is_empty() {
        println!("no metrics artifacts found for branch {branch}");
        return;
    }
    println!("branch={branch}, n={}", runs.len());
    println!(
        "{:>12}  {:>7}  {:>7}  {:>7}  {:>7}  {:>8}",
        "run_id", "wall", "rest", "bazel", "save", "hit_rate"
    );
    for m in runs {
        let hit_rate = m
            .bazel
            .cache_hit_rate()
            .map(|r| format!("{:.1}%", r * 100.0))
            .unwrap_or_else(|| "n/a".into());
        println!(
            "{:>12}  {:>6.0}s  {:>6.0}s  {:>6.0}s  {:>6.0}s  {:>8}",
            m.run_id.0,
            m.timings.job_wall_seconds,
            m.timings.cache_restore_seconds,
            m.timings.bazel_invocation_seconds,
            m.timings.cache_save_seconds,
            hit_rate,
        );
    }
}

#[derive(serde::Serialize)]
struct BaselineJson {
    sample_count: usize,
    median_job_wall_seconds: f64,
    median_cache_hit_rate: f64,
}

impl From<&Baseline> for BaselineJson {
    fn from(b: &Baseline) -> Self {
        Self {
            sample_count: b.sample_count,
            median_job_wall_seconds: b.median_job_wall_seconds,
            median_cache_hit_rate: b.median_cache_hit_rate,
        }
    }
}

async fn record(
    gh: &GithubClient,
    bb: &BuildBuddyClient,
    run_id: RunId,
    job: &str,
    pr: Option<u64>,
    out: &std::path::Path,
) -> Result<()> {
    let started = std::time::Instant::now();
    let meta = gh.run_metadata(run_id).await?;
    let timings = gh.step_timings(run_id, job).await?;
    let invocation_ids = gh.bazel_invocation_ids(run_id, job).await?;
    let mut stats = Vec::with_capacity(invocation_ids.len());
    for id in &invocation_ids {
        stats.push(bb.get_invocation(id).await?);
    }
    let metrics = RunMetrics {
        run_id,
        sha: meta.head_sha,
        pr,
        branch: meta.branch,
        event: meta.event,
        timings,
        bazel: buildbuddy::aggregate(&stats),
        bb_invocation_ids: invocation_ids,
        metrics_collection_seconds: started.elapsed().as_secs_f64(),
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

    #[test]
    fn compare_subcommand_parses() {
        let cli = Cli::try_parse_from([
            "ci_health",
            "compare",
            "--current",
            "/tmp/c.json",
            "--baseline-dir",
            "/tmp/b",
        ])
        .expect("compare args parse");
        assert!(matches!(cli.command, Command::Compare { .. }));
    }

    #[test]
    fn pr_comment_subcommand_parses() {
        let cli = Cli::try_parse_from([
            "ci_health",
            "pr-comment",
            "--current",
            "/tmp/c.json",
            "--pr",
            "42",
        ])
        .expect("pr-comment args parse");
        assert!(matches!(cli.command, Command::PrComment { pr: 42, .. }));
    }

    #[test]
    fn query_pr_conflicts_with_branch() {
        let err = Cli::try_parse_from(["ci_health", "query", "--pr", "1", "--branch", "master"])
            .expect_err("--pr and --branch are mutually exclusive");
        assert!(matches!(
            err.kind(),
            clap::error::ErrorKind::ArgumentConflict
        ));
    }

    /// End-to-end test of the BB-extended `record`: both GH and BB clients pointed at wiremock servers produce a metrics JSON with populated bazel stats.
    #[tokio::test]
    async fn record_writes_metrics_with_bazel_stats() {
        causes_crypto::install_default_provider();
        let gh_mock = wiremock::MockServer::start().await;
        let bb_mock = wiremock::MockServer::start().await;
        let run_id = 7777u64;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!(
                "/repos/causes-tracker/causes-tracker/actions/runs/{run_id}"
            )))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(run_body(run_id)))
            .mount(&gh_mock)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!(
                "/repos/causes-tracker/causes-tracker/actions/runs/{run_id}/jobs"
            )))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(jobs_body(run_id)))
            .mount(&gh_mock)
            .await;
        // GH job log: bazel prints one BB URL per `--bes_results_url`
        // invocation; `bazel_invocation_ids` regexes them out.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/repos/causes-tracker/causes-tracker/actions/jobs/1/logs",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(
                "INFO: Streaming build results to: https://app.buildbuddy.io/invocation/1234aaaa-bbbb-cccc-dddd-eeeeeeeeeeee\n",
            ))
            .mount(&gh_mock)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path(
                "/rpc/BuildBuddyService/GetInvocation",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"invocation": [{
                    "invocationId": "1234aaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                    "actionCount": "100",
                    "cacheStats": {
                        "actionCacheHits": "60",
                        "actionCacheMisses": "10",
                    }
                }]}),
            ))
            .mount(&bb_mock)
            .await;

        let gh = GithubClient::with_base_uri("tk".into(), gh_mock.uri()).unwrap();
        let bb =
            BuildBuddyClient::with_base_url(safelog::Sensitive::new("k".into()), bb_mock.uri())
                .unwrap();
        let out = tempfile::NamedTempFile::new().unwrap();
        record(&gh, &bb, RunId(run_id), "build", Some(99), out.path())
            .await
            .unwrap();

        let parsed: RunMetrics =
            serde_json::from_str(&std::fs::read_to_string(out.path()).unwrap()).unwrap();
        assert_eq!(parsed.pr, Some(99));
        assert_eq!(parsed.bazel.actions_total, 100);
        assert_eq!(parsed.bazel.remote_cache_hits, 60);
        assert_eq!(parsed.bazel.cache_misses, 10);
        assert_eq!(parsed.bb_invocation_ids.len(), 1);
        // Self-timer was started and stopped; with mock latency it's tiny
        // but strictly positive.
        assert!(parsed.metrics_collection_seconds > 0.0);
    }

    fn run_body(run_id: u64) -> serde_json::Value {
        serde_json::json!({
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
        })
    }

    fn jobs_body(run_id: u64) -> serde_json::Value {
        serde_json::json!({
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
                "completed_at": "2026-05-17T00:01:00Z",
                "name": "build",
                "steps": [],
                "check_run_url": "https://api.github.com/c/1",
                "labels": ["ubuntu-latest"],
                "runner_id": 1,
                "runner_name": "r",
                "runner_group_id": 1,
                "runner_group_name": "g",
            }]
        })
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
