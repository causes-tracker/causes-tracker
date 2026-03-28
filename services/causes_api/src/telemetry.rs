use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::Subscriber;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialise the tracing subscriber.
/// When `honeycomb_api_key` is `Some`, spans are exported to Honeycomb via
/// OTLP/HTTP in addition to being written as structured JSON to stdout.
/// Returns a guard that shuts down the OTel pipeline on drop.
pub fn init(
    service_name: &str,
    honeycomb_api_key: Option<&str>,
    honeycomb_endpoint: &str,
) -> OtelGuard {
    let (subscriber, guard) = build(service_name, honeycomb_api_key, honeycomb_endpoint);
    subscriber.init();
    guard
}

/// Build the tracing subscriber and OTel guard without installing globally.
/// Tests can use `tracing::subscriber::set_default` with the returned subscriber.
fn build(
    service_name: &str,
    honeycomb_api_key: Option<&str>,
    honeycomb_endpoint: &str,
) -> (Box<dyn Subscriber + Send + Sync>, OtelGuard) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = tracing_subscriber::fmt::layer().json();

    if let Some(api_key) = honeycomb_api_key {
        let mut headers = std::collections::HashMap::new();
        headers.insert("x-honeycomb-team".to_string(), api_key.to_string());

        let traces_endpoint = format!("{}/v1/traces", honeycomb_endpoint.trim_end_matches('/'));

        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(traces_endpoint)
            .with_headers(headers)
            .build()
            .expect("failed to build OTel OTLP exporter");

        let resource = Resource::builder()
            .with_attribute(KeyValue::new("service.name", service_name.to_owned()))
            .build();

        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(resource)
            .build();

        global::set_tracer_provider(provider.clone());
        let tracer = provider.tracer(service_name.to_owned());
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(otel_layer);

        (
            Box::new(subscriber),
            OtelGuard {
                provider: Some(provider),
            },
        )
    } else {
        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer);

        (Box::new(subscriber), OtelGuard { provider: None })
    }
}

/// Shuts down the OTel pipeline when dropped, flushing any pending spans.
pub struct OtelGuard {
    provider: Option<SdkTracerProvider>,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take() {
            if let Err(e) = provider.shutdown() {
                eprintln!("OTel shutdown error: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tracing::subscriber::set_default;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn otel_exporter_sends_traces_to_endpoint() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/traces"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1..)
            .mount(&mock)
            .await;

        let (subscriber, guard) = build("test-svc", Some("fake-key"), &mock.uri());
        let _default = set_default(subscriber);

        tracing::info_span!("test_span").in_scope(|| {
            tracing::info!("hello from test");
        });

        // Drop flushes pending spans to the exporter.
        drop(guard);

        mock.verify().await;
    }

    #[tokio::test]
    async fn init_without_api_key_returns_no_provider() {
        let (subscriber, guard) = build("test-svc", None, "http://unused");
        let _default = set_default(subscriber);

        assert!(guard.provider.is_none());
    }
}
