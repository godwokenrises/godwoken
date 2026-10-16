use super::*;
use crate::{db::Database, network, report};
use clap::Subcommand;
use serde::de::DeserializeOwned;
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Subcommand)]
pub enum Command {
    /// Offline synthetic L1 history; cold-replay one step from the real read-only DB. Never signs or broadcasts.
    RehearseNext {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long,conflicts_with_all=["deployment","snapshot"])]
        archive: Option<PathBuf>,
        #[arg(long, required_unless_present = "archive")]
        deployment: Option<PathBuf>,
        #[arg(long, required_unless_present = "archive")]
        snapshot: Option<PathBuf>,
        #[arg(long, conflicts_with = "payments")]
        addresses: Option<PathBuf>,
        #[arg(long)]
        payments: Option<PathBuf>,
        #[arg(long)]
        output_dir: PathBuf,
    },
    /// Wrap and authenticate one recoverable ckb-cli util sign-message result.
    ImportSignature {
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        lock_hash: String,
        #[arg(long)]
        ckb_cli_output: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Publish an immutable control proposal with offline Ops approvals for the web portal.
    PublishControlProposal {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        deployment: PathBuf,
        #[arg(long)]
        skeleton: PathBuf,
        #[arg(long, required=true, num_args=1..)]
        signatures: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Render and cross-check payout/capacity/custody CSVs from a completed host rehearsal.
    SummarizeSimulation {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        report: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
    },
    /// Copy only checksummed release tables into a self-contained portal directory.
    ExportPortal {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        tables: PathBuf,
        #[arg(long)]
        unmapped_balances: Option<PathBuf>,
        #[arg(long)]
        empty_successor: Option<PathBuf>,
        #[arg(long)]
        output_dir: PathBuf,
    },
    /// Export full exact source/reserve/claim records from the verified snapshot and published tables.
    ExportPlan {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        tables: PathBuf,
        #[arg(long)]
        l1_snapshot: PathBuf,
        #[arg(long)]
        deployment_id: String,
        #[arg(long)]
        policy_hash: String,
        #[arg(long)]
        unmapped_balances: Option<PathBuf>,
        #[arg(long)]
        empty_successor: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Independently rebuild roots and reconcile all source partitions and claim budgets.
    VerifyPlan {
        #[arg(long)]
        plan: PathBuf,
    },
    /// Bind a verified plan to a fresh bootstrap snapshot and the actual governance multisig.
    CreateProfile {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        l1_snapshot: PathBuf,
        #[arg(long)]
        multisig_config: Option<String>,
        /// v0: standard CKB secp Script JSON.
        #[arg(long)]
        authority_lock: Option<PathBuf>,
        /// v0: the two current PoA helper cells and two producer cells (JSON array).
        #[arg(long)]
        bootstrap_auxiliary: Option<PathBuf>,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, default_value_t = 100_000)]
        fee_shannons: u64,
        #[arg(long, default_value_t = 24)]
        confirmations: u64,
    },
    /// Reconstruct canonical state and build root initialization or an applicant payout.
    BuildNext {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        deployment: PathBuf,
        #[arg(long)]
        addresses: Option<PathBuf>,
        /// Reviewed contract accounts, exact CKB destinations and ETH OmniLock authorization inputs.
        #[arg(long)]
        controls: Option<PathBuf>,
        /// Explicit claim keys and raw L2 amounts for partial payments or large beneficiaries.
        #[arg(long, conflicts_with = "addresses")]
        payments: Option<PathBuf>,
        /// Reviewed governance lock hash and maximum total CKB spend (fee included).
        #[arg(long)]
        capacity_funding: Option<PathBuf>,
        #[arg(long, default_value_t = 4)]
        batch_size: usize,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, default_value = "https://mainnet.ckb.dev/rpc")]
        ckb_rpc: String,
        #[arg(long)]
        indexer_rpc: Option<String>,
        #[arg(long)]
        config_outpoint: Option<String>,
    },
    /// Validate a fully signed transaction against live state and all executed scripts; no broadcast.
    VerifySigned {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        deployment: PathBuf,
        #[arg(long)]
        transaction: PathBuf,
        #[arg(long, default_value_t = 70_000_000)]
        max_cycles: u64,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "https://mainnet.ckb.dev/rpc")]
        ckb_rpc: String,
        #[arg(long)]
        indexer_rpc: Option<String>,
        #[arg(long)]
        config_outpoint: Option<String>,
    },
    /// Regenerate offline signing messages from a reviewed skeleton and pinned deployment.
    SigningRequests {
        #[arg(long)]
        deployment: PathBuf,
        #[arg(long)]
        skeleton: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Offline optional signer. Compare the request with a reviewed skeleton and pinned deployment before reading a key.
    SignMessage {
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        deployment: PathBuf,
        #[arg(long)]
        skeleton: PathBuf,
        /// Independent governance account and maximum total CKB spend for funded payouts.
        #[arg(long)]
        capacity_funding: Option<PathBuf>,
        #[arg(long)]
        lock_hash: String,
        #[arg(long)]
        private_key_file: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Recover and check all external signatures, then insert them without altering the transaction.
    AssembleSignatures {
        #[arg(long)]
        deployment: PathBuf,
        #[arg(long)]
        skeleton: PathBuf,
        #[arg(long, required=true, num_args=1..)]
        signatures: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },

    /// Rehearse all registrations and applicant payouts using host consensus and the read-only DB.
    /// Artificial fee cells, no signature or VM validation, no network or broadcast.
    SimulatePlan {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        deployment: PathBuf,
        #[arg(long)]
        l1_snapshot: PathBuf,
        #[arg(long)]
        addresses: Option<PathBuf>,
        /// Rehearse every beneficiary; capacity-blocked rights remain unpaid.
        #[arg(long, conflicts_with = "addresses")]
        all: bool,
        #[arg(long, default_value_t = 32)]
        batch_size: usize,
        #[arg(long)]
        output_dir: PathBuf,
    },

    /// Decode a binary profile and report the exact CKB-personalized hash used by the validator.
    InspectProfile {
        #[arg(long)]
        profile: PathBuf,
    },

    /// Reconstruct and report root initialization, paid counters, and applicant remainders.
    Status {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        deployment: PathBuf,
        #[arg(long)]
        addresses: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "https://mainnet.ckb.dev/rpc")]
        ckb_rpc: String,
        #[arg(long)]
        indexer_rpc: Option<String>,
        #[arg(long)]
        config_outpoint: Option<String>,
    },
}
fn load<T: DeserializeOwned>(p: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn save<T: serde::Serialize>(p: &Path, v: &T) -> Result<()> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    report::write_new(p, &serde_json::to_vec_pretty(v)?)?;
    Ok(())
}
fn outside_db(path: &Path, db: &Path) -> Result<()> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut parent = absolute.as_path();
    while !parent.exists() {
        parent = parent.parent().context("Invalid output path")?;
    }
    let relative = absolute.strip_prefix(parent)?;
    let resolved = parent.canonicalize()?.join(relative);
    ensure!(
        !resolved.starts_with(db.canonicalize()?)
            && !relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
        "Output cannot be inside the source DB or traverse parents"
    );
    Ok(())
}
pub fn run(cmd: Command) -> Result<()> {
    run_for(cmd, Protocol::V1)
}
pub fn run_for(cmd: Command, protocol: Protocol) -> Result<()> {
    match cmd {
        Command::RehearseNext {
            db,
            plan,
            archive,
            deployment,
            snapshot,
            addresses,
            payments,
            output_dir,
        } => {
            outside_db(&output_dir, &db)?;
            ensure!(!output_dir.exists(), "Output exists");
            let plan: plan::Plan = load(&plan)?;
            ensure!(plan.protocol == protocol, "Wrong protocol");
            let db = adapter::open(protocol, &db)?;
            let mut archive: rehearsal::Archive = if let Some(path) = archive {
                load(&path)?
            } else {
                rehearsal::Archive::initialize(
                    &plan,
                    load(deployment.as_deref().context("Deployment")?)?,
                    adapter::bundle(load(snapshot.as_deref().context("Snapshot")?)?, protocol)?,
                )?
            };
            let addresses: Option<Vec<String>> = addresses.as_deref().map(load).transpose()?;
            let payments: Option<Vec<PaymentSelection>> =
                payments.as_deref().map(load).transpose()?;
            let (sk, summary) = archive.step(
                db.as_ref(),
                &plan,
                addresses.as_deref(),
                payments.as_deref(),
            )?;
            save(&output_dir.join("archive.json"), &archive)?;
            save(&output_dir.join("skeleton.json"), &sk)?;
            save(&output_dir.join("summary.json"), &summary)?;
            println!("{}", summary["status"]);
        }
        Command::ImportSignature {
            request,
            lock_hash,
            ckb_cli_output,
            output,
        } => {
            let request: signing::SigningRequest = load(&request)?;
            let value: serde_json::Value = load(&ckb_cli_output)?;
            ensure!(
                value["recoverable"] == true,
                "ckb-cli requires --recoverable"
            );
            let group = request
                .groups
                .iter()
                .find(|g| g.lock_hash == lock_hash)
                .context("Unknown lock group")?;
            let signature = value["signature"]
                .as_str()
                .context("Missing signature")?
                .to_string();
            signing::recovered(group, &signature)?;
            save(
                &output,
                &signing::Signatures {
                    transaction_hash: request.transaction_hash,
                    signatures: vec![signing::Signature {
                        lock_hash,
                        signature,
                    }],
                },
            )?;
        }
        Command::PublishControlProposal {
            plan,
            deployment,
            skeleton,
            signatures,
            output,
        } => {
            let plan: plan::Plan = load(&plan)?;
            ensure!(plan.protocol == protocol, "Protocol mismatch");
            let d: Deployment = load(&deployment)?;
            let profile = Profile::load(&d)?;
            profile.bind(&plan)?;
            let sk: Skeleton = load(&skeleton)?;
            let action = replay::action_for(&sk.transaction, &profile)?;
            ensure!(
                action.op == s::Operation::Pay
                    && action.epoch == 0
                    && action
                        .native
                        .recipients
                        .iter()
                        .any(|r| c::control::is_descriptor(&r.script)),
                "Not an initial control payout"
            );
            let mut recipients = BTreeSet::new();
            for raw in &action.records {
                let pay: s::records::Payment = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
                ensure!(
                    plan.claims.contains(&hx(encode(&pay.claim)))
                        && plan.recipient_scripts.contains_key(&pay.claim.recipient),
                    "Proposal claim outside published control plan"
                );
                recipients.insert(pay.claim.recipient);
            }
            ensure!(
                recipients.len() == 1,
                "Web proposal must contain exactly one control recipient"
            );
            let req = signing::requests(&d, &sk)?;
            let signatures = signatures
                .iter()
                .map(|p| load::<signing::Signatures>(p))
                .collect::<Result<Vec<_>>>()?;
            signing::verify_ops_signatures(&req, &signatures)?;
            save(
                &output,
                &crate::claim::server::ControlProposal {
                    protocol,
                    plan_manifest: plan.manifest,
                    skeleton: sk,
                    signatures,
                },
            )?;
        }

        Command::SummarizeSimulation {
            plan,
            report,
            output_dir,
        } => {
            let plan: plan::Plan = load(&plan)?;
            ensure!(plan.protocol == protocol, "Plan protocol mismatch");
            println!(
                "{}",
                serde_json::to_string_pretty(&simulation::summarize(&plan, &report, &output_dir)?)?
            );
        }

        Command::ExportPortal {
            plan,
            tables,
            unmapped_balances,
            empty_successor,
            output_dir,
        } => {
            use sha2::{Digest, Sha256};
            let plan: plan::Plan = load(&plan)?;
            plan.verify()?;
            ensure!(
                plan.protocol == protocol && !output_dir.exists(),
                "Protocol mismatch or output exists"
            );
            let mut files = Vec::new();
            for (name, digest) in &plan.input_sha256 {
                let input = match name.as_str() {
                    "report.json"
                    | "assets.json"
                    | "summary.json"
                    | "source-allocations.json"
                    | "source-checks.json"
                    | "claims.csv"
                    | "source-reserves.csv"
                    | "asset-coverage.json" => tables.join(name),
                    "account-assets.csv" => tables
                        .parent()
                        .context("Tables parent")?
                        .join("reconciliation/account-assets.csv"),
                    "unmapped-balances.csv" => unmapped_balances
                        .clone()
                        .context("Supply --unmapped-balances")?,
                    "empty-successor.json" => empty_successor
                        .clone()
                        .context("Supply --empty-successor")?,
                    _ => anyhow::bail!("Unexpected plan input path"),
                };
                let b = fs::read(input)?;
                ensure!(
                    hex::encode(Sha256::digest(&b)) == *digest,
                    "Table checksum mismatch: {name}"
                );
                files.push((name.clone(), b));
            }
            if protocol == Protocol::V1 && !plan.input_sha256.contains_key("report.json") {
                files.push((
                    "accounts.json".into(),
                    fs::read(tables.join("accounts.json"))?,
                ));
            }
            fs::create_dir_all(&output_dir)?;
            for (name, b) in files {
                report::write_new(&output_dir.join(name), &b)?;
            }
            save(&output_dir.join("plan.json"), &plan)?;
            println!(
                "Prepared {} portal tables at {}",
                protocol.name(),
                output_dir.display()
            );
        }

        Command::ExportPlan {
            db,
            tables,
            l1_snapshot,
            deployment_id,
            policy_hash,
            unmapped_balances,
            empty_successor,
            output,
        } => {
            outside_db(&output, &db)?;
            let bundle = adapter::bundle(load(&l1_snapshot)?, protocol)?;
            let p = if protocol == Protocol::V0 {
                let mut db = crate::v0::V0Db::open(&db)?;
                db.rebuild()?;
                super::v0_plan::export(
                    &db,
                    &tables,
                    unmapped_balances
                        .as_deref()
                        .context("v0 export requires --unmapped-balances")?,
                    empty_successor.as_deref(),
                    hash32(&deployment_id)?,
                    hash32(&policy_hash)?,
                    &bundle,
                )?
            } else {
                plan::export(
                    &Database::open(&db)?,
                    &tables,
                    hash32(&deployment_id)?,
                    hash32(&policy_hash)?,
                    &bundle,
                )?
            };
            let report = p.verify()?;
            save(&output, &p)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::VerifyPlan { plan } => {
            let p: plan::Plan = load(&plan)?;
            ensure!(p.protocol == protocol, "Plan protocol mismatch");
            println!("{}", serde_json::to_string_pretty(&p.verify()?)?);
        }
        Command::CreateProfile {
            plan,
            l1_snapshot,
            multisig_config,
            authority_lock,
            bootstrap_auxiliary,
            output_dir,
            fee_shannons,
            confirmations,
        } => {
            ensure!(!output_dir.exists(), "Output directory already exists");
            ensure!(confirmations > 0, "Confirmations must be positive");
            let plan: plan::Plan = load(&plan)?;
            plan.verify()?;
            ensure!(plan.protocol == protocol, "Plan protocol mismatch");
            let b = adapter::bundle(load(&l1_snapshot)?, protocol)?;
            let (_, cfg) = adapter::validate(&b, protocol)?;
            ensure!(
                GlobalState::parse(&bytes(&b.rollup_cell.output_data)?)?.account_root
                    == plan.snapshot_root,
                "Live bootstrap and plan snapshot differ"
            );
            let multisig_config = multisig_config.unwrap_or_else(|| "0x".into());
            let config = bytes(&multisig_config)?;
            let gov = if protocol == Protocol::V1 {
                ensure!(
                    config.len() >= 4
                        && config[0] == 0
                        && config[2] > 0
                        && config[1] <= config[2]
                        && config[2] <= config[3]
                        && config.len() == 4 + 20 * config[3] as usize,
                    "Invalid multisig preimage"
                );
                packed::Script::new_builder()
                    .code_hash(c::MULTISIG.pack())
                    .hash_type(1u8.into())
                    .args(hash(&config)[..20].to_vec().pack())
                    .build()
            } else {
                let lock: Script = load(
                    authority_lock
                        .as_deref()
                        .context("v0 requires --authority-lock")?,
                )?;
                lock.packed()?
            };
            let template = b.cells["custodian"]
                .first()
                .context("No custodian change template")?
                .output
                .lock
                .packed()?;
            let change = if protocol == Protocol::V0 {
                template
            } else {
                let args = template.args().raw_data();
                let old = packed::CustodianLockArgs::from_slice(&args[32..])?;
                let normalized = old
                    .as_builder()
                    .deposit_block_hash([0; 32].pack())
                    .deposit_finalized_timepoint(0u64.pack())
                    .build();
                template
                    .as_builder()
                    .args([args[..32].as_ref(), normalized.as_slice()].concat().pack())
                    .build()
            };
            let limits = plan
                .limits
                .iter()
                .map(|r| plan::parse::<AssetLimit>(r))
                .collect::<Result<Vec<_>>>()?;
            let mut eoa_codes: Vec<c::Hash> = cfg
                .packed
                .allowed_eoa_type_hashes()
                .into_iter()
                .filter(|v| u8::from(v.type_()) == 1)
                .map(|v| v.hash().unpack())
                .collect();
            if protocol == Protocol::V0 {
                eoa_codes = vec![c::v0::ETH];
            }
            eoa_codes.sort();
            eoa_codes.dedup();
            let base = c::Profile {
                rollup: b
                    .rollup_cell
                    .output
                    .type_script
                    .as_ref()
                    .context("Missing rollup type")?
                    .packed()?,
                config_hash: hash(&bytes(&b.config_data)?),
                bootstrap_outpoint: withdraw::outpoint(&b.rollup_cell.out_point)?,
                bootstrap_capacity: quantity(&b.rollup_cell.output.capacity)?,
                bootstrap_lock: b.rollup_cell.output.lock.packed()?,
                bootstrap_state: packed::GlobalState::from_slice(&bytes(
                    &b.rollup_cell.output_data,
                )?)?,
                governance: gov,
                change,
                eoa_codes,
                catalogue: limits.iter().map(|v| (v.asset, v.type_hash)).collect(),
            };
            let p = s::SettlementProfile {
                base,
                deployment: hash32(&plan.deployment)?,
                manifest: hash32(&plan.manifest)?,
                state_root: hash32(&plan.state_root)?,
                source_count: plan.sources.len() as u64,
                reserve_count: plan.reserves.len() as u64,
                initial_epoch: plan.epoch()?,
                limits,
            };
            let data = if protocol == Protocol::V0 {
                let cells: Vec<Cell> = load(
                    bootstrap_auxiliary
                        .as_deref()
                        .context("v0 requires --bootstrap-auxiliary")?,
                )?;
                let auxiliary = cells
                    .iter()
                    .map(|cell| {
                        let before = withdraw::contract_cell(cell)?;
                        let mut after = before.clone();
                        if after.type_.is_some() {
                            after.lock = p.base.governance.clone();
                        }
                        Ok(c::v0::Auxiliary {
                            outpoint: withdraw::outpoint(&cell.out_point)?,
                            before,
                            after,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                c::v0::encode_profile(&c::v0::Profile {
                    settlement: p.clone(),
                    auxiliary,
                })
            } else {
                s::codec::encode_profile(&p)
            };
            let ph = hash(&data);
            let runtime = Profile::decode(&data, ph)?;
            runtime.bind(&plan)?;
            let backing = coverage_version(&p, &b, None, protocol)?;
            let deployment = Deployment {
                profile: hx(&data),
                profile_hash: hx(ph),
                validator_data_hash: hx([0; 32]),
                cell_deps: vec![],
                sponsor_inputs: vec![],
                multisig_config,
                bootstrap_lock_placeholder: {
                    let args = bytes(&b.rollup_cell.output.lock.args)?;
                    if args.len() == 22 && args[0] <= 1 && args[21] == 0 {
                        let mut placeholder = Vec::new();
                        for n in [85u32, 16, 85, 85, 65] {
                            placeholder.extend_from_slice(&n.to_le_bytes());
                        }
                        placeholder.extend_from_slice(&[0; 65]);
                        Some(hx(placeholder))
                    } else {
                        None
                    }
                },
                fee_shannons,
                confirmations,
            };
            if protocol == Protocol::V1 {
                deployment.multisig_placeholder(&p.base)?;
            }
            fs::create_dir_all(&output_dir)?;
            save(&output_dir.join("coverage.json"), &backing)?;
            report::write_new(&output_dir.join("profile.bin"), &data)?;
            report::write_new(&output_dir.join("profile.hash"), hx(ph).as_bytes())?;
            save(&output_dir.join("deployment.template.json"), &deployment)?;
            save(
                &output_dir.join("release-inputs.json"),
                &json!({"status":"profile_prepared_not_deployed","protocol":protocol,"profile_hash":hx(ph),"plan_manifest":plan.manifest,"bootstrap":b.rollup_cell.out_point,"v0_bootstrap_note":if protocol==Protocol::V0 {Some("Two PoA producer signatures plus standard single-signature authority; helper setup must not be a dependency")}else{None},"required_operator_inputs":["Pinned deployed validator binary hash and explicit code dependency outpoints","Published profile cell dependency and config dependency","Bootstrap lock signing placeholder matching its actual lock","Live untyped governance fee sponsor inputs"],"broadcast":false}),
            )?;
            println!("Profile prepared at {}; deployment template must be completed before building transactions",output_dir.display());
        }
        Command::BuildNext {
            db,
            plan,
            deployment,
            addresses,
            controls,
            payments,
            capacity_funding,
            batch_size,
            output_dir,
            ckb_rpc,
            indexer_rpc,
            config_outpoint,
        } => {
            outside_db(&output_dir, &db)?;
            ensure!(!output_dir.exists(), "Output directory already exists");
            let plan: plan::Plan = load(&plan)?;
            let d: Deployment = load(&deployment)?;
            let mut addresses: Option<Vec<String>> = addresses.as_deref().map(load).transpose()?;
            let controls: Vec<build::ControlRequest> = controls
                .as_deref()
                .map(load)
                .transpose()?
                .unwrap_or_default();
            for control in &controls {
                let script = plan
                    .recipient_scripts
                    .get(&control.account_id)
                    .context("Account has no approved control authority")?
                    .packed()?;
                addresses
                    .get_or_insert_with(Vec::new)
                    .push(hx(c::control::identity(&script)));
            }
            let db = adapter::open(protocol, &db)?;
            let rpc = network::Rpc::new(&ckb_rpc)?;
            let bundle = capture(
                protocol,
                &ckb_rpc,
                indexer_rpc.as_deref().unwrap_or(&ckb_rpc),
                config_point(protocol, config_outpoint.as_deref())?,
            )?;
            let payments: Option<Vec<PaymentSelection>> =
                payments.as_deref().map(load).transpose()?;
            let funding: Option<build::CapacityFunding> =
                capacity_funding.as_deref().map(load).transpose()?;
            let out = prepare_selected_funding(
                db.as_ref(),
                &rpc,
                &d,
                &bundle,
                &plan,
                addresses.as_deref(),
                batch_size,
                &controls,
                payments.as_deref(),
                funding.as_ref(),
            )?;
            fs::create_dir_all(&output_dir)?;
            save(&output_dir.join("skeleton.json"), &out)?;
            save(&output_dir.join("transaction.json"), &out.transaction)?;
            save(
                &output_dir.join("signing-request.json"),
                &signing::requests(&d, &out)?,
            )?;
            let mut next = d.clone();
            let gov = Script::from_packed(Profile::load(&d)?.base.governance.clone())?;
            let sponsor_output_indices = if let Some(index) =
                out.allocations["capacity_funding"]["sponsor_change_output_index"].as_u64()
            {
                let index = usize::try_from(index)?;
                let cell = out
                    .transaction
                    .outputs
                    .get(index)
                    .context("Missing recorded sponsor change")?;
                ensure!(
                    cell.lock == gov && cell.type_script.is_none(),
                    "Recorded sponsor change is not a plain governance cell"
                );
                vec![index]
            } else {
                out.transaction
                    .outputs
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.lock == gov && c.type_script.is_none())
                    .map(|(i, _)| i)
                    .collect()
            };
            next.sponsor_inputs = sponsor_output_indices
                .into_iter()
                .map(|i| OutPoint {
                    tx_hash: out.transaction.hash().expect("validated transaction"),
                    index: format!("0x{i:x}"),
                })
                .collect();
            save(
                &output_dir.join("next-deployment-after-confirmation.json"),
                &next,
            )?;
            let a = replay::action_for(&out.transaction, &Profile::load(&d)?)?;
            save(
                &output_dir.join("progress.json"),
                &json!({"operation":format!("{:?}", a.op),"metadata_after":hx(a.after),"capacity_funding":out.allocations.get("capacity_funding"),"use_next_deployment_only_after_confirmation":true}),
            )?;
            println!("{}", serde_json::to_string_pretty(&out.allocations)?);
        }
        Command::VerifySigned {
            db,
            plan,
            deployment,
            transaction,
            max_cycles,
            output,
            ckb_rpc,
            indexer_rpc,
            config_outpoint,
        } => {
            outside_db(&output, &db)?;
            let plan: plan::Plan = load(&plan)?;
            let d: Deployment = load(&deployment)?;
            let tx: Transaction = load(&transaction)?;
            let db = adapter::open(protocol, &db)?;
            let rpc = network::Rpc::new(&ckb_rpc)?;
            let bundle = capture(
                protocol,
                &ckb_rpc,
                indexer_rpc.as_deref().unwrap_or(&ckb_rpc),
                config_point(protocol, config_outpoint.as_deref())?,
            )?;
            let mut checked =
                verify_signed(db.as_ref(), &rpc, &d, &bundle, &plan, &tx, max_cycles)?;
            checked["signed_transaction_data_hash"] = json!(hx(hash(tx.packed()?.as_slice())));
            checked["broadcast_request"] = json!({"jsonrpc":"2.0","id":1,"method":"send_transaction","params":[tx,"passthrough"]});
            save(&output, &checked)?;
            println!("{}", serde_json::to_string_pretty(&checked)?);
        }
        Command::SigningRequests {
            deployment,
            skeleton,
            output,
        } => {
            let d = load(&deployment)?;
            ensure!(
                Profile::load(&d)?.protocol() == protocol,
                "Selected signing protocol differs from deployment"
            );
            let sk = load(&skeleton)?;
            save(&output, &signing::requests(&d, &sk)?)?;
        }
        Command::SignMessage {
            request,
            deployment,
            skeleton,
            capacity_funding,
            lock_hash,
            private_key_file,
            output,
        } => {
            let d: Deployment = load(&deployment)?;
            ensure!(
                Profile::load(&d)?.protocol() == protocol,
                "Selected signing protocol differs from deployment"
            );
            let sk: Skeleton = load(&skeleton)?;
            let supplied: signing::SigningRequest = load(&request)?;
            let request = signing::checked_request(&d, &sk, &supplied)?;
            let funding: Option<build::CapacityFunding> =
                capacity_funding.as_deref().map(load).transpose()?;
            signing::check_capacity_funding(&d, &sk, funding.as_ref())?;
            ensure!(
                request.groups.iter().any(|g| g.lock_hash == lock_hash),
                "Unknown signing group"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                ensure!(
                    fs::metadata(&private_key_file)?.permissions().mode() & 0o077 == 0,
                    "Private key file must not be readable by group/others (chmod 600)"
                );
            }
            let mut secret_text = fs::read(&private_key_file)?;
            let mut secret = bytes(std::str::from_utf8(&secret_text)?.trim())?;
            let response = signing::sign(&request, &lock_hash, &secret);
            secret.fill(0);
            // Best-effort clearing only; for hardware custody use external signatures.
            secret_text.fill(0);
            save(&output, &response?)?;
        }
        Command::AssembleSignatures {
            deployment,
            skeleton,
            signatures,
            output,
        } => {
            let d = load(&deployment)?;
            ensure!(
                Profile::load(&d)?.protocol() == protocol,
                "Selected signing protocol differs from deployment"
            );
            let sk = load(&skeleton)?;
            let responses = signatures
                .iter()
                .map(|p| load(p))
                .collect::<Result<Vec<_>>>()?;
            save(&output, &signing::assemble(&d, &sk, &responses)?)?;
        }

        Command::SimulatePlan {
            db,
            plan,
            deployment,
            l1_snapshot,
            addresses,
            all,
            batch_size,
            output_dir,
        } => {
            outside_db(&output_dir, &db)?;
            let db = adapter::open(protocol, &db)?;
            let plan: plan::Plan = load(&plan)?;
            let selected = if all {
                plan.claims
                    .iter()
                    .map(|v| plan::parse::<Claim>(v).map(|v| hx(v.address)))
                    .collect::<Result<BTreeSet<_>>>()?
                    .into_iter()
                    .collect::<Vec<_>>()
            } else {
                load::<Vec<String>>(
                    addresses
                        .as_deref()
                        .context("Supply --addresses or --all")?,
                )?
            };
            let result = simulation::run(
                db.as_ref(),
                &plan,
                &load(&deployment)?,
                &adapter::bundle(load(&l1_snapshot)?, protocol)?,
                &selected,
                batch_size,
                &output_dir,
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }

        Command::InspectProfile { profile } => {
            let b = fs::read(profile)?;
            let h = hash(&b);
            let p = Profile::decode(&b, h)?;
            ensure!(p.protocol() == protocol, "Profile protocol mismatch");
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"version":s::FORMAT_VERSION,"protocol":p.protocol(),"profile_hash":hx(h),"deployment":hx(p.deployment),"manifest":hx(p.manifest),"metadata_key":hx(metadata_key(p.deployment,c::script_hash(&p.base.rollup))),"state_root":hx(p.state_root),"sources":p.source_count,"reserves":p.reserve_count,"claims":p.initial_epoch.claim_count,"bootstrap_outpoint":withdraw::from_outpoint(&p.base.bootstrap_outpoint),"governance":Script::from_packed(p.base.governance.clone())?})
                )?
            );
        }

        Command::Status {
            db,
            plan,
            deployment,
            addresses,
            output,
            ckb_rpc,
            indexer_rpc,
            config_outpoint,
        } => {
            outside_db(&output, &db)?;
            let db = adapter::open(protocol, &db)?;
            let plan: plan::Plan = load(&plan)?;
            let d: Deployment = load(&deployment)?;
            let p = Profile::load(&d)?;
            ensure!(p.protocol() == protocol, "Deployment protocol mismatch");
            let rpc = network::Rpc::new(&ckb_rpc)?;
            let b = capture(
                protocol,
                &ckb_rpc,
                indexer_rpc.as_deref().unwrap_or(&ckb_rpc),
                config_point(protocol, config_outpoint.as_deref())?,
            )?;
            adapter::validate(&b, protocol)?;
            let (native, ledger, journal) = replay::reconstruct_for(
                db.as_ref(),
                &rpc,
                &p,
                &plan,
                &bytes(&b.config_data)?,
                &b.rollup_cell,
                d.confirmations,
            )?;
            let backing = remaining_coverage(&p, &b, &ledger, &native)?;
            let selected: Option<Vec<String>> = addresses.as_deref().map(load).transpose()?;
            let selected = selected
                .map(|v| {
                    v.iter()
                        .map(|v| eth_address(v))
                        .collect::<Result<BTreeSet<_>>>()
                })
                .transpose()?;
            let mut accounts = BTreeMap::<(String, u32), (U256, U256)>::new();
            for raw in ledger.records[s::CLAIM_PLAN].values() {
                let c: Claim = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
                if selected.as_ref().is_some_and(|v| !v.contains(&c.address)) {
                    continue;
                }
                let src: Source = decode(
                    ledger.records[s::SOURCE_PLAN]
                        .get(&c.source)
                        .context("Missing planned source")?,
                )
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
                let v = accounts.entry((hx(c.address), src.asset)).or_default();
                v.0 = v.0.checked_add(c.cap).context("Overflow")?;
                v.1 =
                    v.1.checked_add(ledger.paid(p.deployment, &c)?)
                        .context("Overflow")?;
            }
            let claim_details=ledger.records[s::CLAIM_PLAN].iter().map(|(key,raw)|{
                let c:Claim=decode(raw).map_err(|e|anyhow::anyhow!("{e:?}"))?;let src:Source=ledger.get(s::SOURCE_PLAN,c.source)?;let paid=ledger.paid(p.deployment,&c)?;
                Ok(json!({"claim_key":hx(key),"account_id":c.recipient,"recipient_identity":hx(c.address),"asset_id":src.asset,"source_account_id":src.account,"cap_raw":c.cap.to_string(),"paid_raw":paid.to_string(),"remaining_raw":(c.cap-paid).to_string()}))
            }).collect::<Result<Vec<_>>>()?;
            let m = ledger.metadata;
            let confirmed_capacity_funding = journal
                .iter()
                .filter_map(|entry| {
                    entry.capacity_funding.as_ref().map(|funding| {
                        json!({
                            "transaction_hash": &entry.transaction_hash,
                            "funding": funding
                        })
                    })
                })
                .collect::<Vec<_>>();
            let report = json!({"version":s::FORMAT_VERSION,"status":"canonical_state_reconstructed","claims":claim_details,"account_root":hx(native.root()),"confirmed_transactions":journal.len(),"confirmed_capacity_funding":confirmed_capacity_funding,"rollup_outpoint":b.rollup_cell.out_point,"sequence":m.map(|m|m.sequence),"state_root":m.map(|m|hx(m.state_root)),"sources":p.source_count,"lots":p.reserve_count,"paid_claims":ledger.records[s::PAID].len(),"next_operation":ledger.next_with_profile_hash(p.hash(),4)?.map(|(op,_)|format!("{op:?}")),"epoch0_accounts":accounts.into_iter().map(|((address,asset),(cap,paid))|json!({"address":address,"asset_id":asset,"entitlement_raw":cap.to_string(),"paid_raw":paid.to_string(),"remaining_raw":(cap-paid).to_string()})).collect::<Vec<_>>(),"coverage":backing});
            save(&output, &report)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}

fn capture(protocol: Protocol, rpc: &str, indexer: &str, config: OutPoint) -> Result<Bundle> {
    if protocol == Protocol::V1 {
        network::capture(rpc, indexer, config)
    } else {
        adapter::bundle(crate::v0::capture_at(rpc, indexer, config)?, protocol)
    }
}

fn config_point(protocol: Protocol, point: Option<&str>) -> Result<OutPoint> {
    network::config_outpoint(point.or(if protocol == Protocol::V0 {
        Some("0xc8502347882e91a849146f20dcff59d413056be9093c83ea17ed88339596f142:0x0")
    } else {
        None
    }))
}
