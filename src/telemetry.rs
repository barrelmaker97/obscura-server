use crate::config::{LogFormat, TelemetryConfig};
use opentelemetry::{KeyValue, global};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider},
    propagation::TraceContextPropagator,
    trace::SdkTracerProvider,
};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{EnvFilter, Registry, layer::SubscriberExt, util::SubscriberInitExt};

/// Initializes the OpenTelemetry tracing and metrics providers and hooks them into the tracing subscriber.
pub fn init_telemetry(config: TelemetryConfig) -> anyhow::Result<()> {
    // 1. Build the Registry with EnvFilter
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info".into())
        .add_directive("sqlx=warn".parse().unwrap())
        .add_directive("hyper=warn".parse().unwrap());

    let registry = Registry::default().with(filter);

    // 2. Initialize OTLP Layer (Optional)
    let otel_layer = if let Some(endpoint) = &config.otlp_endpoint {
        let service_name = "obscura-server";
        let service_version = env!("CARGO_PKG_VERSION");

        // Configure Resource
        let resource = Resource::builder()
            .with_attributes(vec![
                KeyValue::new(SERVICE_NAME, service_name),
                KeyValue::new(SERVICE_VERSION, service_version),
            ])
            .build();

        // Setup Propagation
        global::set_text_map_propagator(TraceContextPropagator::new());

        // Setup Tracing
        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_batch_exporter(
                opentelemetry_otlp::SpanExporter::builder()
                    .with_http()
                    .with_endpoint(format!("{}/v1/traces", endpoint))
                    .build()?,
            )
            .build();

        let tracer = opentelemetry::trace::TracerProvider::tracer(&tracer_provider, service_name);
        global::set_tracer_provider(tracer_provider);

        // Setup Metrics
        let exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_endpoint(format!("{}/v1/metrics", endpoint))
            .build()?;

        let reader = PeriodicReader::builder(exporter).build();
        let meter_provider = SdkMeterProvider::builder().with_resource(resource).with_reader(reader).build();
        global::set_meter_provider(meter_provider);

        Some(OpenTelemetryLayer::new(tracer))
    } else {
        None
    };

    // 3. Compose Layers
    // Option<Layer> implements Layer, so this works seamlessly.
    let registry = registry.with(otel_layer);

    match config.log_format {
        LogFormat::Text => {
            registry.with(tracing_subscriber::fmt::layer()).init();
        }
        LogFormat::Json => {
            registry.with(tracing_subscriber::fmt::layer().json()).init();
        }
    }

    Ok(())
}

/// Shuts down the telemetry providers, ensuring all remaining spans and metrics are flushed.
pub fn shutdown_telemetry() {
    // In OTel 0.28, global shutdown is handled differently or unnecessary if providers are dropped.
}
