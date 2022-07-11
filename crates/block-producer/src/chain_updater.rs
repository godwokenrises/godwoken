#![allow(clippy::mutable_key_type)]

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{anyhow, Context, Result};
use ckb_fixed_hash::H256;
use gw_chain::chain::{Chain, ChallengeCell, L1Action, L1ActionContext, SyncParam};
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::RollupContext,
    packed::{
        CellInfo, CellInput, CellOutput, ChallengeLockArgs, ChallengeLockArgsReader, DepositInfo,
        DepositInfoVec, DepositLockArgs, DepositRequest, L2Block, OutPoint, RollupAction,
        RollupActionUnion, Script, Transaction, WithdrawalRequestExtra, WitnessArgs,
        WitnessArgsReader,
    },
    prelude::*,
};
use tokio::sync::Mutex;
use tracing::instrument;

#[derive(thiserror::Error, Debug)]
#[error("chain updater query l1 tx {tx_hash} error {source}")]
pub struct QueryL1TxError {
    tx_hash: H256,
    source: anyhow::Error,
}

impl QueryL1TxError {
    pub fn new<E: Into<anyhow::Error>>(tx_hash: &H256, source: E) -> Self {
        QueryL1TxError {
            tx_hash: H256(tx_hash.0),
            source: source.into(),
        }
    }
}

pub struct ChainUpdater {
    chain: Arc<Mutex<Chain>>,
    rpc_client: RPCClient,
    rollup_context: RollupContext,
    rollup_type_script: ckb_types::packed::Script,
}

impl ChainUpdater {
    pub fn new(
        chain: Arc<Mutex<Chain>>,
        rpc_client: RPCClient,
        rollup_context: RollupContext,
        rollup_type_script: Script,
    ) -> ChainUpdater {
        let rollup_type_script =
            ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());

        ChainUpdater {
            chain,
            rpc_client,
            rollup_context,
            rollup_type_script,
        }
    }

    #[instrument(skip_all)]
    pub async fn update_single(&self, tx_hash: &H256) -> anyhow::Result<()> {
        let tx = self
            .rpc_client
            .ckb
            .get_transaction(tx_hash.0.into())
            .await?
            .context("get transaction")?;

        let rollup_action = self.extract_rollup_action(&tx)?;
        let context = match rollup_action.to_enum() {
            RollupActionUnion::RollupSubmitBlock(submitted) => {
                let l2block = submitted.block();
                let (deposit_info_vec, asset_type_scripts) =
                    self.extract_deposit_requests(&tx).await?;
                let withdrawals = self.extract_withdrawals(&tx, &l2block).await?;

                L1ActionContext::SubmitBlock {
                    l2block,
                    deposit_info_vec,
                    deposit_asset_scripts: asset_type_scripts,
                    withdrawals,
                }
            }
            RollupActionUnion::RollupEnterChallenge(entered) => {
                let (challenge_cell, challenge_lock_args) =
                    self.extract_challenge_context(&tx).await?;

                L1ActionContext::Challenge {
                    cell: challenge_cell,
                    target: challenge_lock_args.target(),
                    witness: entered.witness(),
                }
            }
            RollupActionUnion::RollupCancelChallenge(_) => L1ActionContext::CancelChallenge,
            RollupActionUnion::RollupRevert(reverted) => {
                let reverted_blocks = reverted.reverted_blocks().into_iter();
                L1ActionContext::Revert {
                    reverted_blocks: reverted_blocks.collect(),
                }
            }
        };

        let update = L1Action {
            transaction: tx,
            context,
        };
        let sync_param = SyncParam {
            reverts: vec![],
            updates: vec![update],
        };
        self.chain.lock().await.sync(sync_param).await?;

        Ok(())
    }

    #[instrument(skip_all)]
    fn extract_rollup_action(&self, tx: &Transaction) -> Result<RollupAction> {
        let rollup_type_hash: [u8; 32] = {
            let hash = self.rollup_type_script.calc_script_hash();
            ckb_types::prelude::Unpack::unpack(&hash)
        };

        // find rollup state cell from outputs
        let (i, _) = {
            let outputs = tx.raw().outputs().into_iter();
            let find_rollup = outputs.enumerate().find(|(_i, output)| {
                output.type_().to_opt().map(|type_| type_.hash()) == Some(rollup_type_hash)
            });
            find_rollup.ok_or_else(|| anyhow!("no rollup cell found"))?
        };

        let witness: Bytes = {
            let rollup_witness = tx.witnesses().get(i).ok_or_else(|| anyhow!("no witness"))?;
            rollup_witness.unpack()
        };

        let witness_args = match WitnessArgsReader::verify(&witness, false) {
            Ok(_) => WitnessArgs::new_unchecked(witness),
            Err(_) => return Err(anyhow!("invalid witness")),
        };

        let output_type: Bytes = {
            let type_ = witness_args.output_type();
            let should_exist = type_.to_opt().ok_or_else(|| anyhow!("no output type"))?;
            should_exist.unpack()
        };

        RollupAction::from_slice(&output_type).map_err(|e| anyhow!("invalid rollup action {}", e))
    }

    #[instrument(skip_all)]
    async fn extract_challenge_context(
        &self,
        tx: &Transaction,
    ) -> Result<(ChallengeCell, ChallengeLockArgs)> {
        let challenge_script_type_hash = self
            .rollup_context
            .rollup_config
            .challenge_script_type_hash();

        let outputs = tx.as_reader().raw().outputs();
        let outputs_data = tx.as_reader().raw().outputs_data();
        for (index, (output, output_data)) in outputs.iter().zip(outputs_data.iter()).enumerate() {
            if output.lock().code_hash().as_slice() != challenge_script_type_hash.as_slice()
                || output.lock().hash_type().to_entity() != ScriptHashType::Type.into()
            {
                continue;
            }

            let lock_args = {
                let args: Bytes = output.lock().args().unpack();
                match ChallengeLockArgsReader::verify(&args.slice(32..), false) {
                    Ok(_) => ChallengeLockArgs::new_unchecked(args.slice(32..)),
                    Err(err) => return Err(anyhow!("invalid challenge lock args {}", err)),
                }
            };

            let input = {
                let out_point = OutPoint::new_builder()
                    .tx_hash(tx.hash().pack())
                    .index((index as u32).pack())
                    .build();

                CellInput::new_builder().previous_output(out_point).build()
            };

            let cell = ChallengeCell {
                input,
                output: output.to_entity(),
                output_data: output_data.unpack(),
            };

            return Ok((cell, lock_args));
        }

        unreachable!("challenge output not found");
    }

    #[instrument(skip_all)]
    async fn extract_deposit_requests(
        &self,
        tx: &Transaction,
    ) -> Result<(DepositInfoVec, HashSet<Script>)> {
        let mut deposits = DepositInfoVec::new_builder();
        let mut asset_type_scripts = HashSet::new();
        for input in tx.raw().inputs().into_iter() {
            // Load cell denoted by the transaction input
            let tx_hash: H256 = input.previous_output().tx_hash().unpack();
            let index = input.previous_output().index().unpack();
            let tx = self
                .rpc_client
                .ckb
                .get_transaction(tx_hash.0.into())
                .await?
                .ok_or_else(|| QueryL1TxError::new(&tx_hash, anyhow!("cannot locate tx")))?;
            let cell_output = tx
                .raw()
                .outputs()
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("OutPoint index out of bound"))?;
            let cell_data = tx
                .raw()
                .outputs_data()
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("OutPoint index out of bound"))?;

            // Check if loaded cell is a deposit request
            if let Some(deposit_request) =
                try_parse_deposit_request(&cell_output, &cell_data.unpack(), &self.rollup_context)
            {
                let info = DepositInfo::new_builder()
                    .cell(
                        CellInfo::new_builder()
                            .out_point(input.previous_output())
                            .data(cell_data)
                            .output(cell_output.clone())
                            .build(),
                    )
                    .request(deposit_request)
                    .build();
                deposits = deposits.push(info);
                if let Some(type_) = &cell_output.type_().to_opt() {
                    asset_type_scripts.insert(type_.clone());
                }
            }
        }
        Ok((deposits.build(), asset_type_scripts))
    }

    async fn extract_withdrawals(
        &self,
        tx: &Transaction,
        block: &L2Block,
    ) -> Result<Vec<WithdrawalRequestExtra>> {
        let mut owner_lock_map = HashMap::with_capacity(block.withdrawals().len());
        for output in tx.raw().outputs().into_iter() {
            if let Some(owner_lock) = try_parse_withdrawal_owner_lock(&output, &self.rollup_context)
            {
                owner_lock_map.insert(owner_lock.hash(), owner_lock);
            }
        }
        // return in block's sort
        let withdrawals = block
            .withdrawals()
            .into_iter()
            .map(|withdrawal| {
                let owner_lock_hash: [u8; 32] = withdrawal.raw().owner_lock_hash().unpack();
                let owner_lock = owner_lock_map
                    .get(&owner_lock_hash)
                    .expect("must exist")
                    .clone();
                WithdrawalRequestExtra::new_builder()
                    .request(withdrawal)
                    .owner_lock(owner_lock)
                    .build()
            })
            .collect();
        Ok(withdrawals)
    }
}

#[instrument(skip_all)]
fn try_parse_deposit_request(
    cell_output: &CellOutput,
    cell_data: &Bytes,
    rollup_context: &RollupContext,
) -> Option<DepositRequest> {
    if cell_output.lock().code_hash() != rollup_context.rollup_config.deposit_script_type_hash()
        || cell_output.lock().hash_type() != ScriptHashType::Type.into()
    {
        return None;
    }
    let args = cell_output.lock().args().raw_data();
    if args.len() < 32 {
        return None;
    }
    if &args[0..32] != rollup_context.rollup_script_hash.as_slice() {
        return None;
    }
    let lock_args = match DepositLockArgs::from_slice(&args.slice(32..)) {
        Ok(lock_args) => lock_args,
        Err(_) => return None,
    };
    // NOTE: In readoly mode, we are only loading on chain data here, timeout validation
    // can be skipped. For generator part, timeout validation needs to be introduced.
    let (amount, sudt_script_hash) = match cell_output.type_().to_opt() {
        Some(script) => {
            if cell_data.len() < 16 {
                return None;
            }
            let mut data = [0u8; 16];
            data.copy_from_slice(&cell_data[0..16]);
            (u128::from_le_bytes(data), script.hash())
        }
        None => (0u128, [0u8; 32]),
    };
    let capacity: u64 = cell_output.capacity().unpack();
    let deposit_request = DepositRequest::new_builder()
        .capacity(capacity.pack())
        .amount(amount.pack())
        .sudt_script_hash(sudt_script_hash.pack())
        .script(lock_args.layer2_lock())
        .registry_id(lock_args.registry_id())
        .build();
    Some(deposit_request)
}

fn try_parse_withdrawal_owner_lock(
    cell_output: &CellOutput,
    rollup_context: &RollupContext,
) -> Option<Script> {
    if cell_output.lock().code_hash() != rollup_context.rollup_config.withdrawal_script_type_hash()
        || cell_output.lock().hash_type() != ScriptHashType::Type.into()
    {
        return None;
    }
    let args = cell_output.lock().args().raw_data();
    if args.len() < 32 {
        return None;
    }
    if &args[0..32] != rollup_context.rollup_script_hash.as_slice() {
        return None;
    }
    let result = gw_utils::withdrawal::parse_lock_args(&args).expect("parse withdrawal extra");
    Some(result.owner_lock)
}
