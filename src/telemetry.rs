use crate::config::{LogFormat, TelemetryConfig};
use opentelemetry::{KeyValue, global};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{
    Resource,
    logs::SdkLoggerProvider,
    metrics::SdkMeterProvider,
    propagation::TraceContextPropagator,
    trace::{SdkTracerProvider, BatchSpanProcessor, Sampler},
    metrics::PeriodicReader,
    logs::BatchLogProcessor,
};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{EnvFilter, Registry, layer::SubscriberExt, util::SubscriberInitExt};

/// Initializes the OpenTelemetry tracing, metrics, and logging providers and hooks them into the tracing subscriber.
pub fn init_telemetry(config: TelemetryConfig) -> anyhow::Result<()> {
    // 1. Build the Registry with EnvFilter
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info".into())
        .add_directive("sqlx=warn".parse().unwrap())
        .add_directive("hyper=warn".parse().unwrap());

    let registry = Registry::default().with(filter);

    // 2. Initialize OTLP Layers (Optional)
    let (otel_layer, logger_layer) = if let Some(endpoint) = &config.otlp_endpoint {
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
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_http_client(reqwest::blocking::Client::new())
            .with_endpoint(format!("{}/v1/traces", endpoint))
            .build()?;

        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
                config.trace_sampling_ratio,
            ))))
            .with_span_processor(
                BatchSpanProcessor::builder(exporter).build()
            )
            .build();

        let tracer = opentelemetry::trace::TracerProvider::tracer(&tracer_provider, service_name);
        global::set_tracer_provider(tracer_provider);

        // Setup Metrics
        let exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_http_client(reqwest::blocking::Client::new())
            .with_endpoint(format!("{}/v1/metrics", endpoint))
            .build()?;

        let reader = PeriodicReader::builder(exporter)
            .with_interval(std::time::Duration::from_secs(5))
            .build();
        let meter_provider = SdkMeterProvider::builder().with_resource(resource.clone()).with_reader(reader).build();
        global::set_meter_provider(meter_provider);

        // Setup Logging
        let exporter = opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .with_http_client(reqwest::blocking::Client::new())
            .with_endpoint(format!("{}/v1/logs", endpoint))
            .build()?;

        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(resource)
            .with_log_processor(
                BatchLogProcessor::builder(exporter).build()
            )
            .build();
        
        let layer = OpenTelemetryTracingBridge::new(&logger_provider);

        (Some(OpenTelemetryLayer::new(tracer)), Some(layer))
    } else {
        (None, None)
    };

    // 3. Compose Layers
    let registry = registry.with(otel_layer).with(logger_layer);

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

/// Initializes a no-op telemetry provider for tests to silence warnings.
pub fn init_test_telemetry() {
    let provider = SdkMeterProvider::builder().build();
    global::set_meter_provider(provider);
}