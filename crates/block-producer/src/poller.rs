use crate::{
    indexer_types::{Order, Pagination, ScriptType, SearchKey, SearchKeyFilter, Tx},
    rpc_client::RPCClient,
};
use crate::{types::ChainEvent, utils::to_result};
use anyhow::{anyhow, Result};
use async_jsonrpc_client::{Params as ClientParams, Transport};
use ckb_fixed_hash::H256;
use gw_chain::chain::{Chain, L1Action, L1ActionContext, SyncParam};
use gw_generator::{ChallengeContext, RollupContext};
use gw_jsonrpc_types::ckb_jsonrpc_types::{BlockNumber, HeaderView, TransactionWithStatus, Uint32};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        CellOutput, ChallengeLockArgs, ChallengeLockArgsReader, DepositLockArgs, DepositRequest,
        L2BlockCommittedInfo, RollupAction, RollupActionUnion, Script, Transaction, WitnessArgs,
        WitnessArgsReader,
    },
    prelude::*,
};
use gw_web3_indexer::indexer::Web3Indexer;
use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;

pub struct ChainUpdater {
    chain: Arc<Mutex<Chain>>,
    rpc_client: RPCClient,
    last_tx_hash: Option<H256>,
    rollup_context: RollupContext,
    rollup_type_script: ckb_types::packed::Script,
    web3_indexer: Option<Web3Indexer>,
}

impl ChainUpdater {
    pub fn new(
        chain: Arc<Mutex<Chain>>,
        rpc_client: RPCClient,
        rollup_context: RollupContext,
        rollup_type_script: Script,
        web3_indexer: Option<Web3Indexer>,
    ) -> ChainUpdater {
        let rollup_type_script =
            ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());

        ChainUpdater {
            chain,
            rpc_client,
            rollup_context,
            rollup_type_script,
            last_tx_hash: None,
            web3_indexer,
        }
    }

    // Start syncing
    pub async fn handle_event(&mut self, _event: ChainEvent) -> Result<()> {
        let rollup_type_script = self.rollup_type_script.clone();
        let tip_l1_block = self.chain.lock().local_state().last_synced().number();
        let search_key = SearchKey {
            script: rollup_type_script.clone().into(),
            script_type: ScriptType::Type,
            filter: Some(SearchKeyFilter {
                script: None,
                output_data_len_range: None,
                output_capacity_range: None,
                block_range: Some([
                    BlockNumber::from(tip_l1_block.unpack() + 1),
                    BlockNumber::from(u64::max_value()),
                ]),
            }),
        };
        let order = Order::Asc;
        let limit = Uint32::from(1000);

        // TODO: right now this logic does not handle forks well, we will need
        // to tweak this.
        // TODO: the syncing logic here works under the assumption that a single
        // L1 CKB block can contain at most one L2 Godwoken block. The logic
        // here needs revising, once we relax this constraint for more performance.
        let mut last_cursor = None;
        loop {
            let txs: Pagination<Tx> = to_result(
                self.rpc_client
                    .indexer_client
                    .request(
                        "get_transactions",
                        Some(ClientParams::Array(vec![
                            json!(search_key),
                            json!(order),
                            json!(limit),
                            json!(last_cursor),
                        ])),
                    )
                    .await?,
            )?;
            if txs.objects.is_empty() {
                break;
            }
            last_cursor = Some(txs.last_cursor);

            log::debug!("Poll transactions: {}", txs.objects.len());
            self.update(&txs.objects).await?;
        }
        Ok(())
    }

    pub async fn update(&mut self, txs: &[Tx]) -> anyhow::Result<()> {
        for tx in txs.iter() {
            self.update_single(&tx.tx_hash).await?;
        }

        Ok(())
    }

    async fn update_single(&mut self, tx_hash: &H256) -> anyhow::Result<()> {
        if let Some(last_tx_hash) = &self.last_tx_hash {
            if last_tx_hash == tx_hash {
                return Ok(());
            }
        }
        self.last_tx_hash = Some(tx_hash.clone());

        let tx: Option<TransactionWithStatus> = to_result(
            self.rpc_client
                .ckb_client
                .request(
                    "get_transaction",
                    Some(ClientParams::Array(vec![json!(tx_hash)])),
                )
                .await?,
        )?;
        let tx_with_status =
            tx.ok_or_else(|| anyhow::anyhow!("Cannot locate transaction: {:x}", tx_hash))?;
        let tx = {
            let tx: ckb_types::packed::Transaction = tx_with_status.transaction.inner.into();
            Transaction::new_unchecked(tx.as_bytes())
        };
        let block_hash = tx_with_status.tx_status.block_hash.ok_or_else(|| {
            anyhow::anyhow!("Transaction {:x} is not committed on chain!", tx_hash)
        })?;
        let header_view: Option<HeaderView> = to_result(
            self.rpc_client
                .ckb_client
                .request(
                    "get_header",
                    Some(ClientParams::Array(vec![json!(block_hash)])),
                )
                .await?,
        )?;
        let header_view =
            header_view.ok_or_else(|| anyhow::anyhow!("Cannot locate block: {:x}", block_hash))?;
        let l2block_committed_info = L2BlockCommittedInfo::new_builder()
            .number(header_view.inner.number.value().pack())
            .block_hash(block_hash.0.pack())
            .transaction_hash(tx_hash.pack())
            .build();

        let rollup_action = self.extract_rollup_action(&tx)?;
        let context = match rollup_action.to_enum() {
            RollupActionUnion::RollupSubmitBlock(submitted) => {
                let requests = self.extract_deposit_requests(&tx).await?;

                L1ActionContext::SubmitBlock {
                    l2block: submitted.block(),
                    deposit_requests: requests,
                    reverted_block_hashes: submitted.reverted_block_hashes().unpack(),
                }
            }
            RollupActionUnion::RollupEnterChallenge(challenge) => {
                let challenge_cell = {
                    let opt_cell = self.rpc_client.query_verified_challenge_cell().await?;
                    opt_cell.ok_or_else(|| anyhow!("challenge cell not found"))?
                };
                let challenge_lock_args = {
                    let lock_args: Bytes = challenge_cell.output.lock().args().unpack();
                    match ChallengeLockArgsReader::verify(&lock_args, false) {
                        Ok(_) => ChallengeLockArgs::new_unchecked(lock_args),
                        Err(err) => return Err(anyhow!("invalid challenge lock args {}", err)),
                    }
                };

                let target = challenge_lock_args.target();
                let witness = challenge.witness();
                L1ActionContext::Challenge {
                    context: ChallengeContext { target, witness },
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
            transaction: tx.clone(),
            l2block_committed_info,
            context,
        };
        // TODO: handle layer1 fork
        let sync_param = SyncParam {
            reverts: vec![],
            updates: vec![update],
        };
        self.chain.lock().sync(sync_param)?;

        // TODO sync missed block
        match &self.web3_indexer {
            Some(indexer) => {
                indexer.store(self.chain.lock().store().clone(), &tx).await;
            }
            None => {}
        }

        Ok(())
    }

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

    async fn extract_deposit_requests(&self, tx: &Transaction) -> Result<Vec<DepositRequest>> {
        let mut results = vec![];
        for input in tx.raw().inputs().into_iter() {
            // Load cell denoted by the transaction input
            let tx_hash: H256 = input.previous_output().tx_hash().unpack();
            let index = input.previous_output().index().unpack();
            let tx: Option<TransactionWithStatus> = to_result(
                self.rpc_client
                    .ckb_client
                    .request(
                        "get_transaction",
                        Some(ClientParams::Array(vec![json!(tx_hash)])),
                    )
                    .await?,
            )?;
            let tx_with_status =
                tx.ok_or_else(|| anyhow::anyhow!("Cannot locate transaction: {:x}", tx_hash))?;
            let tx = {
                let tx: ckb_types::packed::Transaction = tx_with_status.transaction.inner.into();
                Transaction::new_unchecked(tx.as_bytes())
            };
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
                results.push(deposit_request);
            }
        }
        Ok(results)
    }
}

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
    let rollup_type_script_hash: [u8; 32] = rollup_context.rollup_script_hash.into();
    if args.slice(0..32) != rollup_type_script_hash[..] {
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
        .build();
    Some(deposit_request)
}
