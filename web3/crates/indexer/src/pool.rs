use std::time::Duration;

use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions, PgPool,
};

use crate::config::load_indexer_config;

lazy_static::lazy_static! {
    pub static ref POOL: PgPool = {
        let indexer_config = load_indexer_config("./indexer-config.toml").unwrap();

        let mut opts: PgConnectOptions = indexer_config.pg_url.parse().expect("pg url parse error");
        opts.log_statements(log::LevelFilter::Debug)
            .log_slow_statements(log::LevelFilter::Warn, Duration::from_secs(5));
        PgPoolOptions::new()
            .max_connections(5)
            .connect_lazy_with(opts)
    };

    // Adapt slow query duration to 30s, and enlarge max connections
    pub static ref POOL_FOR_UPDATE: PgPool = {
        let indexer_config = load_indexer_config("./indexer-config.toml").unwrap();

        let mut opts: PgConnectOptions = indexer_config.pg_url.parse().expect("pg url parse error");
        opts.log_statements(log::LevelFilter::Debug)
            .log_slow_statements(log::LevelFilter::Warn, Duration::from_secs(30));
        PgPoolOptions::new()
            .max_connections(20)
            .connect_lazy_with(opts)
    };
}
