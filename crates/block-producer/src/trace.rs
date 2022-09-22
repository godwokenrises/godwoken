use anyhow::Result;
use gw_config::Trace;
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

    let registry = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .with(env_filter_layer);

    match trace {
        Some(Trace::Jaeger) => {
            opentelemetry::global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());

            let jaeger_layer = {
                let service_name =
                    std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "godwoken".into());
                let tracer = opentelemetry_jaeger::new_pipeline()
                    .with_service_name(service_name)
                    .with_auto_split_batch(true)
                    .install_batch(opentelemetry::runtime::Tokio)?;
                tracing_opentelemetry::layer().with_tracer(tracer)
            };

            registry.with(jaeger_layer).try_init()?
        }
        Some(Trace::TokioConsole) => {
            // Set up tokio console-subscriber. This should be used in **dev env** only.
            #[cfg(tokio_unstable)]
            {
                console_subscriber::init();
                log::info!("Tokio console_subscriber is enabled.");
            }
        }
        None => registry.try_init()?,
    }

    Ok(ShutdownGuard { trace })
}
