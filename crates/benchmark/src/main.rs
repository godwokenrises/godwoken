use anyhow::Result;
use clap::{App, Arg, SubCommand};
use gw_benchmark::generate_config_file;
#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    let m = App::new("gw benchmark")
        .subcommand(
            SubCommand::with_name("generate").arg(
                Arg::with_name("path")
                    .takes_value(true)
                    .short("p")
                    .help("The path of generated config file. This is optional."),
            ),
        )
        .subcommand(
            SubCommand::with_name("run").arg(
                Arg::with_name("config-path")
                    .takes_value(true)
                    .short("p")
                    .help("The path of generated config file."),
            ),
        )
        .get_matches();

    if let Some(generate) = m.subcommand_matches("generate") {
        let path = generate.value_of("path");
        if let Err(err) = generate_config_file(path) {
            log::error!("generate config file failed: {}", err);
        }
    }

    if let Some(run) = m.subcommand_matches("run") {
        let path = run.value_of("config");
        if let Err(err) = gw_benchmark::run(path).await {
            log::error!("Benchmark error: {:?}", err);
        }
    }

    Ok(())
}
