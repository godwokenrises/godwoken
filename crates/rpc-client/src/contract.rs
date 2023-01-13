use std::{collections::HashMap, sync::Arc, time::Instant};

use anyhow::{anyhow, bail, Result};
use arc_swap::ArcSwap;
pub use arc_swap::Guard;
use gw_config::{ContractsCellDep, SystemTypeScriptConfig};
use gw_jsonrpc_types::{
    ckb_jsonrpc_types::{CellDep, Script},
    JsonCalcHash,
};
use gw_types::{packed::RollupConfig, prelude::*};
use tracing::instrument;

use crate::{
    indexer_types::{Order, ScriptType, SearchKey},
    rpc_client::RPCClient,
};

// Used in block producer and challenge
#[derive(Clone)]
pub struct ContractsCellDepManager {
    rpc_client: RPCClient,
    scripts: Arc<SystemTypeScriptConfig>,
    deps: Arc<ArcSwap<ContractsCellDep>>,
}

impl ContractsCellDepManager {
    pub async fn build(
        rpc_client: RPCClient,
        scripts: SystemTypeScriptConfig,
        rollup_config_cell_dep: CellDep,
    ) -> Result<Self> {
        let now = Instant::now();
        let deps = query_cell_deps(&rpc_client, &scripts, rollup_config_cell_dep).await?;
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

    pub fn load_scripts(&self) -> &SystemTypeScriptConfig {
        &self.scripts
    }

    #[instrument(skip_all)]
    pub async fn refresh(&self) -> Result<()> {
        log::info!("[contracts dep] refresh");

        // rollup_config_cell is identify by data_hash but not type_hash
        let rollup_config_cell_dep = self.load().rollup_config.clone();

        let now = Instant::now();
        let deps = query_cell_deps(&self.rpc_client, &self.scripts, rollup_config_cell_dep).await?;
        log::trace!("[contracts dep] refresh {}ms", now.elapsed().as_millis());

        self.deps.store(Arc::new(deps));
        Ok(())
    }
}

pub fn check_script(
    script_config: &SystemTypeScriptConfig,
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

    for eoa_script in script_config.allowed_eoa_scripts.iter() {
        let eoa_hash = eoa_script.hash();
        let type_hashes: Vec<_> = {
            let type_hashes = rollup_config.allowed_eoa_type_hashes();
            type_hashes.into_iter().collect()
        };
        if eoa_hash.pack() != eoa_script.hash().pack()
            || !type_hashes.iter().any(|h| h.hash() == eoa_hash.pack())
        {
            bail!("unknown eoa script {}", eoa_hash);
        }
    }

    for contract_script in script_config.allowed_contract_scripts.iter() {
        let contract_hash = contract_script.hash();
        let type_hashes: Vec<_> = {
            let type_hashes = rollup_config.allowed_contract_type_hashes();
            type_hashes.into_iter().collect()
        };
        if contract_hash.pack() != contract_script.hash().pack()
            || !type_hashes.iter().any(|h| h.hash() == contract_hash.pack())
        {
            bail!("unknown contract script {}", contract_hash);
        }
    }

    Ok(())
}

pub async fn query_cell_deps(
    rpc_client: &RPCClient,
    script_config: &SystemTypeScriptConfig,
    rollup_config_cell_dep: CellDep,
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
    let omni_lock = query("omni", script_config.omni_lock.clone()).await?;

    let mut allowed_eoa_locks = HashMap::with_capacity(script_config.allowed_eoa_scripts.len());
    for eoa_script in script_config.allowed_eoa_scripts.iter() {
        let eoa_hash = eoa_script.hash();
        let eoa_lock = query("allowed eoa", eoa_script.clone()).await?;
        allowed_eoa_locks.insert(eoa_hash.to_owned(), eoa_lock);
    }

    let mut allowed_contract_types =
        HashMap::with_capacity(script_config.allowed_contract_scripts.len());
    for contract_script in script_config.allowed_contract_scripts.iter() {
        let contract_hash = contract_script.hash();
        let contract_type = query("allowed contract", contract_script.clone()).await?;
        allowed_contract_types.insert(contract_hash.to_owned(), contract_type);
    }

    Ok(ContractsCellDep {
        rollup_config: rollup_config_cell_dep,
        rollup_cell_type,
        deposit_cell_lock,
        stake_cell_lock,
        custodian_cell_lock,
        withdrawal_cell_lock,
        challenge_cell_lock,
        l1_sudt_type,
        omni_lock,
        allowed_eoa_locks,
        allowed_contract_types,
    })
}

async fn query_by_type_script(
    rpc_client: &RPCClient,
    contract: &'static str,
    type_script: Script,
) -> Result<CellDep> {
    use gw_jsonrpc_types::ckb_jsonrpc_types::{DepType, Uint32};

    let search_key = SearchKey {
        script: type_script.clone(),
        script_type: ScriptType::Type,
        filter: None,
    };
    let order = Order::Desc;
    let limit = Uint32::from(1);

    let mut cells = rpc_client
        .indexer
        .get_cells(&search_key, &order, limit, &None)
        .await?;
    match cells.objects.pop() {
        Some(cell) => Ok(Into::into(CellDep {
            dep_type: DepType::Code,
            out_point: cell.out_point,
        })),
        None => Err(anyhow!("{} {} not found", contract, type_script.hash())),
    }
}
