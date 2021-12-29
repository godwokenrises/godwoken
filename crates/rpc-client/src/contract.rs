use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, Result};
use arc_swap::{ArcSwap, Guard};
use async_jsonrpc_client::{HttpClient, Params as ClientParams, Transport};
use gw_config::{BlockProducerConfig, ContractTypeScriptConfig, ContractsCellDep};
use gw_jsonrpc_types::blockchain::{CellDep, Script};
use gw_types::packed::RollupConfig;
use gw_types::prelude::Pack;
use serde_json::json;

use crate::indexer_types::{Cell, Order, Pagination, ScriptType, SearchKey};
use crate::rpc_client::RPCClient;
use crate::utils::to_result;

// Used in block producer and challenge
#[derive(Clone)]
pub struct ContractsCellDepManager {
    rpc_client: RPCClient,
    scripts: Arc<ContractTypeScriptConfig>,
    deps: Arc<ArcSwap<ContractsCellDep>>,
}

impl ContractsCellDepManager {
    pub async fn build(rpc_client: RPCClient, scripts: ContractTypeScriptConfig) -> Result<Self> {
        let now = Instant::now();
        let deps = query_cell_deps(&rpc_client, &scripts).await?;
        log::trace!("[contracts dep] build {}ms", now.elapsed().as_millis());

        Ok(Self {
            rpc_client,
            scripts: Arc::new(scripts),
            deps: Arc::new(ArcSwap::from_pointee(deps)),
        })
    }

    pub fn load(&self) -> Guard<Arc<ContractsCellDep>> {
        self.deps.load()
    }

    pub async fn refresh(&self) -> Result<()> {
        log::info!("[contracts dep] refresh");

        let now = Instant::now();
        let deps = query_cell_deps(&self.rpc_client, &self.scripts).await?;
        log::trace!("[contracts dep] refresh {}ms", now.elapsed().as_millis());

        self.deps.store(Arc::new(deps));
        Ok(())
    }
}

pub fn check_script(
    script_config: &ContractTypeScriptConfig,
    rollup_config: &RollupConfig,
    rollup_type_script: &Script,
) -> Result<()> {
    if script_config.state_validator.hash() != rollup_type_script.code_hash {
        bail!("state validator hash not match");
    }
    if script_config.deposit_lock.hash().pack() != rollup_config.deposit_script_type_hash() {
        bail!("deposit lock hash not match one in rollup config");
    }
    if script_config.stake_lock.hash().pack() != rollup_config.stake_script_type_hash() {
        bail!("stake lock hash not match one in rollup config");
    }
    if script_config.custodian_lock.hash().pack() != rollup_config.custodian_script_type_hash() {
        bail!("custodian lock hash not match one in rollup config");
    }
    if script_config.withdrawal_lock.hash().pack() != rollup_config.withdrawal_script_type_hash() {
        bail!("withdrawal lock hash not match one in rollup config");
    }
    if script_config.challenge_lock.hash().pack() != rollup_config.challenge_script_type_hash() {
        bail!("challenge lock hash not match one in rollup config");
    }

    for (eoa_hash, eoa_script) in script_config.allowed_eoa_scripts.iter() {
        let type_hashes: Vec<_> = {
            let type_hashes = rollup_config.allowed_eoa_type_hashes();
            type_hashes.into_iter().collect()
        };
        if eoa_hash.pack() != eoa_script.hash().pack()
            || !type_hashes.iter().any(|h| h == &eoa_hash.pack())
        {
            bail!("unknown eoa script {}", eoa_hash);
        }
    }

    for (contract_hash, contract_script) in script_config.allowed_contract_scripts.iter() {
        let type_hashes: Vec<_> = {
            let type_hashes = rollup_config.allowed_contract_type_hashes();
            type_hashes.into_iter().collect()
        };
        if contract_hash.pack() != contract_script.hash().pack()
            || !type_hashes.iter().any(|h| h == &contract_hash.pack())
        {
            bail!("unknown contract script {}", contract_hash);
        }
    }

    Ok(())
}

pub async fn query_cell_deps(
    rpc_client: &RPCClient,
    script_config: &ContractTypeScriptConfig,
) -> Result<ContractsCellDep> {
    let query = |contract, type_script: Script| -> _ {
        query_by_type_script(rpc_client, contract, type_script)
    };

    let rollup_cell_type = query("state validator", script_config.state_validator.clone()).await?;
    let deposit_cell_lock = query("deposit", script_config.deposit_lock.clone()).await?;
    let stake_cell_lock = query("stake", script_config.stake_lock.clone()).await?;
    let custodian_cell_lock = query("custodian", script_config.custodian_lock.clone()).await?;
    let withdrawal_cell_lock = query("withdraw", script_config.withdrawal_lock.clone()).await?;
    let challenge_cell_lock = query("challenge", script_config.challenge_lock.clone()).await?;
    let l1_sudt_type = query("l1 sudt", script_config.l1_sudt.clone()).await?;

    let mut allowed_eoa_locks = HashMap::with_capacity(script_config.allowed_eoa_scripts.len());
    for (eoa_hash, eoa_script) in script_config.allowed_eoa_scripts.iter() {
        let eoa_lock = query("allowed eoa", eoa_script.clone()).await?;
        allowed_eoa_locks.insert(eoa_hash.to_owned(), eoa_lock);
    }

    let mut allowed_contract_types =
        HashMap::with_capacity(script_config.allowed_contract_scripts.len());
    for (contract_hash, contract_script) in script_config.allowed_contract_scripts.iter() {
        let contract_type = query("allowed contract", contract_script.clone()).await?;
        allowed_contract_types.insert(contract_hash.to_owned(), contract_type);
    }

    Ok(ContractsCellDep {
        rollup_cell_type,
        deposit_cell_lock,
        stake_cell_lock,
        custodian_cell_lock,
        withdrawal_cell_lock,
        challenge_cell_lock,
        l1_sudt_type,
        allowed_eoa_locks,
        allowed_contract_types,
    })
}

pub async fn query_type_script(
    ckb_client: &HttpClient,
    contract: &str,
    cell_dep: CellDep,
) -> Result<Script> {
    use gw_jsonrpc_types::ckb_jsonrpc_types::TransactionWithStatus;

    let tx_hash = cell_dep.out_point.tx_hash;
    let get_transaction = ckb_client.request(
        "get_transaction",
        Some(ClientParams::Array(vec![json!(tx_hash)])),
    );
    let tx = match to_result::<Option<TransactionWithStatus>>(get_transaction.await?)? {
        Some(tx_with_status) => tx_with_status.transaction.inner,
        None => bail!("{} {} tx not found", contract, tx_hash),
    };

    match tx.outputs.get(cell_dep.out_point.index.value() as usize) {
        Some(output) => match output.type_.as_ref() {
            Some(script) => Ok(script.to_owned().into()),
            None => Err(anyhow!("{} {} tx hasn't type script", contract, tx_hash)),
        },
        None => Err(anyhow!("{} {} tx index not found", contract, tx_hash)),
    }
}

// For old config compatibility
#[allow(deprecated)]
#[deprecated]
pub async fn query_type_script_from_old_config(
    rpc_client: &RPCClient,
    config: &BlockProducerConfig,
) -> Result<ContractTypeScriptConfig> {
    let query = |contract: &'static str, cell_dep: CellDep| -> _ {
        query_type_script(&rpc_client.ckb, contract, cell_dep)
    };

    let state_validator = query("state validator", config.rollup_cell_type_dep.clone()).await?;
    let deposit_lock = query("deposit lock", config.deposit_cell_lock_dep.clone()).await?;
    let stake_lock = query("stake lock", config.stake_cell_lock_dep.clone()).await?;
    let custodian_lock = query("custodian lock", config.custodian_cell_lock_dep.clone()).await?;
    let withdrawal_lock = query("withdrawal lock", config.withdrawal_cell_lock_dep.clone()).await?;
    let challenge_lock = query("challenge lock", config.challenge_cell_lock_dep.clone()).await?;
    let l1_sudt = query("l1 sudt", config.l1_sudt_type_dep.clone()).await?;

    let mut allowed_eoa_scripts = HashMap::with_capacity(config.allowed_eoa_deps.len());
    for (type_hash, cell_dep) in config.allowed_eoa_deps.iter() {
        let eoa_type_script = query("eoa", cell_dep.clone()).await?;
        allowed_eoa_scripts.insert(type_hash.to_owned(), eoa_type_script);
    }

    let mut allowed_contract_scripts = HashMap::with_capacity(config.allowed_contract_deps.len());
    for (type_hash, cell_dep) in config.allowed_contract_deps.iter() {
        let contract_type_script = query("contract", cell_dep.clone()).await?;
        allowed_contract_scripts.insert(type_hash.to_owned(), contract_type_script);
    }

    Ok(ContractTypeScriptConfig {
        state_validator,
        deposit_lock,
        stake_lock,
        custodian_lock,
        withdrawal_lock,
        challenge_lock,
        l1_sudt,
        allowed_eoa_scripts,
        allowed_contract_scripts,
    })
}

async fn query_by_type_script(
    rpc_client: &RPCClient,
    contract: &'static str,
    type_script: Script,
) -> Result<CellDep> {
    use gw_jsonrpc_types::ckb_jsonrpc_types::{CellDep, DepType, Uint32};

    let search_key = SearchKey {
        script: type_script.clone().into(),
        script_type: ScriptType::Type,
        filter: None,
    };
    let order = Order::Desc;
    let limit = Uint32::from(1);

    let get_contract_cell = rpc_client.indexer.client().request(
        "get_cells",
        Some(ClientParams::Array(vec![
            json!(search_key),
            json!(order),
            json!(limit),
        ])),
    );

    let mut cells: Pagination<Cell> = to_result(get_contract_cell.await?)?;
    match cells.objects.pop() {
        Some(cell) => Ok(Into::into(CellDep {
            dep_type: DepType::Code,
            out_point: cell.out_point,
        })),
        None => Err(anyhow!("{} {} not found", contract, type_script.hash())),
    }
}
