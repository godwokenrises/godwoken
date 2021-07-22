use clap::{App, Arg, SubCommand};
use gw_tests::system_tests::{
    test_mode_control::{TestModeConfig, TestModeControl},
    utils::{self, TestModeControlType},
};
use std::path::Path;

fn main() -> Result<(), String> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    let mut app = App::new("godwoken tests")
        .about("Godwoken tests")
        .subcommand(
            SubCommand::with_name("test-mode-control")
                .about("Test mode control")
                .arg(
                    Arg::with_name("config-file-path")
                        .short("c")
                        .takes_value(true)
                        .required(true)
                        .help("Test mode config file path"),
                ),
        )
        .subcommand(
            SubCommand::with_name("utils")
                .about("Test mode utils")
                .arg(
                    Arg::with_name("global-state")
                        .long("global-state")
                        .short("g")
                        .help("Get global state"),
                )
                .arg(
                    Arg::with_name("issue-blocks")
                        .long("issule-blocks")
                        .short("i")
                        .takes_value(true)
                        .help("Issue empty blocks"),
                )
                .arg(
                    Arg::with_name("godwoken-rpc-url")
                        .short("r")
                        .takes_value(true)
                        .required(true)
                        .default_value("http://127.0.0.1:8119")
                        .help("godwoken rpc url"),
                ),
        )
        .subcommand(
            SubCommand::with_name("bad-block")
                .about("Issue bad block")
                .arg(
                    Arg::with_name("from-privkey-path")
                        .short("f")
                        .takes_value(true)
                        .required(true)
                        .help("from user privkey path"),
                )
                .arg(
                    Arg::with_name("to-privkey-path")
                        .short("t")
                        .takes_value(true)
                        .required(true)
                        .help("To user privkey path"),
                )
                .arg(
                    Arg::with_name("scripts-deploy-result-path")
                        .short("d")
                        .takes_value(true)
                        .required(true)
                        .help("Scripts deploy result file path"),
                )
                .arg(
                    Arg::with_name("config-file-path")
                        .short("c")
                        .takes_value(true)
                        .required(true)
                        .help("godwoken node config file path"),
                )
                .arg(
                    Arg::with_name("godwoken-rpc-url")
                        .short("r")
                        .takes_value(true)
                        .required(true)
                        .default_value("http://127.0.0.1:8119")
                        .help("godwoken rpc url"),
                ),
        )
        .subcommand(
            SubCommand::with_name("package-tx")
                .about("Issue a test block containing a tx")
                .arg(
                    Arg::with_name("from-privkey-path")
                        .short("f")
                        .takes_value(true)
                        .required(true)
                        .help("from user privkey path"),
                )
                .arg(
                    Arg::with_name("to-privkey-path")
                        .short("t")
                        .takes_value(true)
                        .required(true)
                        .help("To user privkey path"),
                )
                .arg(
                    Arg::with_name("scripts-deploy-result-path")
                        .short("d")
                        .takes_value(true)
                        .required(true)
                        .help("Scripts deploy result file path"),
                )
                .arg(
                    Arg::with_name("config-file-path")
                        .short("c")
                        .takes_value(true)
                        .required(true)
                        .help("godwoken node config file path"),
                )
                .arg(
                    Arg::with_name("godwoken-rpc-url")
                        .short("r")
                        .takes_value(true)
                        .required(true)
                        .default_value("http://127.0.0.1:8119")
                        .help("godwoken rpc url"),
                ),
        )
        .subcommand(
            SubCommand::with_name("bad-challenge")
                .about("Issue bad challenge")
                .arg(
                    Arg::with_name("block-number")
                        .short("b")
                        .takes_value(true)
                        .help("block number"),
                )
                .arg(
                    Arg::with_name("godwoken-rpc-url")
                        .short("r")
                        .takes_value(true)
                        .required(true)
                        .default_value("http://127.0.0.1:8119")
                        .help("godwoken rpc url"),
                ),
        )
        .subcommand(
            SubCommand::with_name("deposit")
                .about("Deposit ckb multiple times")
                .arg(
                    Arg::with_name("privkey-path")
                        .short("p")
                        .takes_value(true)
                        .required(true)
                        .help("Privkey path"),
                )
                .arg(
                    Arg::with_name("scripts-deploy-result-path")
                        .short("d")
                        .takes_value(true)
                        .required(true)
                        .help("Scripts deploy result file path"),
                )
                .arg(
                    Arg::with_name("config-file-path")
                        .short("c")
                        .takes_value(true)
                        .required(true)
                        .help("godwoken node config file path"),
                )
                .arg(
                    Arg::with_name("ckb-rpc-url")
                        .short("r")
                        .takes_value(true)
                        .required(true)
                        .default_value("http://127.0.0.1:8114")
                        .help("Ckb rpc url"),
                )
                .arg(
                    Arg::with_name("times")
                        .short("t")
                        .takes_value(true)
                        .required(true)
                        .default_value("1")
                        .help("deposit call times"),
                ),
        );
    let matches = app.clone().get_matches();
    match matches.subcommand() {
        ("test-mode-control", Some(m)) => {
            let _config_path = Path::new(m.value_of("config-file-path").unwrap());
            let config = TestModeConfig {
                loop_interval_secs: 2,
                attack_rand_range: 2,
                track_record_interval_min: 2,
                rpc_timeout_secs: 180,
                transfer_from_privkey_path: "deploy/user_1_pk".into(),
                transfer_to_privkey_path: "deploy/user_2_pk".into(),
                godwoken_rpc_url: "http://127.0.0.1:8129".to_owned(),
                ckb_url: "http://127.0.0.1:8114".to_owned(),
                godwoken_config_path: "deploy/node2/config.toml".into(),
                deployment_results_path: "deploy/scripts-deploy-result.json".into(),
            };
            let mut control = TestModeControl::new(config);
            control.run();
        }
        ("utils", Some(m)) => {
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();
            if m.is_present("global-state") {
                let state = utils::get_global_state(godwoken_rpc_url)?;
                println!("global state is: {:?}", state);
            } else if m.is_present("issue-blocks") {
                let count = m
                    .value_of("issue-blocks")
                    .map(|c| c.parse().expect("count of blocks"))
                    .unwrap();
                utils::issue_blocks(godwoken_rpc_url, count)?;
            } else {
                app.print_help().expect("print help");
            }
        }
        ("bad-block", Some(m)) => {
            let deployment_path = Path::new(m.value_of("scripts-deploy-result-path").unwrap());
            let from_privkey_path = Path::new(m.value_of("from-privkey-path").unwrap());
            let to_privkey_path = Path::new(m.value_of("to-privkey-path").unwrap());
            let config_path = Path::new(m.value_of("config-file-path").unwrap());
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();

            if let Err(err) = utils::transfer_and_issue_block(
                utils::TestModeControlType::BadBlock,
                from_privkey_path,
                to_privkey_path,
                config_path,
                deployment_path,
                godwoken_rpc_url,
            ) {
                log::error!("Issue bad block error: {}", err);
                std::process::exit(-1);
            }
        }
        ("package-tx", Some(m)) => {
            let deployment_path = Path::new(m.value_of("scripts-deploy-result-path").unwrap());
            let from_privkey_path = Path::new(m.value_of("from-privkey-path").unwrap());
            let to_privkey_path = Path::new(m.value_of("to-privkey-path").unwrap());
            let config_path = Path::new(m.value_of("config-file-path").unwrap());
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();

            if let Err(err) = utils::transfer_and_issue_block(
                utils::TestModeControlType::NormalBlock,
                from_privkey_path,
                to_privkey_path,
                config_path,
                deployment_path,
                godwoken_rpc_url,
            ) {
                log::error!("Package a transaction error: {}", err);
                std::process::exit(-1);
            }
        }
        ("bad-challenge", Some(m)) => {
            let block_number = m
                .value_of("block-number")
                .map(|c| c.parse().expect("block number"));
            let godwoken_rpc_url = m.value_of("godwoken-rpc-url").unwrap();

            if let Err(err) = utils::issue_control(
                TestModeControlType::Challenge,
                godwoken_rpc_url,
                block_number,
            ) {
                log::error!("Issue bad challenge error: {}", err);
                std::process::exit(-1);
            }
        }
        ("deposit", Some(m)) => {
            let deployment_path = Path::new(m.value_of("scripts-deploy-result-path").unwrap());
            let privkey_path = Path::new(m.value_of("privkey-path").unwrap());
            let config_path = Path::new(m.value_of("config-file-path").unwrap());
            let ckb_rpc_url = m.value_of("ckb-rpc-url").unwrap();
            let times = m
                .value_of("times")
                .map(|c| c.parse().expect("deposit call times"))
                .unwrap();
            if let Err(err) = utils::deposit(
                privkey_path,
                deployment_path,
                config_path,
                ckb_rpc_url,
                times,
            ) {
                log::error!("Deposit error: {}", err);
                std::process::exit(-1);
            }
        }
        _ => {
            app.print_help().expect("print help");
        }
    }
    Ok(())
}
