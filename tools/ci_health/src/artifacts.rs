//! Pull recent `ci-metrics-*` workflow-run artifacts from GitHub Actions
//! to build the rolling baseline.
//!
//! `download_artifact` and `list_workflow_run_artifacts` go through
//! octocrab, which handles auth, the 302 redirect to the presigned blob,
//! and pagination.
//!
//! The repo-wide listing endpoint
//! (`GET /repos/{owner}/{repo}/actions/artifacts`) is reached through
//! `octocrab.get` with a local response type: octocrab's
//! `WorkflowListArtifact` model omits the parent run's `head_branch`,
//! which the baseline filter needs.

use crate::metrics::RunMetrics;
use anyhow::{Context, Result};
use octocrab::Octocrab;
use octocrab::models::ArtifactId;
use octocrab::params::actions::ArchiveFormat;
use serde::Deserialize;

const ARTIFACT_NAME_PREFIX: &str = "ci-metrics-";

pub struct ArtifactClient {
    octo: Octocrab,
    owner: String,
    repo: String,
}

impl ArtifactClient {
    pub fn new(token: String, owner: String, repo: String) -> Result<Self> {
        let octo = Octocrab::builder()
            .personal_token(token)
            .build()
            .context("build octocrab")?;
        Ok(Self { octo, owner, repo })
    }

    #[cfg(test)]
    pub fn with_base_uri(
        token: String,
        owner: String,
        repo: String,
        base_uri: String,
    ) -> Result<Self> {
        let octo = Octocrab::builder()
            .personal_token(token)
            .base_uri(base_uri)
            .context("set base_uri")?
            .build()
            .context("build octocrab")?;
        Ok(Self { octo, owner, repo })
    }

    /// Pull the most recent `take` master CI metrics records.
    /// Filtering on branch happens client-side because GitHub's artifact
    /// endpoint has no server-side filter on branch + name.
    pub async fn fetch_baseline_runs(&self, branch: &str, take: usize) -> Result<Vec<RunMetrics>> {
        let route = format!(
            "/repos/{}/{}/actions/artifacts?per_page=100",
            self.owner, self.repo
        );
        let listing: ArtifactList = self
            .octo
            .get(&route, None::<&()>)
            .await
            .context("list repo artifacts")?;
        let mut runs = Vec::new();
        for a in listing.artifacts {
            if runs.len() >= take {
                break;
            }
            if !a.name.starts_with(ARTIFACT_NAME_PREFIX) {
                continue;
            }
            let Some(wr) = &a.workflow_run else { continue };
            if wr.head_branch.as_deref() != Some(branch) {
                continue;
            }
            match self.download_and_extract(a.id).await {
                Ok(run) => runs.push(run),
                // A single corrupted/missing artifact must not poison
                // the whole baseline computation.
                Err(e) => eprintln!("ci_health: skipping artifact {}: {e:#}", a.id),
            }
        }
        Ok(runs)
    }

    async fn download_and_extract(&self, artifact_id: ArtifactId) -> Result<RunMetrics> {
        let bytes = self
            .octo
            .actions()
            .download_artifact(&self.owner, &self.repo, artifact_id, ArchiveFormat::Zip)
            .await
            .with_context(|| format!("download artifact {artifact_id}"))?;
        extract_metrics_from_zip(&bytes)
    }
}

#[derive(Deserialize)]
struct ArtifactList {
    artifacts: Vec<Artifact>,
}

#[derive(Deserialize)]
struct Artifact {
    id: ArtifactId,
    name: String,
    workflow_run: Option<ArtifactWorkflowRun>,
}

#[derive(Deserialize)]
struct ArtifactWorkflowRun {
    head_branch: Option<String>,
}

/// Pulls `ci-metrics.json` out of the workflow-artifact zip. The upload-
/// artifact action zips a single file at the root, so the search is
/// straightforward; we accept any `.json` to be tolerant of future
/// renames.
fn extract_metrics_from_zip(bytes: &[u8]) -> Result<RunMetrics> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("parse artifact zip")?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("read zip entry")?;
        let name = entry.name().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let mut text = String::new();
        std::io::Read::read_to_string(&mut entry, &mut text)
            .with_context(|| format!("read {name}"))?;
        return serde_json::from_str(&text).with_context(|| format!("parse {name}"));
    }
    anyhow::bail!("no .json entry found in artifact zip")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{BazelStats, CommitSha, RunId, StepTimings};
    use std::io::Write;

    fn sample_run(id: u64) -> RunMetrics {
        RunMetrics {
            run_id: RunId(id),
            sha: CommitSha(format!("sha-{id}")),
            pr: None,
            branch: "master".into(),
            event: "push".into(),
            timings: StepTimings {
                job_wall_seconds: 180.0,
                cache_restore_seconds: 10.0,
                cache_save_seconds: 5.0,
                bazel_invocation_seconds: 150.0,
                other_seconds: 15.0,
            },
            bazel: BazelStats::default(),
            bb_invocation_ids: vec![],
            metrics_collection_seconds: 0.0,
        }
    }

    fn zip_metrics(run: &RunMetrics) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts: zip::write::SimpleFileOptions = Default::default();
            w.start_file("ci-metrics.json", opts).unwrap();
            w.write_all(serde_json::to_string(run).unwrap().as_bytes())
                .unwrap();
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn extract_round_trips() {
        let run = sample_run(7);
        let zip_bytes = zip_metrics(&run);
        let back = extract_metrics_from_zip(&zip_bytes).unwrap();
        assert_eq!(back, run);
    }

    #[test]
    fn extract_errors_on_empty_zip() {
        let mut buf = Vec::new();
        {
            let w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            w.finish().unwrap();
        }
        let err = extract_metrics_from_zip(&buf).unwrap_err();
        assert!(err.to_string().contains("no .json entry"));
    }

    #[tokio::test]
    async fn fetches_filters_and_extracts_from_mock_server() {
        causes_crypto::install_default_provider();
        let mock = wiremock::MockServer::start().await;

        let list = serde_json::json!({
            "artifacts": [
                {"id": 1, "name": "ci-metrics-1",
                 "workflow_run": {"head_branch": "master"}},
                {"id": 2, "name": "ci-metrics-2",
                 "workflow_run": {"head_branch": "feature"}},
                {"id": 3, "name": "ci-metrics-3",
                 "workflow_run": {"head_branch": "master"}},
                {"id": 4, "name": "some-other-artifact",
                 "workflow_run": {"head_branch": "master"}},
            ]
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/repos/o/r/actions/artifacts"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&list))
            .mount(&mock)
            .await;
        for (id, run_id) in [(1u64, 11u64), (3, 13)] {
            let body = zip_metrics(&sample_run(run_id));
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .and(wiremock::matchers::path(format!(
                    "/repos/o/r/actions/artifacts/{id}/zip"
                )))
                .respond_with(
                    wiremock::ResponseTemplate::new(200)
                        .set_body_bytes(body)
                        .insert_header("content-type", "application/zip"),
                )
                .mount(&mock)
                .await;
        }

        let client =
            ArtifactClient::with_base_uri("tk".into(), "o".into(), "r".into(), mock.uri()).unwrap();
        let runs = client.fetch_baseline_runs("master", 20).await.unwrap();
        let ids: Vec<u64> = runs.iter().map(|r| r.run_id.0).collect();
        assert_eq!(ids, vec![11, 13]);
    }
}
