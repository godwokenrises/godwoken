pub mod context;
pub mod layer1;
pub mod rollup;
pub mod rollup_config;

pub fn init_env_log() {
    let _ = env_logger::builder().is_test(true).try_init();
}
