#[allow(unused_variables)]
pub fn init_meter(endpoint: String, worker_threads: usize) {
    #[cfg(feature = "metrics")]
    crate::opentelemeter::config::init_meter(endpoint, worker_threads);
    #[cfg(not(feature = "metrics"))]
    log::debug!("Metrics feature is not enabled.");
}
