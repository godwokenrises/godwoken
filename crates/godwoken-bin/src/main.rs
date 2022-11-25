#[cfg(all(not(target_env = "msvc"), not(target_os = "macos")))]
#[global_allocator]
static GLOBAL_ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use anyhow::{Context, Result};
use clap::{Arg, Command, CommandFactory, Parser};
use godwoken_bin::subcommand::db_block_validator;
use godwoken_bin::subcommand::export_block::{ExportArgs, ExportBlock};
use godwoken_bin::subcommand::import_block::{ImportArgs, ImportBlock};
use godwoken_bin::subcommand::migrate::{MigrateCommand, COMMAND_MIGRATE};
use godwoken_bin::subcommand::peer_id::{PeerIdCommand, COMMAND_PEER_ID};
use godwoken_bin::subcommand::rewind_to_last_valid_block::{
    RewindToLastValidBlockCommand, COMMAND_REWIND_TO_LAST_VALID_BLOCK,
};
use gw_block_producer::{runner, trace};
use gw_config::{BackendForkConfig, Config};
use gw_version::Version;
use std::{env, fs, path::Path};

const COMMAND_RUN: &str = "run";
const COMMAND_EXAMPLE_CONFIG: &str = "generate-example-config";
const COMMAND_VERIFY_DB_BLOCK: &str = "verify-db-block";
const COMMAND_EXPORT_BLOCK: &str = "export-block";
const COMMAND_IMPORT_BLOCK: &str = "import-block";
const ARG_OUTPUT_PATH: &str = "output-path";
const ARG_CONFIG: &str = "config";
const ARG_SKIP_CONFIG_CHECK: &str = "skip-config-check";
const ARG_FROM_BLOCK: &str = "from-block";
const ARG_TO_BLOCK: &str = "to-block";
const ARG_SHOW_PROGRESS: &str = "show-progress";
const ARG_SOURCE_PATH: &str = "source-path";
const ARG_READ_BATCH: &str = "read-batch";
const ARG_REWIND_TO_LAST_VALID_TIP: &str = "rewind-to-last-valid-tip";

fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let content = fs::read(&path)
        .with_context(|| format!("read config file from {}", path.as_ref().to_string_lossy()))?;
    let config = toml::from_slice(&content).with_context(|| "parse config file")?;
    Ok(config)
}

fn generate_example_config<P: AsRef<Path>>(path: P) -> Result<()> {
    let mut config = Config::default();
    config.fork.backend_forks.push(BackendForkConfig {
        fork_height: 0,
        backends: Default::default(),
    });
    config.block_producer = Some(Default::default());
    let content = toml::to_string_pretty(&config)?;
    fs::write(path, content)?;
    Ok(())
}

// TODO: @zeroqn update clap to v3
async fn run_cli() -> Result<()> {
    let version = Version::current().to_string();
    let app = Command::new("Godwoken")
        .about("The layer2 rollup built upon Nervos CKB.")
        .version(version.as_ref())
        .subcommand(
            Command::new(COMMAND_RUN)
                .about("Run Godwoken node")
                .arg(
                    Arg::new(ARG_CONFIG)
                        .short('c')
                        .takes_value(true)
                        .required(true)
                        .default_value("./config.toml")
                        .help("The config file path"),
                )
                .arg(
                    Arg::new(ARG_SKIP_CONFIG_CHECK)
                        .long(ARG_SKIP_CONFIG_CHECK)
                        .help("Force to accept unsafe config file"),
                )
                .display_order(0),
        )
        .subcommand(
            Command::new(COMMAND_EXAMPLE_CONFIG)
                .about("Generate an example config file")
                .arg(
                    Arg::new(ARG_OUTPUT_PATH)
                        .short('o')
                        .takes_value(true)
                        .required(true)
                        .default_value("./config.example.toml")
                        .help("The path of the example config file"),
                )
                .display_order(1),
        )
        .subcommand(
            Command::new(COMMAND_VERIFY_DB_BLOCK)
                .about("Verify history blocks in db")
                .arg(
                    Arg::new(ARG_CONFIG)
                        .short('c')
                        .takes_value(true)
                        .required(true)
                        .default_value("./config.toml")
                        .help("The config file path"),
                )
                .arg(
                    Arg::new(ARG_FROM_BLOCK)
                        .short('f')
                        .takes_value(true)
                        .help("From block number"),
                )
                .arg(
                    Arg::new(ARG_TO_BLOCK)
                        .short('t')
                        .takes_value(true)
                        .help("To block number"),
                )
                .display_order(2),
        )
        .subcommand(
            Command::new(COMMAND_EXPORT_BLOCK)
                .about("Export history blocks in db")
                .arg(
                    Arg::new(ARG_CONFIG)
                        .short('c')
                        .takes_value(true)
                        .required(true)
                        .default_value("./config.toml")
                        .help("The config file path"),
                )
                .arg(
                    Arg::new(ARG_OUTPUT_PATH)
                        .short('o')
                        .long("output-path")
                        .takes_value(true)
                        .required(true)
                        .help("The output file for exported blocks"),
                )
                .arg(
                    Arg::new(ARG_FROM_BLOCK)
                        .short('f')
                        .long("from-block")
                        .takes_value(true)
                        .help("From block number"),
                )
                .arg(
                    Arg::new(ARG_TO_BLOCK)
                        .short('t')
                        .long("to-block")
                        .takes_value(true)
                        .help("To block number"),
                )
                .arg(
                    Arg::new(ARG_SHOW_PROGRESS)
                        .short('p')
                        .long("show-progress")
                        .required(false)
                        .takes_value(false)
                        .help("Show progress bar"),
                )
                .display_order(3),
        )
        .subcommand(
            Command::new(COMMAND_IMPORT_BLOCK)
                .about("Import block from source file")
                .arg(
                    Arg::new(ARG_CONFIG)
                        .short('c')
                        .takes_value(true)
                        .required(true)
                        .default_value("./config.toml")
                        .help("The config file path"),
                )
                .arg(
                    Arg::new(ARG_SOURCE_PATH)
                        .short('s')
                        .long("source-path")
                        .takes_value(true)
                        .required(true)
                        .help("The source file for exported blocks"),
                )
                .arg(
                    Arg::new(ARG_READ_BATCH)
                        .short('b')
                        .long("read-batch")
                        .takes_value(true)
                        .help("The read block batch size"),
                )
                .arg(
                    Arg::new(ARG_TO_BLOCK)
                        .short('t')
                        .long("to-block")
                        .takes_value(true)
                        .help("To block number"),
                )
                .arg(
                    Arg::new(ARG_REWIND_TO_LAST_VALID_TIP)
                        .long("rewind-to-last-valid-tip")
                        .required(false)
                        .takes_value(false)
                        .help("Rewind to last valid tip block before import"),
                )
                .arg(
                    Arg::new(ARG_SHOW_PROGRESS)
                        .short('p')
                        .long("show-progress")
                        .required(false)
                        .takes_value(false)
                        .help("Show progress bar"),
                )
                .display_order(4),
        )
        .subcommand(PeerIdCommand::command())
        .subcommand(RewindToLastValidBlockCommand::command())
        .subcommand(MigrateCommand::command());

    // handle subcommands
    let matches = app.clone().get_matches();
    match matches.subcommand() {
        Some((COMMAND_RUN, m)) => {
            let config_path = m.value_of(ARG_CONFIG).unwrap();
            let config = read_config(&config_path)?;
            let _guard = trace::init(config.trace)?;
            runner::run(config, m.is_present(ARG_SKIP_CONFIG_CHECK)).await?;
        }
        Some((COMMAND_EXAMPLE_CONFIG, m)) => {
            let path = m.value_of(ARG_OUTPUT_PATH).unwrap();
            let _guard = trace::init(None)?;
            generate_example_config(path)?;
        }
        Some((COMMAND_VERIFY_DB_BLOCK, m)) => {
            let config_path = m.value_of(ARG_CONFIG).unwrap();
            let config = read_config(&config_path)?;
            let _guard = trace::init(None)?;
            let from_block: Option<u64> = m.value_of(ARG_FROM_BLOCK).map(str::parse).transpose()?;
            let to_block: Option<u64> = m.value_of(ARG_TO_BLOCK).map(str::parse).transpose()?;
            db_block_validator::verify(config, from_block, to_block).await?;
        }
        Some((COMMAND_EXPORT_BLOCK, m)) => {
            let config_path = m.value_of(ARG_CONFIG).unwrap();
            let config = read_config(&config_path)?;
            let _guard = trace::init(None)?;
            let output = m.value_of(ARG_OUTPUT_PATH).unwrap().into();
            let from_block: Option<u64> = m.value_of(ARG_FROM_BLOCK).map(str::parse).transpose()?;
            let to_block: Option<u64> = m.value_of(ARG_TO_BLOCK).map(str::parse).transpose()?;
            let show_progress = m.is_present(ARG_SHOW_PROGRESS);

            let args = ExportArgs {
                config,
                output,
                from_block,
                to_block,
                show_progress,
            };
            ExportBlock::create(args)?.execute()?;
        }
        Some((COMMAND_IMPORT_BLOCK, m)) => {
            let config_path = m.value_of(ARG_CONFIG).unwrap();
            let config = read_config(&config_path)?;
            let _guard = trace::init(None)?;
            let source = m.value_of(ARG_SOURCE_PATH).unwrap().into();
            let read_batch: Option<usize> =
                m.value_of(ARG_READ_BATCH).map(str::parse).transpose()?;
            let to_block: Option<u64> = m.value_of(ARG_TO_BLOCK).map(str::parse).transpose()?;
            let rewind_to_last_valid_tip = m.is_present(ARG_REWIND_TO_LAST_VALID_TIP);
            let show_progress = m.is_present(ARG_SHOW_PROGRESS);

            let args = ImportArgs {
                config,
                source,
                read_batch,
                to_block,
                rewind_to_last_valid_tip,
                show_progress,
            };
            ImportBlock::create(args).await?.execute().await?;
        }
        Some((COMMAND_PEER_ID, m)) => {
            PeerIdCommand::from_clap(m).run()?;
        }
        Some((COMMAND_REWIND_TO_LAST_VALID_BLOCK, m)) => {
            RewindToLastValidBlockCommand::from_clap(m).run().await?;
        }
        Some((COMMAND_MIGRATE, m)) => {
            MigrateCommand::from_clap(m).run()?;
        }
        _ => {
            // default command: start a Godwoken node
            let config_path = "./config.toml";
            let config = read_config(&config_path)?;
            let _guard = trace::init(config.trace)?;
            runner::run(config, false).await?;
        }
    };
    Ok(())
}

/// Godwoken entry
fn main() -> Result<()> {
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

    rt.block_on(run_cli())
}
