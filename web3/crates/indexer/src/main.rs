use gw_web3_indexer::{config::load_indexer_config, runner::Runner};
use std::env;

use anyhow::Result;

fn main() -> Result<()> {
    init_log();
    let indexer_config = load_indexer_config("./indexer-config.toml")?;

    if indexer_config.sentry_dsn.is_some() {
        log::warn!("Deprecated option: sentry_dsn");
    }

    let mut runner = Runner::new(indexer_config)?;

    let command_name = std::env::args().nth(1);

    // Supports SMOL_THREADS for backward compatibility.
    let threads = match env::var("SMOL_THREADS").or_else(|_| env::var("GODWOKEN_THREADS")) {
        Err(env::VarError::NotPresent) => num_cpus::get(),
        Err(e) => return Err(e.into()),
        Ok(v) => v.parse()?,
    };
    let blocking_threads = match env::var("GODWOKEN_BLOCKING_THREADS") {
        Err(env::VarError::NotPresent) => {
            // set blocking_threads to the number of CPUs (but at least 4). Our
            // blocking tasks are mostly CPU bound.
            std::cmp::max(4, threads)
        }
        Err(e) => return Err(e.into()),
        Ok(v) => v.parse()?,
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(threads)
        .max_blocking_threads(blocking_threads)
        .enable_all()
        .build()?;

    // `cargo run` -> run sync mode
    // `cargo run update <optional start number> <optional end number>` -> run update mode
    if let Some(name) = command_name {
        if name == "update" {
            let start_block_number = std::env::args()
                .nth(2)
                .map(|num| num.parse::<u64>().unwrap());
            let end_block_number = std::env::args()
                .nth(3)
                .map(|num| num.parse::<u64>().unwrap());
            rt.block_on(runner.run_update(start_block_number, end_block_number))?;
        } else {
            rt.block_on(runner.run())?;
        }
    } else {
        rt.block_on(runner.run())?;
    }

    Ok(())
}

fn init_log() {
    let logger = env_logger::builder()
        .parse_env(env_logger::Env::default().default_filter_or("info"))
        .build();
    let level = logger.filter();
    log::set_boxed_logger(Box::new(logger))
        .map(|()| log::set_max_level(level))
        .expect("set log");
}
