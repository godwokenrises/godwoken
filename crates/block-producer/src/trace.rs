use anyhow::Result;
use gw_config::Trace;
use sentry_tracing::EventFilter;
use tracing_subscriber::prelude::*;

pub struct ShutdownGuard {
    trace: Option<Trace>,
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        if let Some(Trace::Jaeger) = self.trace {
            opentelemetry::global::shutdown_tracer_provider(); // Sending remaining spans
        }
    }
}

pub fn init(trace: Option<Trace>) -> Result<ShutdownGuard> {
    let env_filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new("info"))?;

    // NOTE: `traces_sample_rate` in sentry client option is 0.0 by default, which disable sentry
    // tracing. Here we just use sentry-log feature.
    let sentry_layer = sentry_tracing::layer().event_filter(|md| match md.level() {
        &tracing::Level::ERROR | &tracing::Level::WARN => EventFilter::Event,
        _ => EventFilter::Ignore,
    });

    let registry = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .with(env_filter_layer)
        .with(sentry_layer);

    match trace {
        Some(Trace::Jaeger) => {
            opentelemetry::global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());

            let jaeger_layer = {
                let tracer = opentelemetry_jaeger::new_pipeline()
                    .with_service_name("godwoken")
                    .install_batch(opentelemetry::runtime::Tokio)?;
                tracing_opentelemetry::layer().with_tracer(tracer)
            };

            registry.with(jaeger_layer).try_init()?
        }
        None => registry.try_init()?,
    }

    Ok(ShutdownGuard { trace })
}
