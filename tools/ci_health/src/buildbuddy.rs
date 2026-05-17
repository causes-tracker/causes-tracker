//! BuildBuddy JSON-RPC client.
//! Talks to the same gRPC service as the `bazel ... --bes_backend=...` setup but over HTTP/JSON (the encoding BuildBuddy's frontend uses).
//! Using JSON lets us read a handful of fields without generating proto bindings.

use crate::metrics::{BazelStats, InvocationId};
use anyhow::{Context, Result};
use reqwest::header;
use safelog::Sensitive;
use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_URL: &str = "https://app.buildbuddy.io";

pub struct BuildBuddyClient {
    http: reqwest::Client,
    base_url: String,
}

impl BuildBuddyClient {
    pub fn new(api_key: Sensitive<String>) -> Result<Self> {
        Self::with_base_url(api_key, DEFAULT_BASE_URL.into())
    }

    pub fn with_base_url(api_key: Sensitive<String>, base_url: String) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        // The header value is marked sensitive so reqwest's tracing layer redacts it.
        // The source `Sensitive<String>` is dropped at the end of this constructor.
        // After that the only remaining copy of the key lives inside the http client's headers.
        let mut auth = header::HeaderValue::from_str(api_key.as_inner())
            .context("invalid BuildBuddy api key (rejected by HTTP header)")?;
        auth.set_sensitive(true);
        headers.insert("x-buildbuddy-api-key", auth);
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("building reqwest client")?;
        Ok(Self { http, base_url })
    }

    /// Returns the per-invocation stats we care about: action counts, cache hits (split by local disk vs remote CAS), bytes transferred, duration.
    /// Missing fields are tolerated because BuildBuddy varies what it emits depending on invocation state.
    pub async fn get_invocation(&self, id: &InvocationId) -> Result<InvocationStats> {
        #[derive(Serialize)]
        struct Req<'a> {
            lookup: Lookup<'a>,
        }
        #[derive(Serialize)]
        struct Lookup<'a> {
            #[serde(rename = "invocationId")]
            invocation_id: &'a str,
        }
        let req = Req {
            lookup: Lookup {
                invocation_id: &id.0,
            },
        };
        let raw: serde_json::Value = self.post("GetInvocation", &req).await?;
        Ok(InvocationStats::from_json(&raw))
    }
}

impl BuildBuddyClient {
    async fn post<Req: Serialize, Out: for<'de> Deserialize<'de>>(
        &self,
        rpc: &str,
        body: &Req,
    ) -> Result<Out> {
        let url = format!("{}/rpc/BuildBuddyService/{rpc}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = resp.status();
        let text = resp.text().await.context("read body")?;
        if !status.is_success() {
            anyhow::bail!("BuildBuddy {rpc} -> {status}: {text}");
        }
        serde_json::from_str(&text).with_context(|| format!("decode {rpc} response: {text}"))
    }
}

/// Trimmed view over BuildBuddy's per-invocation stats.
/// We split cache hits into local disk vs remote because a "BuildBuddy cache hit" is a network fetch and is not free.
/// The regression detector trips on either dimension.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InvocationStats {
    pub action_count: u64,
    pub duration_seconds: f64,
    pub local_cache_hits: u64,
    pub remote_cache_hits: u64,
    pub cache_misses: u64,
    pub total_download_bytes: u64,
    pub total_upload_bytes: u64,
}

impl InvocationStats {
    /// BuildBuddy's JSON uses string-encoded int64s (proto3 JSON convention).
    /// The response shape varies by invocation state (still running, partial, completed).
    /// Pull what we can without failing the entire run if a field is missing.
    fn from_json(v: &serde_json::Value) -> Self {
        let inv = v
            .get("invocation")
            .and_then(|x| x.as_array())
            .and_then(|a| a.first());
        let Some(inv) = inv else {
            return Self::default();
        };
        let cache = inv.get("cacheStats");
        let action_cache_hits = i64_field(cache, "actionCacheHits");
        let cas_cache_hits = i64_field(cache, "casCacheHits");
        Self {
            action_count: u64_field(Some(inv), "actionCount"),
            duration_seconds: i64_field(Some(inv), "durationUsec") as f64 / 1_000_000.0,
            // BB does not split local/remote in the wire format.
            // Local disk cache hits never reach BB at all because they short-circuit before the BES upload.
            // So all hits BB reports ARE remote hits.
            // Local hits are inferred from (action_count - hits - misses) by the caller that aggregates across invocations.
            local_cache_hits: 0,
            remote_cache_hits: (action_cache_hits + cas_cache_hits).max(0) as u64,
            cache_misses: u64_field(cache, "actionCacheMisses"),
            total_download_bytes: u64_field(cache, "totalDownloadSizeBytes"),
            total_upload_bytes: u64_field(cache, "totalUploadSizeBytes"),
        }
    }
}

fn u64_field(v: Option<&serde_json::Value>, key: &str) -> u64 {
    i64_field(v, key).max(0) as u64
}

fn i64_field(v: Option<&serde_json::Value>, key: &str) -> i64 {
    let Some(v) = v.and_then(|x| x.get(key)) else {
        return 0;
    };
    if let Some(n) = v.as_i64() {
        return n;
    }
    v.as_str().and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Sum a per-invocation stat list into the aggregate BazelStats shape.
/// Local cache hits are inferred from the leftover after subtracting BB-observed remote hits and misses from the total action count.
pub fn aggregate(invocations: &[InvocationStats]) -> BazelStats {
    let mut out = BazelStats::default();
    for s in invocations {
        out.actions_total += s.action_count;
        out.remote_cache_hits += s.remote_cache_hits;
        out.cache_misses += s.cache_misses;
        out.remote_bytes_downloaded += s.total_download_bytes;
        out.remote_bytes_uploaded += s.total_upload_bytes;
        out.critical_path_seconds += s.duration_seconds;
    }
    let counted = out.remote_cache_hits + out.cache_misses;
    out.local_cache_hits = out.actions_total.saturating_sub(counted);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives `get_invocation` against a wiremock-backed BuildBuddy.
    /// The response shape (camelCase keys, string-encoded int64s, `invocation[0].cacheStats.{actionCacheHits, casCacheHits, ...}`) was captured from a real `GetInvocation` call on app.buildbuddy.io.
    #[tokio::test]
    async fn get_invocation_via_mock() {
        causes_crypto::install_default_provider();
        let mock = wiremock::MockServer::start().await;
        let get_resp = serde_json::json!({
            "invocation": [{
                "invocationId": "uuid-a",
                "actionCount": "500",
                "durationUsec": "120000000",
                "cacheStats": {
                    "actionCacheHits": "300",
                    "casCacheHits": "50",
                    "actionCacheMisses": "20",
                    "totalDownloadSizeBytes": "100000",
                    "totalUploadSizeBytes": "200",
                }
            }]
        });
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path(
                "/rpc/BuildBuddyService/GetInvocation",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&get_resp))
            .mount(&mock)
            .await;

        let client =
            BuildBuddyClient::with_base_url(Sensitive::new("k".into()), mock.uri()).unwrap();
        let stats = client
            .get_invocation(&InvocationId("uuid-a".into()))
            .await
            .unwrap();
        assert_eq!(stats.action_count, 500);
        assert_eq!(stats.remote_cache_hits, 350);
        assert_eq!(stats.cache_misses, 20);
        assert_eq!(stats.total_download_bytes, 100000);
        assert!((stats.duration_seconds - 120.0).abs() < 1e-9);
    }

    #[test]
    fn aggregate_infers_local_hits_from_leftover() {
        let s = vec![InvocationStats {
            action_count: 1000,
            remote_cache_hits: 200,
            cache_misses: 50,
            ..Default::default()
        }];
        let agg = aggregate(&s);
        assert_eq!(agg.local_cache_hits, 750);
        assert_eq!(agg.actions_total, 1000);
    }

    #[test]
    fn aggregate_handles_overcounted_inputs() {
        let s = vec![InvocationStats {
            action_count: 100,
            remote_cache_hits: 200,
            ..Default::default()
        }];
        let agg = aggregate(&s);
        assert_eq!(agg.local_cache_hits, 0);
    }
}
