mod deploy_genesis;
mod deploy_scripts;
mod generate_config;

use clap::{App, Arg, SubCommand};
use std::path::Path;

fn main() {
    env_logger::init();

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
                    Arg::with_name("output-path")
                        .short("o")
                        .takes_value(true)
                        .required(true)
                        .help("The output json file path"),
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

            if let Err(err) = generate_config::generate_config(
                genesis_path,
                scripts_path,
                polyjuice_binaries_dir,
                ckb_url,
                indexer_url,
                output_path,
            ) {
                log::error!("Deploy genesis error: {}", err);
                std::process::exit(-1);
            };
        }
        _ => {
            app.print_help().expect("print help");
        }
    }
}
