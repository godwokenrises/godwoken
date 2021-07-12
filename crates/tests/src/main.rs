use clap::{App, Arg, SubCommand};
use gw_tests::system_tests::challenge;

fn main() -> Result<(), String> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    let mut app = App::new("godwoken tests")
        .about("Godwoken tests")
        .subcommand(
            SubCommand::with_name("challenge")
                .about("Challenge flow tests")
                .arg(
                    Arg::with_name("test-block")
                        .long("test-block")
                        .short("t")
                        .help("Issue test block"),
                )
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
        ("challenge", Some(m)) => {
            if m.is_present("test-block") {
                challenge::issue_test_blocks(5)?;
            }
            if m.is_present("bad-block-and-revert") {
                challenge::issue_bad_block()?;
            }
            if m.is_present("bad-challenge-and-cancel") {
                challenge::issue_bad_challenge()?;
            }
            if m.is_present("check-balance-when-revert") {
                challenge::check_balance_when_revert()?;
            }
            if m.is_present("multi-bad-blocks-and-revert") {
                challenge::issue_multi_bad_blocks()?;
            }
        }
        _ => {
            app.print_help().expect("print help");
        }
    }
    Ok(())
}
