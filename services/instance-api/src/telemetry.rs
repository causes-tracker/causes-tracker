use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const HONEYCOMB_ENDPOINT: &str = "https://api.honeycomb.io:443";

/// Initialise the tracing subscriber.
/// When `honeycomb_api_key` is `Some`, spans are exported to Honeycomb via
/// OTLP in addition to being written as structured JSON to stdout.
/// Returns a guard that shuts down the OTel pipeline on drop.
pub fn init(service_name: &str, honeycomb_api_key: Option<&str>) -> OtelGuard {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = tracing_subscriber::fmt::layer().json();

    if let Some(api_key) = honeycomb_api_key {
        let exporter = opentelemetry_otlp::new_exporter()
            .tonic()
            .with_endpoint(HONEYCOMB_ENDPOINT)
            .with_metadata({
                let mut meta = tonic::metadata::MetadataMap::new();
                meta.insert(
                    "x-honeycomb-team",
                    api_key.parse().expect("valid Honeycomb API key"),
                );
                meta
            });

        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(exporter)
            .with_trace_config(
                opentelemetry_sdk::trace::Config::default()
                    .with_resource(opentelemetry_sdk::Resource::new(vec![
                        opentelemetry::KeyValue::new("service.name", service_name.to_owned()),
                    ])),
            )
            .install_batch(runtime::Tokio)
            .expect("failed to install OTel OTLP pipeline");

        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(otel_layer)
            .init();

        OtelGuard { enabled: true }
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();

        OtelGuard { enabled: false }
    }
}

/// Shuts down the OTel pipeline when dropped, flushing any pending spans.
pub struct OtelGuard {
    enabled: bool,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if self.enabled {
            global::shutdown_tracer_provider();
        }
    }
}
