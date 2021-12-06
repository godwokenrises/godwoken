mod replay;

use replay::*;

use anyhow::{anyhow, Context, Result};
use clap::{App, Arg, SubCommand};
use gw_config::Config;
use std::{fs, path::Path};

const ARG_CONFIG: &str = "config";

fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let content = fs::read(&path)
        .with_context(|| format!("read config file from {}", path.as_ref().to_string_lossy()))?;
    let config = toml::from_slice(&content).with_context(|| "parse config file")?;
    Ok(config)
}

fn run_cli() -> Result<()> {
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
            let to_db_store = m.value_of("to-db-store").unwrap().into();
            let args = ReplayArgs {
                config,
                from_db_store,
                to_db_store,
            };
            replay(args).expect("replay");
        }
        _ => {
            return Err(anyhow!("unknown command"));
        }
    };
    Ok(())
}

fn main() {
    run_cli().expect("cli");
}
