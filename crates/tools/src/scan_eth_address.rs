//! Scan eth address by iterate all account ids with following RPC:
//! - gw_get_script_hash
//! - gw_get_script_hash_by_registry_address

use std::convert::TryInto;

use anyhow::Result;
use clap::{App, Arg, ArgMatches, Command};
use gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
use gw_rpc_client::gw_client::GWClient;
use std::io::Write;
use tokio::task::JoinSet;

pub(crate) const COMMAND: &str = "scan-eth-address";

pub(crate) fn command() -> App<'static> {
    Command::new(COMMAND)
        .about("Scan ethereum address through node RPC")
        .arg(
            Arg::with_name("url")
                .long("url")
                .takes_value(true)
                .required(true)
                .help("The URL of web3 or godwoken RPC"),
        )
        .arg(
            Arg::with_name("csv-path")
                .long("csv-path")
                .takes_value(true)
                .required(true)
                .help("The path to export CSV file"),
        )
}

enum GetAddrReturn {
    Addr([u8; 20]),
    NoAccount,
    NoEthAddr(u32),
}

async fn scan_account(
    client: &GWClient,
    from_id: u32,
    count: usize,
) -> Result<(Vec<[u8; 20]>, bool)> {
    // spwan ids to channel
    let mut futs: JoinSet<Result<GetAddrReturn>> = JoinSet::new();
    // fetch channel call gw_get_script_hash
    // fetch channel call gw_get_script_hash_by_registry_address
    for id in from_id..from_id + count as u32 {
        let client = client.clone();
        futs.spawn(async move {
            let script_hash = client.gw_get_script_hash(id.into()).await?;
            if script_hash == Default::default() {
                return Ok(GetAddrReturn::NoAccount);
            }
            match client
                .gw_get_registry_address_by_script_hash(script_hash, ETH_REGISTRY_ACCOUNT_ID.into())
                .await?
            {
                Some(addr) => {
                    let addr: [u8; 20] = addr.address.as_bytes().try_into()?;
                    Ok(GetAddrReturn::Addr(addr))
                }
                None => Ok(GetAddrReturn::NoEthAddr(id)),
            }
        });
    }
    let mut addrs = Vec::with_capacity(count);
    let mut stop_scan = false;
    while let Some(addr_res) = futs.join_next().await.transpose()?.transpose()? {
        match addr_res {
            GetAddrReturn::Addr(addr) => addrs.push(addr),
            GetAddrReturn::NoEthAddr(id) => {
                log::warn!("No eth address for account id: {}", id)
            }
            GetAddrReturn::NoAccount => stop_scan = true, // continue cause futures returns in misorder
        }
    }
    Ok((addrs, stop_scan))
}

pub(crate) async fn run(m: &ArgMatches) -> Result<()> {
    let url: String = m.value_of("url").unwrap().into();
    let export_path: String = m.value_of("csv-path").unwrap().into();

    let client = GWClient::with_url(&url)?;

    let mut from_id = 0u32;
    let count = 20;
    let mut total_found = 0;
    loop {
        log::info!("Scan account from_id: {} count: {}", from_id, count);
        let (addrs, stop_scan) = scan_account(&client, from_id, count).await?;
        from_id += count as u32;
        total_found += addrs.len();
        log::info!(
            "Returns {} addresses, total found {}",
            addrs.len(),
            total_found
        );

        // write addrs to export_path
        let mut f = std::fs::File::options()
            .append(true)
            .create(true)
            .open(&export_path)?;
        for addr in addrs {
            f.write_fmt(format_args!("0x{}\n", hex::encode(addr)))?;
        }
        f.flush()?;

        // stop scan if no more address
        if stop_scan {
            log::info!("Found non-existed id, stop scan");
            break;
        }
    }

    Ok(())
}
