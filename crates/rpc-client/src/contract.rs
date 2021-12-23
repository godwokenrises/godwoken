use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use async_jsonrpc_client::{Params as ClientParams, Transport};
use futures::Future;
use gw_common::H256;
use gw_jsonrpc_types::blockchain::CellDep;
use gw_jsonrpc_types::ckb_jsonrpc_types::Uint32;
use gw_types::packed::{Byte32, RollupConfig};
use gw_types::prelude::Unpack;
use serde_json::json;

use crate::indexer_types::{Cell, Order, Pagination, ScriptType, SearchKey};
use crate::rpc_client::RPCClient;
use crate::utils::{to_result, DEFAULT_QUERY_LIMIT, TYPE_ID_CODE_HASH};

pub struct LiveContractDeps {
    pub rollup_cell_type_dep: CellDep,
    pub deposit_cell_lock_dep: CellDep,
    pub stake_cell_lock_dep: CellDep,
    pub custodian_cell_lock_dep: CellDep,
    pub withdrawal_cell_lock_dep: CellDep,
    pub challenge_cell_lock_dep: CellDep,
    pub l1_sudt_type_dep: CellDep,
    pub allowed_eoa_deps: HashMap<ckb_fixed_hash::H256, CellDep>,
    pub allowed_contract_deps: HashMap<ckb_fixed_hash::H256, CellDep>,
}

// TODO: query by lock script?
pub async fn query_cell_deps(
    rpc_client: &RPCClient,
    rollup_config: &RollupConfig,
) -> Result<LiveContractDeps> {
    let query = |contract, type_hash: Byte32| -> _ {
        let type_hash = type_hash.unpack();
        ensure_exist(
            contract,
            type_hash,
            query_by_type_hash(rpc_client, type_hash),
        )
    };

    let rollup_cell = {
        let query = rpc_client.query_rollup_cell().await?;
        query.ok_or_else(|| anyhow!("rollup not found"))?
    };
    let state_validator_script_type_hash = {
        let opt_type_script = rollup_cell.output.type_().to_opt();
        opt_type_script.expect("rollup type").code_hash()
    };
    let rollup_cell_type_dep = query("state validator", state_validator_script_type_hash).await?;

    let deposit_cell_lock_dep = query("deposit", rollup_config.deposit_script_type_hash()).await?;
    let stake_cell_lock_dep = query("stake", rollup_config.stake_script_type_hash()).await?;
    let custodian_cell_lock_dep =
        query("custodian", rollup_config.custodian_script_type_hash()).await?;
    let withdrawal_cell_lock_dep =
        query("withdrwal", rollup_config.withdrawal_script_type_hash()).await?;
    let challenge_cell_lock_dep =
        query("challenge", rollup_config.challenge_script_type_hash()).await?;
    let l1_sudt_type_dep = query("l1 sudt", rollup_config.l1_sudt_script_type_hash()).await?;

    let mut allowed_eoa_deps =
        HashMap::with_capacity(rollup_config.allowed_eoa_type_hashes().len());
    for eoa_type_hash in rollup_config.allowed_eoa_type_hashes().into_iter() {
        let eoa_key = ckb_fixed_hash::H256(eoa_type_hash.unpack());
        let eoa_dep = query("allowed eoa", eoa_type_hash).await?;
        allowed_eoa_deps.insert(eoa_key, eoa_dep);
    }
    let mut allowed_contract_deps =
        HashMap::with_capacity(rollup_config.allowed_contract_type_hashes().len());
    for contract_type_hash in rollup_config.allowed_contract_type_hashes().into_iter() {
        let contract_key = ckb_fixed_hash::H256(contract_type_hash.unpack());
        let contract_dep = query("allowed contract", contract_type_hash).await?;
        allowed_contract_deps.insert(contract_key, contract_dep);
    }

    Ok(LiveContractDeps {
        rollup_cell_type_dep,
        deposit_cell_lock_dep,
        stake_cell_lock_dep,
        custodian_cell_lock_dep,
        withdrawal_cell_lock_dep,
        challenge_cell_lock_dep,
        l1_sudt_type_dep,
        allowed_eoa_deps,
        allowed_contract_deps,
    })
}

async fn ensure_exist(
    contract: &'static str,
    type_hash: H256,
    query: impl Future<Output = Result<Option<CellDep>>>,
) -> Result<CellDep> {
    use gw_types::prelude::Pack;

    let opt_dep = query.await.with_context(|| contract)?;
    opt_dep.ok_or_else(|| anyhow!("{} {} dep not found", contract, type_hash.pack()))
}

async fn query_by_type_hash(
    rpc_client: &RPCClient,
    type_script_hash: H256,
) -> Result<Option<CellDep>> {
    use ckb_types::core::{DepType, ScriptHashType};
    use ckb_types::packed::{CellDep, CellOutput, Script};
    use ckb_types::prelude::{Builder, Entity, Pack};

    // Reference: crates/tools/src/deploy_scripts.rs
    let contract_type_script = Script::new_builder()
        .code_hash(TYPE_ID_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let type_script_hash = ckb_fixed_hash::H256(type_script_hash.into()).pack();

    let search_key = SearchKey {
        script: contract_type_script.into(),
        script_type: ScriptType::Type,
        filter: None,
    };
    let order = Order::Desc;
    let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

    let mut dep = None;
    let mut cursor = None;
    let indexer = rpc_client.indexer.client();
    while dep.is_none() {
        let cells: Pagination<Cell> = to_result(
            indexer
                .request(
                    "get_cells",
                    Some(ClientParams::Array(vec![
                        json!(search_key),
                        json!(order),
                        json!(limit),
                        json!(cursor),
                    ])),
                )
                .await?,
        )?;

        dep = cells.objects.into_iter().find_map(|cell| {
            let output: CellOutput = cell.output.into();
            match output.type_().to_opt() {
                Some(type_script) if type_script.calc_script_hash() == type_script_hash => Some(
                    CellDep::new_builder()
                        .out_point(cell.out_point.into())
                        .dep_type(DepType::Code.into())
                        .build(),
                ),
                _ => None,
            }
        });

        if cells.last_cursor.is_empty() {
            break;
        }
        cursor = Some(cells.last_cursor);
    }

    Ok(dep.map(|d| gw_types::packed::CellDep::new_unchecked(d.as_bytes()).into()))
}
