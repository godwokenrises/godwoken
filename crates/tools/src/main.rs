mod account;
mod address;
mod create_creator_account;
mod deploy_genesis;
mod deploy_scripts;
mod deposit_ckb;
mod dump_tx;
mod generate_config;
mod get_balance;
pub mod godwoken_rpc;
mod hasher;
mod polyjuice;
mod prepare_scripts;
mod setup;
mod transfer;
mod utils;
mod withdraw;

use clap::{value_t, App, Arg, SubCommand};
use dump_tx::ChallengeBlock;
use gw_jsonrpc_types::godwoken::ChallengeTargetType;
use std::{path::Path, str::FromStr};

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let arg_privkey_path = Arg::with_name("privkey-path")
        .short("k")
        .takes_value(true)
        .required(true)
        .help("The private key file path");
    let arg_ckb_rpc = Arg::with_name("ckb-rpc-url")
        .short("r")
        .takes_value(true)
        .default_value("http://127.0.0.1:8114")
        .help("CKB jsonrpc rpc sever URL");
    let arg_deployment_results_path = Arg::with_name("deployment-results-path")
        .short("d")
        .long("deployment-results-path")
        .takes_value(true)
        .required(true)
        .help("The deployment results json file path");
    let arg_config_path = Arg::with_name("config-path")
        .short("o")
        .long("config-path")
        .takes_value(true)
        .required(true)
        .help("The config.toml file path");
    let arg_godwoken_rpc_url = Arg::with_name("godwoken-rpc-url")
        .short("g")
        .long("godwoken-rpc-url")
        .takes_value(true)
        .default_value("http://127.0.0.1:8119")
        .help("Godwoken jsonrpc rpc sever URL");

    let mut app = App::new("godwoken tools")
        .about("Godwoken cli tools")
        .subcommand(
            SubCommand::with_name("deploy-scripts")
                .about("Deploy scripts used by godwoken")
                .arg(arg_privkey_path.clone())
                .arg(arg_ckb_rpc.clone())
                .arg(
                    Arg::with_name("input-path")
                        .short("i")
                        .takes_value(true)
                        .required(true)
                        .help("The input json file path"),
                )
                .arg(
                    Arg::with_name("output-path")
                        .short("o")
                        .takes_value(true)
                        .required(true)
                        .help("The output json file path"),
                ),
        )
        .subcommand(
            SubCommand::with_name("deploy-genesis")
                .about("Deploy genesis block of godwoken")
                .arg(arg_privkey_path.clone())
                .arg(arg_ckb_rpc.clone())
                .arg(
                    Arg::with_name("deployment-results-path")
                        .short("d")
                        .takes_value(true)
                        .required(true)
                        .help("The deployment results json file path"),
                )
                .arg(
                    Arg::with_name("genesis-timestamp")
                        .short("t")
                        .takes_value(true)
                        .required(false)
                        .help("Genesis timestamp in milliseconds"),
                )
                .arg(
                    Arg::with_name("user-rollup-config-path")
                        .short("u")
                        .takes_value(true)
                        .required(true)
                        .help("The user rollup config json file path"),
                )
                .arg(
                    Arg::with_name("poa-config-path")
                        .short("p")
                        .takes_value(true)
                        .required(true)
                        .help("The poa config json file path"),
                )
                .arg(
                    Arg::with_name("output-path")
                        .short("o")
                        .takes_value(true)
                        .required(true)
                        .help("The output json file path"),
                )
                .arg(
                    Arg::with_name("skip-config-check")
                        .long("skip-config-check")
                        .help("Force to accept unsafe config file"),
                ),
        )
        .subcommand(
            SubCommand::with_name("generate-config")
                .about("Generate configure")
                .arg(arg_ckb_rpc.clone())
                .arg(
                    Arg::with_name("indexer-rpc-url")
                        .short("i")
                        .takes_value(true)
                        .default_value("http://127.0.0.1:8116")
                        .required(true)
                        .help("The URL of ckb indexer"),
                )
                .arg(
                    Arg::with_name("scripts-deployment-results-path")
                        .short("s")
                        .takes_value(true)
                        .required(true)
                        .help("Scripts deployment results json file path"),
                )
                .arg(
                    Arg::with_name("genesis-deployment-results-path")
                        .short("g")
                        .takes_value(true)
                        .required(true)
                        .help("The genesis deployment results json file path"),
                )
                .arg(arg_privkey_path.clone())
                .arg(
                    Arg::with_name("database-url")
                        .short("d")
                        .takes_value(true)
                        .help("The web3 store database url"),
                )
                .arg(
                    Arg::with_name("output-path")
                        .short("o")
                        .takes_value(true)
                        .required(true)
                        .help("The output json file path"),
                )
                .arg(
                    Arg::with_name("scripts-deployment-config-path")
                        .short("c")
                        .takes_value(true)
                        .required(true)
                        .help("Scripts deployment config json file path"),
                )
                .arg(
                    Arg::with_name("rpc-server-url")
                        .short("u")
                        .takes_value(true)
                        .default_value("localhost:8119")
                        .required(true)
                        .help("The URL of rpc server"),
                ),
        )
        .subcommand(
            SubCommand::with_name("prepare-scripts")
                .about("Prepare scripts used by godwoken")
                .arg(
                    Arg::with_name("mode")
                        .short("m")
                        .takes_value(true)
                        .default_value("build")
                        .required(true)
                        .help("Scripts generation mode: build or copy"),
                )
                .arg(
                    Arg::with_name("input-path")
                        .short("i")
                        .takes_value(true)
                        .required(true)
                        .help("The input json file path"),
                )
                .arg(
                    Arg::with_name("repos-dir-path")
                        .short("r")
                        .takes_value(true)
                        .default_value(prepare_scripts::REPOS_DIR_PATH)
                        .required(true)
                        .help("The repos dir path"),
                )
                .arg(
                    Arg::with_name("scripts-dir-path")
                        .short("s")
                        .takes_value(true)
                        .default_value(prepare_scripts::SCRIPTS_DIR_PATH)
                        .required(true)
                        .help("Scripts directory path"),
                )
                .arg(
                    Arg::with_name("output-path")
                        .short("o")
                        .takes_value(true)
                        .required(true)
                        .help("The output scripts deploy json file path"),
                ),
        )
        .subcommand(
            SubCommand::with_name("deposit-ckb")
                .about("Deposit CKB to godwoken")
                .arg(arg_ckb_rpc.clone())
                .arg(arg_privkey_path.clone())
                .arg(arg_deployment_results_path.clone())
                .arg(arg_config_path.clone())
                .arg(arg_godwoken_rpc_url.clone())
                .arg(
                    Arg::with_name("capacity")
                        .short("c")
                        .long("capacity")
                        .takes_value(true)
                        .required(true)
                        .help("CKB capacity to deposit"),
                )
                .arg(
                    Arg::with_name("eth-address")
                        .short("e")
                        .long("eth-address")
                        .takes_value(true)
                        .required(false)
                        .help("Target eth address, calculated by private key in default"),
                )
                .arg(
                    Arg::with_name("fee")
                        .short("f")
                        .long("fee")
                        .takes_value(true)
                        .required(false)
                        .default_value("0.0001")
                        .help("Transaction fee, default to 0.0001 CKB"),
                ),
        )
        .subcommand(
            SubCommand::with_name("withdraw")
                .about("withdraw CKB / sUDT from godwoken")
                .arg(arg_privkey_path.clone())
                .arg(arg_deployment_results_path.clone())
                .arg(arg_config_path.clone())
                .arg(arg_godwoken_rpc_url.clone())
                .arg(
                    Arg::with_name("capacity")
                        .short("c")
                        .long("capacity")
                        .takes_value(true)
                        .required(true)
                        .help("CKB capacity to withdrawal"),
                )
                .arg(
                    Arg::with_name("amount")
                        .short("m")
                        .long("amount")
                        .takes_value(true)
                        .default_value("0")
                        .help("sUDT amount to withdrawal"),
                )
                .arg(
                    Arg::with_name("owner-ckb-address")
                        .short("a")
                        .long("owner-ckb-address")
                        .takes_value(true)
                        .required(true)
                        .help("owner ckb address (to)"),
                )
                .arg(
                    Arg::with_name("sudt-script-hash")
                        .short("s")
                        .long("sudt-script-hash")
                        .takes_value(true)
                        .required(false)
                        .default_value(
                            "0x0000000000000000000000000000000000000000000000000000000000000000",
                        )
                        .help("l1 sudt script hash, default for withdrawal CKB"),
                ),
        )
        .subcommand(
            SubCommand::with_name("setup")
                .about("Prepare scripts, deploy scripts, setup nodes, deploy genesis and generate configs")
                .arg(arg_ckb_rpc.clone())
                .arg(
                    Arg::with_name("indexer-rpc-url")
                        .short("i")
                        .takes_value(true)
                        .default_value("http://127.0.0.1:8116")
                        .required(true)
                        .help("The URL of ckb indexer"),
                )
                .arg(
                    Arg::with_name("mode")
                        .short("m")
                        .takes_value(true)
                        .default_value("build")
                        .required(true)
                        .help("Scripts generation mode: build or copy"),
                )
                .arg(
                    Arg::with_name("scripts-build-file-path")
                        .short("s")
                        .takes_value(true)
                        .required(true)
                        .help("The scripts build json file path"),
                )
                .arg(arg_privkey_path.clone())
                .arg(
                    Arg::with_name("nodes-count")
                        .short("n")
                        .takes_value(true)
                        .default_value("2")
                        .required(true)
                        .help("The godwoken nodes count"),
                )
                .arg(
                    Arg::with_name("rpc-server-url")
                        .short("u")
                        .takes_value(true)
                        .default_value("localhost:8119")
                        .required(true)
                        .help("The URL of rpc server"),
                )
                .arg(
                    Arg::with_name("output-dir-path")
                        .short("o")
                        .takes_value(true)
                        .default_value("deploy/")
                        .required(true)
                        .help("The godwoken nodes configs output dir path"),
                ),
        )
        .subcommand(
            SubCommand::with_name("transfer")
                .about("transfer CKB / sUDT to another account")
                .arg(arg_privkey_path.clone())
                .arg(arg_deployment_results_path.clone())
                .arg(arg_config_path.clone())
                .arg(arg_godwoken_rpc_url.clone())
                .arg(
                    Arg::with_name("amount")
                        .short("m")
                        .long("amount")
                        .takes_value(true)
                        .default_value("0")
                        .help("sUDT amount to transfer, CKB in shannon"),
                )
                .arg(
                    Arg::with_name("fee")
                        .short("f")
                        .long("fee")
                        .takes_value(true)
                        .required(true)
                        .help("transfer fee"),
                )
                .arg(
                    Arg::with_name("to")
                        .short("t")
                        .long("to")
                        .takes_value(true)
                        .required(true)
                        .help("to short address OR to account id"),
                )
                .arg(
                    Arg::with_name("sudt-id")
                        .short("s")
                        .long("sudt-id")
                        .takes_value(true)
                        .required(true)
                        .help("sudt id"),
                ),
        )
        .subcommand(
            SubCommand::with_name("create-creator-account")
                .about("Create polyjuice contract account")
                .arg(arg_privkey_path.clone())
                .arg(arg_deployment_results_path.clone())
                .arg(arg_config_path.clone())
                .arg(arg_godwoken_rpc_url.clone())
                .arg(
                    Arg::with_name("fee")
                        .short("f")
                        .long("fee")
                        .takes_value(true)
                        .required(false)
                        .default_value("0")
                        .help("transfer fee"),
                )
                .arg(
                    Arg::with_name("sudt-id")
                        .short("s")
                        .long("sudt-id")
                        .takes_value(true)
                        .required(false)
                        .default_value("1")
                        .help("sudt id"),
                ),
        )
        .subcommand(
            SubCommand::with_name("get-balance")
                .about("Get balance")
                .arg(arg_godwoken_rpc_url.clone())
                .arg(
                    Arg::with_name("account")
                        .short("a")
                        .long("account")
                        .takes_value(true)
                        .help("short address OR account id"),
                )
                .arg(
                    Arg::with_name("sudt-id")
                        .short("s")
                        .long("sudt-id")
                        .takes_value(true)
                        .required(false)
                        .default_value("1")
                        .help("sudt id"),
                ),
        )
        .subcommand(
            SubCommand::with_name("polyjuice-deploy")
                .about("Deploy a EVM contract")
                .arg(arg_godwoken_rpc_url.clone())
                .arg(arg_privkey_path.clone())
                .arg(arg_config_path.clone())
                .arg(arg_deployment_results_path.clone())
                .arg(
                    Arg::with_name("creator-account-id")
                        .short("c")
                        .long("creator-account-id")
                        .takes_value(true)
                        .required(true)
                        .help("creator account id"),
                )
                .arg(
                    Arg::with_name("gas-limit")
                        .short("l")
                        .long("gas-limit")
                        .takes_value(true)
                        .required(true)
                        .help("gas limit"),
                )
                .arg(
                    Arg::with_name("gas-price")
                        .short("p")
                        .long("gas-price")
                        .takes_value(true)
                        .required(true)
                        .help("gas price"),
                )
                .arg(
                    Arg::with_name("data")
                        .short("a")
                        .long("data")
                        .takes_value(true)
                        .required(true)
                        .help("data"),
                )
                .arg(
                    Arg::with_name("value")
                        .short("v")
                        .long("value")
                        .takes_value(true)
                        .required(false)
                        .default_value("0")
                        .help("value"),
                ),
        )
        .subcommand(
            SubCommand::with_name("polyjuice-send")
                .about("Send a transaction to godwoken by `eth_sendRawTransaction`")
                .arg(arg_godwoken_rpc_url.clone())
                .arg(arg_privkey_path.clone())
                .arg(arg_config_path.clone())
                .arg(arg_deployment_results_path.clone())
                .arg(
                    Arg::with_name("creator-account-id")
                        .short("c")
                        .long("creator-account-id")
                        .takes_value(true)
                        .required(true)
                        .help("creator account id"),
                )
                .arg(
                    Arg::with_name("gas-limit")
                        .short("l")
                        .long("gas-limit")
                        .takes_value(true)
                        .required(true)
                        .help("gas limit"),
                )
                .arg(
                    Arg::with_name("gas-price")
                        .short("p")
                        .long("gas-price")
                        .takes_value(true)
                        .required(true)
                        .help("gas price"),
                )
                .arg(
                    Arg::with_name("data")
                        .short("a")
                        .long("data")
                        .takes_value(true)
                        .required(true)
                        .help("data"),
                )
                .arg(
                    Arg::with_name("to-address")
                        .short("t")
                        .long("to-address")
                        .takes_value(true)
                        .required(true)
                        .help("to eth address"),
                )
                .arg(
                    Arg::with_name("value")
                        .short("v")
                        .long("value")
                        .takes_value(true)
                        .required(false)
                        .default_value("0")
                        .help("value"),
                ),
        )
        .subcommand(
            SubCommand::with_name("polyjuice-call")
                .about("Static Call a EVM contract by `eth_call`")
                .arg(arg_godwoken_rpc_url.clone())
                .arg(
                    Arg::with_name("from")
                        .short("f")
                        .long("from")
                        .takes_value(true)
                        .required(true)
                        .help("from address OR from id"),
                )
                .arg(
                    Arg::with_name("gas-limit")
                        .short("l")
                        .long("gas-limit")
                        .takes_value(true)
                        .required(false)
                        .default_value("16777216")
                        .help("gas limit"),
                )
                .arg(
                    Arg::with_name("gas-price")
                        .short("p")
                        .long("gas-price")
                        .takes_value(true)
                        .required(false)
                        .default_value("1")
                        .help("gas price"),
                )
                .arg(
                    Arg::with_name("data")
                        .short("a")
                        .long("data")
                        .takes_value(true)
                        .required(true)
                        .help("data"),
                )
                .arg(
                    Arg::with_name("value")
                        .short("v")
                        .long("value")
                        .takes_value(true)
                        .required(false)
                        .default_value("0")
                        .help("value"),
                )
                .arg(
                    Arg::with_name("to-address")
                        .short("t")
                        .long("to-address")
                        .takes_value(true)
                        .required(true)
                        .help("to eth address"),
                ),
        )
        .subcommand(
            SubCommand::with_name("to-short-address")
                .about("Eth eoa address to godwoken short address")
                .arg(arg_config_path.clone())
                .arg(arg_deployment_results_path.clone())
                .arg(
                    Arg::with_name("eth-address")
                        .short("a")
                        .long("eth-address")
                        .takes_value(true)
                        .help("eth eoa address"),
                ),
        )
        .subcommand(
            SubCommand::with_name("to-eth-address")
                .about("Godwoken short address to eth eoa address")
                .arg(arg_godwoken_rpc_url.clone())
                .arg(
                    Arg::with_name("short-address")
                        .short("a")
                        .long("short-address")
                        .takes_value(true)
                        .help("godwoken short address"),
                ),
        )
        .subcommand(
            SubCommand::with_name("dump-tx")
                .about("Dump offchain cancel challenge tx")
                .arg(arg_godwoken_rpc_url.clone())
                .arg(
                    Arg::with_name("block")
                        .short("b")
                        .long("block")
                        .takes_value(true)
                        .required(true)
                        .help("challenge block"),
                )
                .arg(
                    Arg::with_name("index")
                        .short("i")
                        .long("index")
                        .takes_value(true)
                        .required(true)
                        .help("challenge target index"),
                )
                .arg(
                    Arg::with_name("type")
                        .short("t")
                        .long("type")
                        .takes_value(true)
                        .required(true)
                        .possible_values(&["tx_execution", "tx_signature", "withdrawal"])
                        .help("challenge target type"),
                )
                .arg(
                    Arg::with_name("output")
                        .short("o")
                        .long("output")
                        .takes_value(true)
                        .required(true)
                        .help("output file"),
                ),
        );

    let matches = app.clone().get_matches();
    match matches.subcommand() {
        ("deploy-scripts", Some(m)) => {
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let ckb_rpc_url = m.value_of("ckb-rpc-url").unwrap();
            let input_path = Path::new(m.value_of("input-path").unwrap());
            let output_path = Path::new(m.value_of("output-path").unwrap());
            if let Err(err) = deploy_scripts::deploy_scripts(
                &privkey_path,
                ckb_rpc_url,
                &input_path,
                &output_path,
            ) {
                log::error!("Deploy scripts error: {}", err);
                std::process::exit(-1);
            };
        }
        ("deploy-genesis", Some(m)) => {
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let ckb_rpc_url = m.value_of("ckb-rpc-url").unwrap();
            let deployment_results_path = Path::new(m.value_of("deployment-results-path").unwrap());
            let user_rollup_path = Path::new(m.value_of("user-rollup-config-path").unwrap());
            let poa_config_path = Path::new(m.value_of("poa-config-path").unwrap());
            let output_path = Path::new(m.value_of("output-path").unwrap());
            let timestamp = m
                .value_of("genesis-timestamp")
                .map(|s| s.parse().expect("timestamp in milliseconds"));
            if let Err(err) = deploy_genesis::deploy_genesis(
                &privkey_path,
                ckb_rpc_url,
                &deployment_results_path,
                &user_rollup_path,
                &poa_config_path,
                timestamp,
                &output_path,
                m.is_present("skip-config-check"),
            ) {
                log::error!("Deploy genesis error: {}", err);
                std::process::exit(-1);
            };
        }
        ("generate-config", Some(m)) => {
            let ckb_url = m.value_of("ckb-rpc-url").unwrap().to_string();
            let indexer_url = m.value_of("indexer-rpc-url").unwrap().to_string();
            let scripts_results_path =
                Path::new(m.value_of("scripts-deployment-results-path").unwrap());
            let genesis_path = Path::new(m.value_of("genesis-deployment-results-path").unwrap());
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let output_path = Path::new(m.value_of("output-path").unwrap());
            let database_url = m.value_of("database-url");
            let scripts_config_path =
                Path::new(m.value_of("scripts-deployment-config-path").unwrap());
            let server_url = m.value_of("rpc-server-url").unwrap().to_string();

            if let Err(err) = generate_config::generate_config(
                genesis_path,
                scripts_results_path,
                privkey_path,
                ckb_url,
                indexer_url,
                output_path,
                database_url,
                scripts_config_path,
                server_url,
            ) {
                log::error!("Generate config error: {}", err);
                std::process::exit(-1);
            };
        }
        ("prepare-scripts", Some(m)) => {
            let mode = value_t!(m, "mode", prepare_scripts::ScriptsBuildMode).unwrap();
            let input_path = Path::new(m.value_of("input-path").unwrap());
            let repos_dir = Path::new(m.value_of("repos-dir-path").unwrap());
            let scripts_dir = Path::new(m.value_of("scripts-dir-path").unwrap());
            let output_path = Path::new(m.value_of("output-path").unwrap());
            if let Err(err) = prepare_scripts::prepare_scripts(
                mode,
                input_path,
                repos_dir,
                scripts_dir,
                output_path,
            ) {
                log::error!("Prepare scripts error: {}", err);
                std::process::exit(-1);
            };
        }
        ("deposit-ckb", Some(m)) => {
            let ckb_rpc_url = m.value_of("ckb-rpc-url").unwrap().to_string();
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let capacity = m.value_of("capacity").unwrap();
            let fee = m.value_of("fee").unwrap();
            let eth_address = m.value_of("eth-address");
            let deployment_results_path = Path::new(m.value_of("deployment-results-path").unwrap());
            let config_path = Path::new(m.value_of("config-path").unwrap());
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();

            if let Err(err) = deposit_ckb::deposit_ckb(
                privkey_path,
                deployment_results_path,
                config_path,
                capacity,
                fee,
                ckb_rpc_url.as_str(),
                eth_address,
                godwoken_rpc_url,
            ) {
                log::error!("Deposit CKB error: {}", err);
                std::process::exit(-1);
            };
        }
        ("withdraw", Some(m)) => {
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let capacity = m.value_of("capacity").unwrap();
            let amount = m.value_of("amount").unwrap();
            let deployment_results_path = Path::new(m.value_of("deployment-results-path").unwrap());
            let config_path = Path::new(m.value_of("config-path").unwrap());
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();
            let owner_ckb_address = m.value_of("owner-ckb-address").unwrap();
            let sudt_script_hash = m.value_of("sudt-script-hash").unwrap();

            if let Err(err) = withdraw::withdraw(
                godwoken_rpc_url,
                privkey_path,
                capacity,
                amount,
                sudt_script_hash,
                owner_ckb_address,
                config_path,
                deployment_results_path,
            ) {
                log::error!("Withdrawal error: {}", err);
                std::process::exit(-1);
            };
        }
        ("setup", Some(m)) => {
            let ckb_rpc_url = m.value_of("ckb-rpc-url").unwrap();
            let indexer_url = m.value_of("indexer-rpc-url").unwrap();
            let mode = value_t!(m, "mode", prepare_scripts::ScriptsBuildMode).unwrap();
            let scripts_path = Path::new(m.value_of("scripts-build-file-path").unwrap());
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let nodes_count = m
                .value_of("nodes-count")
                .map(|c| c.parse().expect("nodes count"))
                .unwrap();
            let server_url = m.value_of("rpc-server-url").unwrap();
            let output_dir = Path::new(m.value_of("output-dir-path").unwrap());
            setup::setup(
                ckb_rpc_url,
                indexer_url,
                mode,
                scripts_path,
                privkey_path,
                nodes_count,
                server_url,
                output_dir,
            );
        }
        ("transfer", Some(m)) => {
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let amount = m.value_of("amount").unwrap();
            let fee = m.value_of("fee").unwrap();
            let deployment_results_path = Path::new(m.value_of("deployment-results-path").unwrap());
            let config_path = Path::new(m.value_of("config-path").unwrap());
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();
            let to = m.value_of("to").unwrap();
            let sudt_id = m
                .value_of("sudt-id")
                .unwrap()
                .parse()
                .expect("sudt id format error");

            if let Err(err) = transfer::transfer(
                godwoken_rpc_url,
                privkey_path,
                to.trim(),
                sudt_id,
                amount,
                fee,
                config_path,
                deployment_results_path,
            ) {
                log::error!("Transfer error: {}", err);
                std::process::exit(-1);
            };
        }
        ("create-creator-account", Some(m)) => {
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let fee = m.value_of("fee").unwrap();
            let deployment_results_path = Path::new(m.value_of("deployment-results-path").unwrap());
            let config_path = Path::new(m.value_of("config-path").unwrap());
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();
            let sudt_id = m
                .value_of("sudt-id")
                .unwrap()
                .parse()
                .expect("sudt id format error");

            if let Err(err) = create_creator_account::create_creator_account(
                godwoken_rpc_url,
                privkey_path,
                sudt_id,
                fee,
                config_path,
                deployment_results_path,
            ) {
                log::error!("Create creator account error: {}", err);
                std::process::exit(-1);
            };
        }
        ("get-balance", Some(m)) => {
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();
            let account = m.value_of("account").unwrap();
            let sudt_id = m
                .value_of("sudt-id")
                .unwrap()
                .parse()
                .expect("sudt id format error");

            if let Err(err) = get_balance::get_balance(godwoken_rpc_url, account, sudt_id) {
                log::error!("Get balance error: {}", err);
                std::process::exit(-1);
            };
        }
        ("polyjuice-deploy", Some(m)) => {
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let deployment_results_path = Path::new(m.value_of("deployment-results-path").unwrap());
            let config_path = Path::new(m.value_of("config-path").unwrap());
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();

            let data = m.value_of("data").unwrap();
            let gas_price = m
                .value_of("gas-price")
                .unwrap()
                .parse()
                .expect("gas price format error");
            let gas_limit = m
                .value_of("gas-limit")
                .unwrap()
                .parse()
                .expect("gas limit format error");
            let creator_account_id = m
                .value_of("creator-account-id")
                .unwrap()
                .parse()
                .expect("creator account id format error");
            let value = m
                .value_of("value")
                .unwrap()
                .parse()
                .expect("value format error");

            if let Err(err) = polyjuice::deploy(
                godwoken_rpc_url,
                config_path,
                deployment_results_path,
                privkey_path,
                creator_account_id,
                gas_limit,
                gas_price,
                data,
                value,
            ) {
                log::error!("Polyjuice deploy error: {}", err);
                std::process::exit(-1);
            };
        }
        ("polyjuice-send", Some(m)) => {
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let deployment_results_path = Path::new(m.value_of("deployment-results-path").unwrap());
            let config_path = Path::new(m.value_of("config-path").unwrap());
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();

            let data = m.value_of("data").unwrap();
            let gas_price = m
                .value_of("gas-price")
                .unwrap()
                .parse()
                .expect("gas price format error");
            let gas_limit = m
                .value_of("gas-limit")
                .unwrap()
                .parse()
                .expect("gas limit format error");
            let creator_account_id = m
                .value_of("creator-account-id")
                .unwrap()
                .parse()
                .expect("creator account id format error");
            let value = m
                .value_of("value")
                .unwrap()
                .parse()
                .expect("value format error");
            let to_address = m.value_of("to-address").unwrap();

            if let Err(err) = polyjuice::send_transaction(
                godwoken_rpc_url,
                config_path,
                deployment_results_path,
                privkey_path,
                creator_account_id,
                gas_limit,
                gas_price,
                data,
                value,
                to_address,
            ) {
                log::error!("Polyjuice send error: {}", err);
                std::process::exit(-1);
            };
        }
        ("polyjuice-call", Some(m)) => {
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();

            let data = m.value_of("data").unwrap();
            let from = m.value_of("from").unwrap();
            let gas_price = m
                .value_of("gas-price")
                .unwrap()
                .parse()
                .expect("gas price format error");
            let gas_limit = m
                .value_of("gas-limit")
                .unwrap()
                .parse()
                .expect("gas limit format error");
            let value = m
                .value_of("value")
                .unwrap()
                .parse()
                .expect("value format error");
            let to_address = m.value_of("to-address").unwrap();

            if let Err(err) = polyjuice::polyjuice_call(
                godwoken_rpc_url,
                gas_limit,
                gas_price,
                data,
                value,
                to_address,
                from,
            ) {
                log::error!("Polyjuice call error: {}", err);
                std::process::exit(-1);
            };
        }
        ("to-short-address", Some(m)) => {
            let deployment_results_path = Path::new(m.value_of("deployment-results-path").unwrap());
            let config_path = Path::new(m.value_of("config-path").unwrap());
            let eth_address = m.value_of("eth-address").unwrap();

            if let Err(err) = address::to_godwoken_short_address(
                eth_address,
                config_path,
                deployment_results_path,
            ) {
                log::error!("To short address error: {}", err);
                std::process::exit(-1);
            };
        }
        ("to-eth-address", Some(m)) => {
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();
            let short_address = m.value_of("short-address").unwrap();

            if let Err(err) = address::to_eth_eoa_address(godwoken_rpc_url, short_address) {
                log::error!("To eth address error: {}", err);
                std::process::exit(-1);
            };
        }
        ("dump-tx", Some(m)) => {
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();
            let block = ChallengeBlock::from_str(m.value_of("block").unwrap()).unwrap();
            let index = u32::from_str(m.value_of("index").unwrap()).unwrap();
            let type_ = match m.value_of("type").unwrap() {
                "tx_execution" => ChallengeTargetType::TxExecution,
                "tx_signature" => ChallengeTargetType::TxSignature,
                "withdrawal" => ChallengeTargetType::Withdrawal,
                _ => panic!("invalid challenge target type"),
            };
            let output = Path::new(m.value_of("output").unwrap());

            if let Err(err) = dump_tx::dump_tx(godwoken_rpc_url, block, index, type_, output) {
                log::error!("Dump offchain cancel challenge tx: {}", err);
                std::process::exit(-1);
            };
        }
        _ => {
            app.print_help().expect("print help");
        }
    }
}
