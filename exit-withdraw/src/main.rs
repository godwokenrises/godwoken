use anyhow::{ensure, Context, Result};
use clap::{Args, Parser, Subcommand};
use godwoken_withdraw::{asset_query, model::*, network, report, settlement::adapter::Protocol};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Parser)]
#[command(
    name = "gw-withdraw",
    version,
    about = "Godwoken V0/V1 refunds: independent asset queries, approved settlement and ETH wallet claims"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Args)]
struct Network {
    #[arg(long, default_value = "https://mainnet.ckb.dev/rpc")]
    ckb_rpc: String,
    #[arg(long)]
    indexer_rpc: Option<String>,
    #[arg(long, help = "RollupConfig dependency, tx_hash:0xindex")]
    config_outpoint: Option<String>,
}
#[derive(Subcommand)]
enum Command {
    /// Read native CKB/sUDT supplies and registered balances from L2 DB only; no network or L1 snapshot.
    L2Db {
        #[arg(long, value_enum)]
        protocol: Protocol,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        eth_address: Option<String>,
        /// Also extract embedded protocol storage layouts from this DB (not refund allocations).
        #[arg(long)]
        protocol_state: bool,
        #[arg(long)]
        output: PathBuf,
    },
    /// Query L1 custodian assets only, independently of the L2 DB.
    L1Custodian {
        #[arg(long, value_enum)]
        protocol: Protocol,
        #[arg(long,help="Read a previous raw capture or l1-custodian report instead of making RPC calls",conflicts_with_all=["ckb_rpc","indexer_rpc","config_outpoint"])]
        snapshot: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        #[command(flatten)]
        network: Network,
    },
    /// Compare independently generated L2 DB and L1 custodian reports offline.
    CheckBacking {
        #[arg(long)]
        l2_report: PathBuf,
        #[arg(long)]
        l1_report: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Reconstruct refund tables from the original DB and embedded policy only.
    Refund {
        #[command(subcommand)]
        command: RefundCommand,
    },
    /// Build and verify a TYPE_ID-preserving multisig validator upgrade.
    Upgrade {
        #[command(subcommand)]
        command: UpgradeCommand,
    },
    /// Serve explicitly selected V1/V0 refunds and the ETH-signed L1 claim API.
    ClaimServe(godwoken_withdraw::claim::server::Options),
    /// Approved V3 refunds: commit one state root, pay and verify.
    Settlement {
        #[arg(long, value_enum, default_value = "v1", global = true)]
        protocol: Protocol,
        #[command(subcommand)]
        command: godwoken_withdraw::settlement::cli::Command,
    },
}
#[derive(Subcommand)]
enum UpgradeCommand {
    Build {
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        deployment: PathBuf,
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, default_value = "https://mainnet.ckb.dev/rpc")]
        ckb_rpc: String,
    },
    AssembleSignatures {
        #[arg(long)]
        skeleton: PathBuf,
        #[arg(long, required=true, num_args=1..)]
        signatures: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    VerifySigned {
        #[arg(long)]
        skeleton: PathBuf,
        #[arg(long)]
        deployment: PathBuf,
        #[arg(long)]
        transaction: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "https://mainnet.ckb.dev/rpc")]
        ckb_rpc: String,
    },
}
#[derive(Subcommand)]
enum RefundCommand {
    /// Inspect every asset in a generated report; optionally select an account or ETH owner.
    Inspect {
        #[arg(long, value_enum)]
        protocol: Protocol,
        #[arg(long)]
        report: PathBuf,
        #[arg(long)]
        account_id: Option<u32>,
        #[arg(long)]
        eth_address: Option<String>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Commit a complete generated report to a settlement plan, checking source balances in DB.
    ExportPlan {
        #[arg(long, value_enum)]
        protocol: Protocol,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        report: PathBuf,
        #[arg(long)]
        deployment_id: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Offline V0/V1 reconstruction from the original DB and embedded approved policy.
    Generate {
        #[arg(long, value_enum)]
        protocol: Protocol,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
    },
}
fn resolve_future(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    if absolute.exists() {
        return Ok(absolute.canonicalize()?);
    }
    let name = absolute.file_name().context("Output needs a filename")?;
    Ok(resolve_future(absolute.parent().context("Output parent")?)?.join(name))
}
fn new_output(output: &Path, db: Option<&Path>) -> Result<PathBuf> {
    let output = resolve_future(output)?;
    ensure!(!output.exists(), "Output already exists");
    if let Some(db) = db {
        let db = db.canonicalize()?;
        ensure!(
            !output.starts_with(&db) && !db.starts_with(&output),
            "Output must be separate from DB"
        );
    }
    Ok(output)
}
fn save(path: &Path, value: &Value) -> Result<()> {
    fs::create_dir_all(path.parent().context("Output parent")?)?;
    report::write_new(path, &serde_json::to_vec_pretty(value)?)
}
fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn execute(cli: Cli) -> Result<u8> {
    match cli.command {
        Command::Upgrade { command } => {
            use godwoken_withdraw::upgrade;
            match command {
                UpgradeCommand::Build {
                    request,
                    deployment,
                    binary,
                    output_dir,
                    ckb_rpc,
                } => {
                    let out = new_output(&output_dir, None)?;
                    let sk = upgrade::build(
                        &serde_json::from_value(read(&request)?)?,
                        &serde_json::from_value(read(&deployment)?)?,
                        &fs::read(binary)?,
                        &network::Rpc::new(&ckb_rpc)?,
                    )?;
                    save(&out.join("skeleton.json"), &serde_json::to_value(&sk)?)?;
                    save(
                        &out.join("transaction.json"),
                        &serde_json::to_value(&sk.transaction)?,
                    )?;
                    save(
                        &out.join("signing-request.json"),
                        &serde_json::to_value(upgrade::signing_request(&sk)?)?,
                    )?;
                }
                UpgradeCommand::AssembleSignatures {
                    skeleton,
                    signatures,
                    output,
                } => {
                    let out = new_output(&output, None)?;
                    let responses = signatures
                        .iter()
                        .map(|p| Ok(serde_json::from_value(read(p)?)?))
                        .collect::<Result<Vec<_>>>()?;
                    save(
                        &out,
                        &serde_json::to_value(upgrade::assemble(
                            &serde_json::from_value(read(&skeleton)?)?,
                            &responses,
                        )?)?,
                    )?;
                }
                UpgradeCommand::VerifySigned {
                    skeleton,
                    deployment,
                    transaction,
                    output,
                    ckb_rpc,
                } => {
                    let out = new_output(&output, None)?;
                    let rpc = network::Rpc::new(&ckb_rpc)?;
                    ensure!(
                        rpc.call("get_block_hash", serde_json::json!(["0x0"]))?
                            == godwoken_withdraw::claim::GENESIS,
                        "Not CKB mainnet"
                    );
                    save(
                        &out,
                        &upgrade::verify(
                            &serde_json::from_value(read(&skeleton)?)?,
                            &serde_json::from_value(read(&deployment)?)?,
                            &serde_json::from_value(read(&transaction)?)?,
                            &rpc,
                        )?,
                    )?;
                }
            }
            Ok(0)
        }
        Command::Refund {
            command:
                RefundCommand::Inspect {
                    protocol,
                    report,
                    account_id,
                    eth_address,
                    output,
                },
        } => {
            let out = new_output(&output, None)?;
            save(
                &out,
                &godwoken_withdraw::settlement::table::inspect(
                    &read(&report)?,
                    protocol,
                    account_id,
                    eth_address.as_deref(),
                )?,
            )?;
            Ok(0)
        }
        Command::Refund {
            command:
                RefundCommand::ExportPlan {
                    protocol,
                    db,
                    report,
                    deployment_id,
                    output,
                },
        } => {
            let out = new_output(&output, Some(&db))?;
            let db = godwoken_withdraw::settlement::adapter::open(protocol, &db)?;
            let raw = fs::read(report)?;
            let plan = godwoken_withdraw::settlement::table::export(
                db.as_ref(),
                &serde_json::from_slice(&raw)?,
                &raw,
                hash32(&deployment_id)?,
            )?;
            save(&out, &serde_json::to_value(plan)?)?;
            Ok(0)
        }
        Command::Refund {
            command:
                RefundCommand::Generate {
                    protocol,
                    db,
                    output_dir,
                },
        } => {
            let out = new_output(&output_dir, Some(&db))?;
            let tables = match protocol {
                Protocol::V0 => {
                    let candidate =
                        godwoken_withdraw::refund::reconstruct_candidate(protocol, &db)?;
                    godwoken_withdraw::refund::reports::v0(&candidate)?
                }
                Protocol::V1 => godwoken_withdraw::refund::v1::reconstruct(&db)?,
            };
            fs::create_dir_all(out.parent().context("Report parent")?)?;
            godwoken_withdraw::refund::reports::write_tables(&out, &tables)?;
            println!(
                "{} offline refund calculation: {} (no L1 coverage or execution approval)",
                protocol.name(),
                out.display()
            );
            Ok(0)
        }
        Command::ClaimServe(o) => {
            godwoken_withdraw::claim::server::run(o)?;
            Ok(0)
        }
        Command::Settlement { protocol, command } => {
            godwoken_withdraw::settlement::cli::run_for(command, protocol)?;
            Ok(0)
        }
        Command::L2Db {
            protocol,
            db,
            eth_address: eth,
            protocol_state,
            output,
        } => {
            let out = new_output(&output, Some(&db))?;
            let eth = eth.as_deref().map(eth_address).transpose()?;
            let mut value = asset_query::l2_db(protocol, &db, eth)?;
            if protocol_state {
                value["protocol_state"] = godwoken_withdraw::refund::state::extract(protocol, &db)?;
            }
            save(&out, &value)?;
            println!("{} L2 assets: {}", protocol.name(), out.display());
            Ok(0)
        }
        Command::L1Custodian {
            protocol,
            snapshot,
            output,
            network: n,
        } => {
            let out = new_output(&output, None)?;
            let observation = if let Some(p) = &snapshot {
                let v = read(p)?;
                v.get("observation").unwrap_or(&v).clone()
            } else {
                let indexer = n.indexer_rpc.as_deref().unwrap_or(&n.ckb_rpc);
                match protocol {
                    Protocol::V1 => serde_json::to_value(network::capture(
                        &n.ckb_rpc,
                        indexer,
                        network::config_outpoint(n.config_outpoint.as_deref())?,
                    )?)?,
                    Protocol::V0 => {
                        let cp = if let Some(cp) = n.config_outpoint.as_deref() {
                            network::config_outpoint(Some(cp))?
                        } else {
                            OutPoint{tx_hash:"0xc8502347882e91a849146f20dcff59d413056be9093c83ea17ed88339596f142".into(),index:"0x0".into()}
                        };
                        godwoken_withdraw::v0::capture_at(&n.ckb_rpc, indexer, cp)?
                    }
                }
            };
            let mut value = asset_query::l1_custodian(protocol, observation)?;
            value["observation_source"] = serde_json::json!(if snapshot.is_some() {
                "saved_not_current_liveness"
            } else {
                "live_rpc"
            });
            save(&out, &value)?;
            println!("{} L1 custodian: {}", protocol.name(), out.display());
            Ok(0)
        }
        Command::CheckBacking {
            l2_report,
            l1_report,
            output,
        } => {
            let out = new_output(&output, None)?;
            let r = asset_query::check_backing(&read(&l2_report)?, &read(&l1_report)?)?;
            save(&out, &r)?;
            let mismatch = r["assets"].as_array().context("assets")?.iter().any(|a| {
                a["covers_selected_basis"] != true || a["registered_sum_matches_supply"] == false
            });
            Ok(if mismatch { 1 } else { 2 })
        }
    }
}
fn main() {
    let code = match execute(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("ERROR: {e:#}");
            3
        }
    };
    std::process::exit(code.into());
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_commands_removed() {
        for c in [
            "audit",
            "capture",
            "contract-inventory",
            "state-report",
            "build-skeleton",
            "verify-signed",
            "verify-evidence",
        ] {
            assert!(Cli::try_parse_from(["gw-withdraw", c]).is_err());
        }
    }
    #[test]
    fn protocol_required_and_db_has_no_network() {
        assert!(
            Cli::try_parse_from(["gw-withdraw", "l2-db", "--db", "db", "--output", "out"]).is_err()
        );
        assert!(Cli::try_parse_from([
            "gw-withdraw",
            "l2-db",
            "--protocol",
            "v0",
            "--db",
            "db",
            "--output",
            "out",
            "--ckb-rpc",
            "x"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "gw-withdraw",
            "l1-custodian",
            "--protocol",
            "v1",
            "--snapshot",
            "file",
            "--output",
            "out"
        ])
        .is_ok());
    }
}
