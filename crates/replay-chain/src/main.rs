mod replay;
mod setup;

use replay::*;

use anyhow::{anyhow, Context, Result};
use clap::{App, Arg, SubCommand};
use gw_config::Config;
use gw_db::schema::COLUMNS;
use setup::{setup, SetupArgs};
use std::{fs, path::Path};

const ARG_CONFIG: &str = "config";

fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let content = fs::read(&path)
        .with_context(|| format!("read config file from {}", path.as_ref().to_string_lossy()))?;
    let config = toml::from_slice(&content).with_context(|| "parse config file")?;
    Ok(config)
}

async fn run_cli() -> Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    let app = App::new("gw-chain-replay")
        .about("The layer2 rollup built upon Nervos CKB.")
        .subcommand(
            SubCommand::with_name("replay")
                .about("Replay chain")
                .arg(
                    Arg::with_name(ARG_CONFIG)
                        .short("c")
                        .takes_value(true)
                        .required(true)
                        .default_value("./config.toml")
                        .help("The config file path"),
                )
                .arg(
                    Arg::with_name("from-db-store")
                        .long("from-db-store")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("from-db-columns")
                        .long("from-db-columns")
                        .takes_value(true)
                        .required(false),
                )
                .arg(
                    Arg::with_name("to-db-store")
                        .long("to-db-store")
                        .takes_value(true)
                        .required(true),
                )
                .display_order(0),
        )
        .subcommand(
            SubCommand::with_name("detach")
                .about("Detach chain")
                .arg(
                    Arg::with_name(ARG_CONFIG)
                        .short("c")
                        .takes_value(true)
                        .required(true)
                        .default_value("./config.toml")
                        .help("The config file path"),
                )
                .arg(
                    Arg::with_name("from-db-store")
                        .long("from-db-store")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("from-db-columns")
                        .long("from-db-columns")
                        .takes_value(true)
                        .required(false),
                )
                .arg(
                    Arg::with_name("to-db-store")
                        .long("to-db-store")
                        .takes_value(true)
                        .required(true),
                )
                .display_order(0),
        );

    // handle subcommands
    let matches = app.clone().get_matches();
    match matches.subcommand() {
        ("replay", Some(m)) => {
            let config_path = m.value_of(ARG_CONFIG).unwrap();
            let config = read_config(&config_path)?;
            let from_db_store = m.value_of("from-db-store").unwrap().into();
            let from_db_columns = m
                .value_of("from-db-columns")
                .map(|s| s.parse())
                .transpose()?
                .unwrap_or(COLUMNS);
            let to_db_store = m.value_of("to-db-store").unwrap().into();
            let args = SetupArgs {
                config,
                from_db_store,
                to_db_store,
                from_db_columns,
            };
            let context = setup(args).await.expect("setup");
            replay_chain(context).expect("replay");
        }
        ("detach", Some(m)) => {
            let config_path = m.value_of(ARG_CONFIG).unwrap();
            let config = read_config(&config_path)?;
            let from_db_store = m.value_of("from-db-store").unwrap().into();
            let from_db_columns = m
                .value_of("from-db-columns")
                .map(|s| s.parse())
                .transpose()?
                .unwrap_or(COLUMNS);
            let to_db_store = m.value_of("to-db-store").unwrap().into();
            let args = SetupArgs {
                config,
                from_db_store,
                to_db_store,
                from_db_columns,
            };
            let context = setup(args).await.expect("setup");
            detach_chain(context).expect("detach");
        }
        _ => {
            return Err(anyhow!("unknown command"));
        }
    };
    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    run_cli().await
}
