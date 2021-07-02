use anyhow::{Context, Result};
use clap::{crate_version, App, Arg, SubCommand};
use gw_block_producer::runner;
use gw_config::Config;
use std::{fs, path::Path};

const COMMAND_RUN: &str = "run";
const COMMAND_EXAMPLE_CONFIG: &str = "generate-example-config";
const ARG_OUTPUT_PATH: &str = "output-path";
const ARG_CONFIG: &str = "config";
const ARG_SKIP_CONFIG_CHECK: &str = "skip-config-check";

fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let content = fs::read(&path)
        .with_context(|| format!("read config file from {}", path.as_ref().to_string_lossy()))?;
    let config = toml::from_slice(&content).with_context(|| "parse config file")?;
    Ok(config)
}

fn generate_example_config<P: AsRef<Path>>(path: P) -> Result<()> {
    let mut config = Config::default();
    config.backends.push(Default::default());
    config.block_producer = Some(Default::default());
    let content = toml::to_string_pretty(&config)?;
    fs::write(path, content)?;
    Ok(())
}

fn run_cli() -> Result<()> {
    let app = App::new("Godwoken")
        .about("The layer2 rollup built upon Nervos CKB.")
        .version(crate_version!())
        .subcommand(
            SubCommand::with_name(COMMAND_RUN)
                .about("Run Godwoken node")
                .arg(
                    Arg::with_name(ARG_CONFIG)
                        .short("c")
                        .takes_value(true)
                        .required(true)
                        .default_value("./config.toml")
                        .help("The config file path"),
                )
                .arg(
                    Arg::with_name(ARG_SKIP_CONFIG_CHECK)
                        .long(ARG_SKIP_CONFIG_CHECK)
                        .help("Force to accept unsafe config file"),
                )
                .display_order(0),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_EXAMPLE_CONFIG)
                .about("Generate an example config file")
                .arg(
                    Arg::with_name(ARG_OUTPUT_PATH)
                        .short("o")
                        .takes_value(true)
                        .required(true)
                        .default_value("./config.example.toml")
                        .help("The path of the example config file"),
                )
                .display_order(1),
        );

    // handle subcommands
    let matches = app.clone().get_matches();
    match matches.subcommand() {
        (COMMAND_RUN, Some(m)) => {
            let config_path = m.value_of(ARG_CONFIG).unwrap();
            let config = read_config(&config_path)?;
            runner::run(config, m.is_present(ARG_SKIP_CONFIG_CHECK))?;
        }
        (COMMAND_EXAMPLE_CONFIG, Some(m)) => {
            let path = m.value_of(ARG_OUTPUT_PATH).unwrap();
            generate_example_config(path)?;
        }
        _ => {
            // default command: start a Godwoken node
            let config_path = "./config.toml";
            let config = read_config(&config_path)?;
            runner::run(config, false)?;
        }
    };
    Ok(())
}

/// Godwoken entry
fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    run_cli().expect("run cli");
}
