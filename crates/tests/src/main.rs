use clap::{App, Arg, SubCommand};
use gw_tests::system_tests::{
    test_mode_control::{TestModeConfig, TestModeControl},
    utils::{self, TestModeControlType},
};
use std::{fs, path::Path};

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
            SubCommand::with_name("normal-block")
                .about("Issue normal block containing a transfer tx")
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
        );
    let matches = app.clone().get_matches();
    match matches.subcommand() {
        ("test-mode-control", Some(m)) => {
            let config_path = Path::new(m.value_of("config-file-path").unwrap());
            let content = fs::read(&config_path).map_err(|op| op.to_string())?;
            let config: TestModeConfig = toml::from_slice(&content).map_err(|op| op.to_string())?;
            let guard = sentry::init((
                config.sentry_dsn.clone(),
                sentry::ClientOptions {
                    release: sentry::release_name!(),
                    ..Default::default()
                },
            ));
            log::info!("Sentry guard enabled: {}", guard.is_enabled());
            sentry::capture_message("Test mode control program boot", sentry::Level::Info);
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
        ("normal-block", Some(m)) => {
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
                log::error!("Issue normal block error: {}", err);
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
        _ => {
            app.print_help().expect("print help");
        }
    }
    Ok(())
}
