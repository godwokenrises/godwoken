use futures::stream::Stream;
use futures::StreamExt;
use once_cell::sync::OnceCell;
use opentelemetry::sdk::metrics::{selectors, PushController};
use opentelemetry_otlp::{ExportConfig, WithExportConfig};
use tokio::runtime::Runtime;

use std::time::Duration;

pub(crate) static THREAD_POOL: OnceCell<Runtime> = OnceCell::new();
//PushController will send `Shutdown` on `Drop`.
pub(crate) static PUSH_CONTROLLER: OnceCell<PushController> = OnceCell::new();

// Skip first immediate tick from tokio, not needed for async_std.
fn delayed_interval(duration: Duration) -> impl Stream<Item = tokio::time::Instant> {
    opentelemetry::util::tokio_interval_stream(duration).skip(1)
}

// Init opentelemeter by creating a tokio runtime globaly.
// For now opentelemetry metrics pipeline only supports grpc + tokio.
pub(crate) fn init_meter(endpoint: String, worker_threads: usize) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .thread_name("metrics-thread-pool")
        .worker_threads(worker_threads)
        .enable_all()
        .on_thread_start(|| {
            log::debug!("Metrics thread started!");
        })
        .on_thread_stop(|| {
            log::debug!("Metrics thread stoped!");
        })
        .build()
        .unwrap();
    let _gard = rt.handle().enter();
    let _ = THREAD_POOL.set(rt);

    let export_config = ExportConfig {
        endpoint,

        ..ExportConfig::default()
    };
    let res = opentelemetry_otlp::new_pipeline()
        .metrics(tokio::spawn, delayed_interval)
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_export_config(export_config),
        )
        .with_aggregator_selector(selectors::simple::Selector::Exact)
        .build()
        .map(|controller| {
            let _ = PUSH_CONTROLLER.set(controller);
        });
    if let Err(err) = res {
        log::error!("Metrics export setup failed: {:?}", err);
    }
}
