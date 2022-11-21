use tracing_appender::non_blocking;
use tracing_subscriber::{prelude::*, EnvFilter};

pub mod format;
pub use opentelemetry::trace::*;
pub use opentelemetry_http as http;

const ENV_OTEL_TRACES_EXPORTER: &str = "OTEL_TRACES_EXPORTER";
const DEFAULT_LOG_LEVEL: &str = "info";

#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub enum TraceInitError {
    Opentelemetry(#[from] opentelemetry::trace::TraceError),
    ParseError(#[from] tracing_subscriber::filter::ParseError),
    TryInitError(#[from] tracing_subscriber::util::TryInitError),
}

pub enum TraceExporter {
    None,
    Jaeger,
}

pub struct TraceGuard {
    _non_blocking_worker: non_blocking::WorkerGuard,
    trace_exporter: TraceExporter,
}

impl Drop for TraceGuard {
    fn drop(&mut self) {
        if !matches!(self.trace_exporter, TraceExporter::None) {
            opentelemetry::global::shutdown_tracer_provider(); // Sending remaining spans
        }
    }
}

pub fn init() -> Result<TraceGuard, TraceInitError> {
    let trace_exporter = match std::env::var(ENV_OTEL_TRACES_EXPORTER).as_deref() {
        Ok("jaeger") => TraceExporter::Jaeger,
        Ok("none") => TraceExporter::None,
        Err(_) | Ok(_) => TraceExporter::None,
    };

    let env_filter_layer =
        EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new(DEFAULT_LOG_LEVEL))?;

    let (fmt_layer, _non_blocking_worker) = {
        let (non_blocking_stdout, non_blocking_worker) = non_blocking(std::io::stdout());
        let layer = tracing_subscriber::fmt::layer().with_writer(non_blocking_stdout);

        let layer = match trace_exporter {
            TraceExporter::None => layer.boxed(),
            _ => { layer.json() } // Use json for better trace info support
                .with_current_span(true)
                .event_format(format::TraceFormat) // Add trace info to log
                .boxed(),
        };
        (layer, non_blocking_worker)
    };

    let trace_layer = match trace_exporter {
        TraceExporter::Jaeger => {
            // TODO: opentelemetry-otpl requires protoc cli, wait next release
            // Reference: https://github.com/open-telemetry\/telemetry-rust/pull/881
            opentelemetry::global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());

            // Set serivce name through `OTEL_SERVICE_NAME` or `OTEL_RESOURCE_ATTRIBUTES: service.name`
            let tracer = opentelemetry_jaeger::new_agent_pipeline()
                .with_auto_split_batch(true)
                .install_batch(opentelemetry::runtime::Tokio)?;

            Some(tracing_opentelemetry::layer().with_tracer(tracer).boxed())
        }
        TraceExporter::None => None,
    };

    let registry = tracing_subscriber::registry()
        .with(fmt_layer)
        .with(env_filter_layer);

    match trace_layer {
        Some(layer) => registry.with(layer).try_init()?,
        None => registry.try_init()?,
    }

    let guard = TraceGuard {
        _non_blocking_worker,
        trace_exporter,
    };

    Ok(guard)
}
