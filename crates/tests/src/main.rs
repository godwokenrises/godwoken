use clap::{App, Arg, SubCommand};
use gw_tests::system_tests::{bad_block, bad_challenge, utils};
use std::path::Path;

fn main() -> Result<(), String> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    let mut app = App::new("godwoken tests")
        .about("Godwoken tests")
        .subcommand(
            SubCommand::with_name("test-mode")
                .about("Test mode control")
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
                ),
        )
        .subcommand(
            SubCommand::with_name("package-tx")
                .about("Issue a test block containing tx")
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
                ),
        );
    let matches = app.clone().get_matches();
    match matches.subcommand() {
        ("test-mode", Some(m)) => {
            if m.is_present("global-state") {
                let state = utils::get_global_state()?;
                println!("global state is: {:?}", state);
            } else if m.is_present("issue-blocks") {
                let count = m
                    .value_of("issue-blocks")
                    .map(|c| c.parse().expect("count of blocks"))
                    .unwrap();
                utils::issue_blocks(count)?;
            } else {
                app.print_help().expect("print help");
            }
        }
        ("bad-block", Some(m)) => {
            let deployment_path = Path::new(m.value_of("scripts-deploy-result-path").unwrap());
            let from_privkey_path = Path::new(m.value_of("from-privkey-path").unwrap());
            let to_privkey_path = Path::new(m.value_of("to-privkey-path").unwrap());
            let config_path = Path::new(m.value_of("config-file-path").unwrap());

            if let Err(err) = bad_block::issue_bad_block(
                from_privkey_path,
                to_privkey_path,
                config_path,
                deployment_path,
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

            if let Err(err) = utils::package_a_transaction(
                from_privkey_path,
                to_privkey_path,
                config_path,
                deployment_path,
            ) {
                log::error!("Package a transaction error: {}", err);
                std::process::exit(-1);
            }
        }
        ("bad-challenge", Some(m)) => {
            let block_number = m
                .value_of("block-number")
                .map(|c| c.parse().expect("block number"))
                .unwrap();
            if let Err(err) = bad_challenge::issue_bad_challenge(block_number) {
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
