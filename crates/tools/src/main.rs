mod deploy_genesis;
mod deploy_scripts;
mod deposit_ckb;
mod generate_config;
pub mod godwoken_rpc;
mod prepare_scripts;
mod setup_nodes;
mod utils;
mod withdraw;

use clap::{value_t, App, Arg, SubCommand};
use std::path::Path;

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
                .arg(
                    Arg::with_name("polyjuice-binaries-dir-path")
                        .short("p")
                        .takes_value(true)
                        .required(true)
                        .help("Polyjuice binaries directory path"),
                )
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
                        .default_value("tmp/scripts-build-dir/")
                        .required(true)
                        .help("The repos dir path"),
                )
                .arg(
                    Arg::with_name("scripts-dir-path")
                        .short("s")
                        .takes_value(true)
                        .default_value("scripts/")
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
                .arg(
                    Arg::with_name("deployment-results-path")
                        .short("d")
                        .long("deployment-results-path")
                        .takes_value(true)
                        .required(true)
                        .help("The deployment results json file path"),
                )
                .arg(
                    Arg::with_name("config-path")
                        .short("o")
                        .long("config-path")
                        .takes_value(true)
                        .required(true)
                        .help("The config.toml file path"),
                )
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
                )
                .arg(
                    Arg::with_name("godwoken-rpc-url")
                        .short("g")
                        .long("godwoken-rpc-url")
                        .takes_value(true)
                        .default_value("http://127.0.0.1:8119")
                        .help("Godwoken jsonrpc rpc sever URL"),
                ),
        )
        .subcommand(
            SubCommand::with_name("withdraw")
                .about("withdraw CKB / sUDT from godwoken")
                .arg(arg_privkey_path.clone())
                .arg(
                    Arg::with_name("deployment-results-path")
                        .short("d")
                        .long("deployment-results-path")
                        .takes_value(true)
                        .required(true)
                        .help("The deployment results json file path"),
                )
                .arg(
                    Arg::with_name("config-path")
                        .short("o")
                        .long("config-path")
                        .takes_value(true)
                        .required(true)
                        .help("The config.toml file path"),
                )
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
                )
                .arg(
                    Arg::with_name("godwoken-rpc-url")
                        .short("g")
                        .long("godwoken-rpc-url")
                        .takes_value(true)
                        .default_value("http://127.0.0.1:8119")
                        .help("Godwoken jsonrpc rpc sever URL"),
                ),
        )
        .subcommand(
            SubCommand::with_name("setup-nodes")
                .about("Generate godwoken nodes private keys, poa and rollup configs")
                .arg(arg_privkey_path.clone())
                .arg(
                    Arg::with_name("capacity")
                        .short("c")
                        .takes_value(true)
                        .default_value("200000")
                        .required(true)
                        .help("Capacity transferred to every node"),
                )
                .arg(
                    Arg::with_name("nodes-count")
                        .short("n")
                        .takes_value(true)
                        .default_value("2")
                        .required(true)
                        .help("The godwoken nodes count"),
                )
                .arg(
                    Arg::with_name("output-dir-path")
                        .short("o")
                        .takes_value(true)
                        .default_value("deploy/")
                        .required(true)
                        .help("The godwoken nodes private keys output dir path"),
                )
                .arg(
                    Arg::with_name("poa-config-path")
                        .short("p")
                        .takes_value(true)
                        .required(true)
                        .help("Output poa config file path"),
                )
                .arg(
                    Arg::with_name("rollup-config-path")
                        .short("r")
                        .takes_value(true)
                        .required(true)
                        .help("Output rollup config file path"),
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
            ) {
                log::error!("Deploy genesis error: {}", err);
                std::process::exit(-1);
            };
        }
        ("generate-config", Some(m)) => {
            let ckb_url = m.value_of("ckb-rpc-url").unwrap().to_string();
            let indexer_url = m.value_of("indexer-rpc-url").unwrap().to_string();
            let scripts_path = Path::new(m.value_of("scripts-deployment-results-path").unwrap());
            let genesis_path = Path::new(m.value_of("genesis-deployment-results-path").unwrap());
            let polyjuice_binaries_dir =
                Path::new(m.value_of("polyjuice-binaries-dir-path").unwrap());
            let output_path = Path::new(m.value_of("output-path").unwrap());
            let database_url = m.value_of("database-url");

            if let Err(err) = generate_config::generate_config(
                genesis_path,
                scripts_path,
                polyjuice_binaries_dir,
                ckb_url,
                indexer_url,
                output_path,
                database_url,
            ) {
                log::error!("Deploy genesis error: {}", err);
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
        ("setup-nodes", Some(m)) => {
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let capacity = m
                .value_of("capacity")
                .map(|c| c.parse().expect("get capacity"))
                .unwrap();
            let nodes_count = m
                .value_of("nodes-count")
                .map(|c| c.parse().expect("nodes count"))
                .unwrap();
            let output_dir = Path::new(m.value_of("output-dir-path").unwrap());
            let poa_config_path = Path::new(m.value_of("poa-config-path").unwrap());
            let rollup_config_path = Path::new(m.value_of("rollup-config-path").unwrap());
            setup_nodes::setup_nodes(
                privkey_path,
                capacity,
                nodes_count,
                output_dir,
                poa_config_path,
                rollup_config_path,
            );
        }
        _ => {
            app.print_help().expect("print help");
        }
    }
}
