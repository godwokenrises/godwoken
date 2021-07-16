use clap::{App, Arg, SubCommand};
use gw_tests::system_tests::{challenge_tests, test_mode_control};
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
                    Arg::with_name("test-blocks")
                        .long("test-blocks")
                        .short("t")
                        .takes_value(true)
                        .help("Issue test blocks"),
                )
                .arg(
                    Arg::with_name("challenge")
                        .long("challenge")
                        .short("c")
                        .takes_value(true)
                        .help("Issue challenge of specific block number"),
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
            SubCommand::with_name("challenge-tests")
                .about("Challenge flow tests")
                .arg(
                    Arg::with_name("bad-block-and-revert")
                        .long("bad-block-and-revert")
                        .short("b")
                        .help("Issue bad block and assert revert"),
                )
                .arg(
                    Arg::with_name("bad-challenge-and-cancel")
                        .long("bad-challenge-and-cancel")
                        .short("c")
                        .help("Issue bad challenge and assert cancel"),
                )
                .arg(
                    Arg::with_name("balance-check-when-revert")
                        .long("balance-check-when-revert")
                        .short("r")
                        .help("Check deposit and withdrawal status when revert"),
                )
                .arg(
                    Arg::with_name("multi-bad-blocks-and-revert")
                        .long("multi-bad-blocks-and-revert")
                        .short("m")
                        .help("Issue multiple bad blocks and assert revert"),
                ),
        );
    let matches = app.clone().get_matches();
    match matches.subcommand() {
        ("test-mode", Some(m)) => {
            if m.is_present("global-state") {
                let state = test_mode_control::get_global_state()?;
                println!("global state is: {:?}", state);
            } else if m.is_present("test-blocks") {
                let count = m
                    .value_of("test-blocks")
                    .map(|c| c.parse().expect("count of blocks"))
                    .unwrap();
                test_mode_control::issue_test_blocks(count)?;
            } else if m.is_present("challenge") {
                let block_number = m
                    .value_of("challenge")
                    .map(|c| c.parse().expect("block number"))
                    .unwrap();
                test_mode_control::issue_challenge(block_number)?;
            } else {
                app.print_help().expect("print help");
            }
        }
        ("bad-block", Some(m)) => {
            let deployment_path = Path::new(m.value_of("scripts-deploy-result-path").unwrap());
            let from_privkey_path = Path::new(m.value_of("from-privkey-path").unwrap());
            let to_privkey_path = Path::new(m.value_of("to-privkey-path").unwrap());
            let config_path = Path::new(m.value_of("config-file-path").unwrap());

            if let Err(err) = test_mode_control::issue_bad_block(
                from_privkey_path,
                to_privkey_path,
                config_path,
                deployment_path,
            ) {
                log::error!("Issue bad block error: {}", err);
                std::process::exit(-1);
            }
        }
        ("challenge-tests", Some(m)) => {
            if m.is_present("bad-block-and-revert") {
                challenge_tests::issue_bad_block_and_revert()?;
            } else if m.is_present("bad-challenge-and-cancel") {
                challenge_tests::issue_bad_challenge_and_cancel()?;
            } else if m.is_present("check-balance-when-revert") {
                challenge_tests::check_balance_when_revert()?;
            } else if m.is_present("multi-bad-blocks-and-revert") {
                challenge_tests::issue_multi_bad_blocks_and_revert()?;
            } else {
                app.print_help().expect("print help");
            }
        }
        _ => {
            app.print_help().expect("print help");
        }
    }
    Ok(())
}
