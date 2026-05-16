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
    /// Fetch GitHub Actions step timings for a workflow run and emit a
    /// typed metrics JSON file.
    Record {
        #[arg(long)]
        run_id: u64,
        #[arg(long)]
        out: std::path::PathBuf,
        /// Name of the job inside the run to read step timings from.
        #[arg(long, default_value = "build")]
        job: String,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Record { run_id, out, job } => record(RunId(run_id), &job, &out).await,
    }
}

async fn record(run_id: RunId, job: &str, out: &std::path::Path) -> Result<()> {
    let token = std::env::var("GH_TOKEN")
        .or_else(|_| std::env::var("GITHUB_TOKEN"))
        .context("GH_TOKEN or GITHUB_TOKEN must be set for the record subcommand")?;
    let gh = GithubClient::new(token)?;
    let meta = gh.run_metadata(run_id).await?;
    let timings = gh.step_timings(run_id, job).await?;
    let metrics = RunMetrics {
        run_id,
        sha: meta.head_sha,
        pr: meta.pr,
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
        ])
        .expect("record args parse");
        assert!(matches!(cli.command, Command::Record { run_id: 123, .. }));
    }
}
