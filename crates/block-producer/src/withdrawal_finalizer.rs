#![allow(clippy::mutable_key_type)]

use std::{
    collections::{HashMap, HashSet},
    ops::RangeInclusive,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{bail, ensure, Result};
use async_trait::async_trait;
use gw_common::{
    smt::{generate_block_proof, Blake2bHasher},
    sparse_merkle_tree::CompiledMerkleProof,
    CKB_SUDT_SCRIPT_ARGS, H256,
};
use gw_config::{ContractsCellDep, DebugConfig};
use gw_generator::Guard;
use gw_mem_pool::custodian::query_finalized_custodians;
use gw_rpc_client::{contract::ContractsCellDepManager, rpc_client::RPCClient};
use gw_store::traits::chain_store::ChainStore;
use gw_types::{
    core::Status,
    offchain::{
        global_state_from_slice, CellInfo, CollectedCustodianCells, InputCellInfo, RollupContext,
        TxStatus,
    },
    packed::{
        CellDep, CellInput, GlobalState, L2Block, LastFinalizedWithdrawal, OutPoint, RollupAction,
        RollupActionUnion, Script, Transaction, WithdrawalRequestExtra, WitnessArgs,
    },
    prelude::{Builder, Entity, Pack, Unpack},
};
use gw_utils::{
    fee::fill_tx_fee, genesis_info::CKBGenesisInfo, transaction_skeleton::TransactionSkeleton,
    wallet::Wallet,
};
use tracing::instrument;

use crate::{types::ChainEvent, withdrawal::BlockWithdrawals};

const TRANSACTION_FAILED_TO_RESOLVE_ERROR: &str = "TransactionFailedToResolve";
const LOOP_TX_STATUS_INTERVAL: Duration = Duration::from_secs(1);

pub const MAX_FINALIZE_BLOCKS: u32 = 8;
pub const MAX_FINALIZE_WITHDRAWALS: u32 = 60;

pub struct FinalizerArgs {
    pub store: gw_store::Store,
    pub rpc_client: RPCClient,
    pub ckb_genesis_info: CKBGenesisInfo,
    pub contracts_dep_manager: ContractsCellDepManager,
    pub wallet: Wallet,
    pub rollup_config_cell_dep: CellDep,
    pub last_block_submitted_tx: Arc<tokio::sync::RwLock<H256>>,
}

pub struct UserWithdrawalFinalizer {
    inner: DefaultFinalizer,
    last_finalize_tx: Mutex<Option<H256>>,
    debug_config: DebugConfig,
}

impl UserWithdrawalFinalizer {
    pub fn new(args: FinalizerArgs, debug_config: DebugConfig) -> Self {
        let inner = DefaultFinalizer::new(args);

        Self {
            inner,
            last_finalize_tx: Mutex::new(None),
            debug_config,
        }
    }

    pub fn last_finalize_tx(&self) -> Option<H256> {
        *self.last_finalize_tx.lock().expect("lock")
    }

    #[instrument(skip_all, err, name = "user withdrawal finalizer handle_event")]
    pub async fn handle_event(&self, _event: &ChainEvent) -> Result<()> {
        let rpc_client = &self.inner.rpc_client;

        if let Some(tx_hash) = self.last_finalize_tx() {
            match rpc_client.ckb.get_transaction_status(tx_hash).await {
                Err(err) => {
                    tracing::info!(tx = %tx_hash.pack(), error = ?err, "drop finalize tx");
                }
                Ok(None) => {
                    tracing::info!(tx = %tx_hash.pack(), "drop finalize tx");
                }
                Ok(Some(tx_status)) => {
                    match tx_status {
                        TxStatus::Pending | TxStatus::Proposed => return Ok(()), // Wait
                        TxStatus::Committed => {
                            tracing::debug!(tx = %tx_hash.pack(), "finalize tx is committed");
                        }
                        TxStatus::Unknown | TxStatus::Rejected => {
                            tracing::debug!(tx = %tx_hash.pack(), status = ?tx_status, "drop finalize tx");
                        }
                        _ => {
                            tracing::warn!(tx = %tx_hash.pack(), status = ?tx_status, "drop finalize tx");
                        }
                    }
                }
            }

            self.set_last_finalize_tx(None);
        }

        if let Some((tx, pending_finalized)) = self.inner.query_and_finalize_to_owner().await? {
            match rpc_client.send_transaction(&tx).await {
                Ok(tx_hash) => {
                    tracing::info!(tx_hash = %tx_hash.pack(), blk_idx = ?(pending_finalized.unpack_block_index()), "finalize withdrawal");

                    // TODO: Optimize and remove tx blocking, block producer wait indexer refresh
                    // rollup cell
                    self.inner.wait(&tx_hash, TxStatus::Committed).await?;
                    {
                        *self.inner.last_block_submitted_tx.write().await = tx_hash;
                    }
                    self.set_last_finalize_tx(Some(tx_hash));
                }
                Err(err) => {
                    let err_string = err.to_string();
                    if err_string.contains(TRANSACTION_FAILED_TO_RESOLVE_ERROR) {
                        tracing::info!("failed to resolve, try again later");
                        return Ok(());
                    }

                    let debug_tx_dump_path = &self.debug_config.debug_tx_dump_path;
                    tracing::info!(dump_path = ?debug_tx_dump_path, "dump finalize tx");

                    crate::utils::dump_transaction(debug_tx_dump_path, rpc_client, &tx).await;
                    bail!("dry run finalize tx failed {}", err);
                }
            };
        }

        Ok(())
    }

    fn set_last_finalize_tx(&self, tx_hash: Option<H256>) {
        *self.last_finalize_tx.lock().expect("lock") = tx_hash;
    }
}

#[async_trait]
pub trait FinalizeWithdrawalToOwner {
    fn rollup_context(&self) -> &RollupContext;

    fn contracts_dep(&self) -> Guard<Arc<ContractsCellDep>>;

    fn rollup_deps(&self) -> Vec<CellDep>;

    fn transaction_skeleton(&self) -> TransactionSkeleton;

    fn generate_block_proof(&self, withdrawals: &[BlockWithdrawals])
        -> Result<CompiledMerkleProof>;

    fn get_withdrawal_extras(
        &self,
        block_withdrawals: &[BlockWithdrawals],
    ) -> Result<HashMap<H256, WithdrawalRequestExtra>>;

    fn get_sudt_scripts(
        &self,
        block_withdrawals: &[BlockWithdrawals],
    ) -> Result<HashMap<H256, Script>>;

    fn get_pending_finalized_withdrawals(
        &self,
        last_finalized_withdrawal: &LastFinalizedWithdrawal,
        last_finalized_block_number: u64,
    ) -> Result<Option<Vec<BlockWithdrawals>>>;

    async fn query_rollup_cell(&self) -> Result<Option<InputCellInfo>>;

    async fn query_finalized_custodians(
        &self,
        last_finalized_block_number: u64,
        withdrawals: &[BlockWithdrawals],
    ) -> Result<CollectedCustodianCells>;

    async fn complete_tx(&self, tx_skeleton: TransactionSkeleton) -> Result<Transaction>;

    async fn query_and_finalize_to_owner(
        &self,
    ) -> Result<Option<(Transaction, LastFinalizedWithdrawal)>> {
        let rollup_input = match self.query_rollup_cell().await? {
            Some(cell) => cell,
            None => {
                tracing::warn!("rollup cell not found");
                return Ok(None);
            }
        };

        let global_state = global_state_from_slice(&rollup_input.cell.data)?;
        if global_state.version_u8() < 2 {
            return Ok(None);
        }

        // Check rollup status is running
        {
            let status_byte: u8 = global_state.status().into();
            let status = match Status::try_from(status_byte) {
                Ok(status) => status,
                Err(err) => {
                    tracing::error!(status = err, "invalid rollup status");
                    return Ok(None);
                }
            };

            if Status::Running != status {
                tracing::debug!(status = ?status, "rollup status isn't running");
                return Ok(None);
            }
        };

        let last_finalized_block_number: u64 = global_state.last_finalized_block_number().unpack();
        let last_finalized_withdrawal = global_state.last_finalized_withdrawal();

        let block_withdrawals = match self.get_pending_finalized_withdrawals(
            &last_finalized_withdrawal,
            last_finalized_block_number,
        )? {
            Some(withdrawals) => withdrawals,
            None => return Ok(None),
        };
        ensure!(!block_withdrawals.is_empty(), "has block withdrawals");
        tracing::debug!(
            finalized_block = last_finalized_block_number,
            pending_to_finalized = block_withdrawals.len()
        );

        let block_proof = self.generate_block_proof(&block_withdrawals)?;
        let extra_map = self.get_withdrawal_extras(&block_withdrawals)?;
        let sudt_script_map = self.get_sudt_scripts(&block_withdrawals)?;

        let to_finalized = crate::withdrawal::finalize(
            &block_withdrawals,
            &block_proof,
            &extra_map,
            &sudt_script_map,
        )?;

        let mut tx_skeleton = self.transaction_skeleton();

        let last_finalized_withdrawal =
            { block_withdrawals.last().expect("last block withdrawals") }
                .to_last_finalized_withdrawal();

        let rollup_output = {
            let post_global_state = { global_state.clone().as_builder() }
                .last_finalized_withdrawal(last_finalized_withdrawal.clone())
                .build();

            (
                rollup_input.cell.output.clone(),
                post_global_state.as_bytes(),
            )
        };

        let rollup_witness = {
            let rollup_action = RollupAction::new_builder()
                .set(RollupActionUnion::RollupFinalizeWithdrawal(
                    to_finalized.witness,
                ))
                .build();

            WitnessArgs::new_builder()
                .output_type(Some(rollup_action.as_bytes()).pack())
                .build()
        };

        tx_skeleton.cell_deps_mut().extend(self.rollup_deps());
        tx_skeleton.inputs_mut().push(rollup_input);
        tx_skeleton.outputs_mut().push(rollup_output);
        tx_skeleton.witnesses_mut().push(rollup_witness);

        if let Some((withdrawals_amount, user_withdrawal_outputs)) = to_finalized.withdrawals {
            ensure!(!withdrawals_amount.is_zero(), "all withdrawals are valid");

            let finalized_custodians = self
                .query_finalized_custodians(last_finalized_block_number, &block_withdrawals)
                .await?;

            let contracts_dep = self.contracts_dep();
            let custodian_lock_dep = contracts_dep.custodian_cell_lock.clone().into();
            tx_skeleton.cell_deps_mut().push(custodian_lock_dep);
            if !finalized_custodians.sudt.is_empty() {
                let sudt_type_dep = contracts_dep.l1_sudt_type.clone().into();
                tx_skeleton.cell_deps_mut().push(sudt_type_dep);
            }

            let custodians = crate::custodian::aggregate_balance(
                self.rollup_context(),
                finalized_custodians,
                withdrawals_amount,
            )?
            .expect("withdrawal amount isn't zero");

            let custodian_witnesses = vec![WitnessArgs::default(); custodians.inputs.len()];

            tx_skeleton.inputs_mut().extend(custodians.inputs);
            tx_skeleton.outputs_mut().extend(user_withdrawal_outputs);
            tx_skeleton.outputs_mut().extend(custodians.outputs);
            tx_skeleton.witnesses_mut().extend(custodian_witnesses);
        }

        let tx = self.complete_tx(tx_skeleton).await?;
        Ok(Some((tx, last_finalized_withdrawal)))
    }
}

struct LastRollupData {
    block: L2Block,
    global_state: GlobalState,
    used_inputs: HashSet<OutPoint>,
    payment_outputs: Vec<CellInfo>,
}

struct DefaultFinalizer {
    store: gw_store::Store,
    rpc_client: RPCClient,
    ckb_genesis_info: CKBGenesisInfo,
    contracts_dep_manager: ContractsCellDepManager,
    wallet: Wallet,
    rollup_config_cell_dep: CellDep,
    pending: PendingFinalizedWithdrawal,
    last_block_submitted_tx: Arc<tokio::sync::RwLock<H256>>,
    last_rollup_data: Arc<Mutex<Option<LastRollupData>>>,
}

impl DefaultFinalizer {
    fn new(args: FinalizerArgs) -> Self {
        let FinalizerArgs {
            store,
            rpc_client,
            ckb_genesis_info,
            contracts_dep_manager,
            wallet,
            rollup_config_cell_dep,
            last_block_submitted_tx,
        } = args;

        let max_block = MAX_FINALIZE_BLOCKS;
        let max_withdrawals = MAX_FINALIZE_WITHDRAWALS;
        tracing::info!(
            limit = ?(max_block, max_withdrawals),
            "create pending finalize withdrawal queue"
        );

        let pending = PendingFinalizedWithdrawal::new(max_block, max_withdrawals);
        let last_rollup_data = Arc::new(Mutex::new(None));

        Self {
            store,
            rpc_client,
            ckb_genesis_info,
            contracts_dep_manager,
            wallet,
            rollup_config_cell_dep,
            pending,
            last_block_submitted_tx,
            last_rollup_data,
        }
    }

    async fn wait(&self, tx_hash: &H256, until: TxStatus) -> Result<()> {
        use TxStatus::*;

        #[derive(Debug, Clone, Copy)]
        #[repr(u8)]
        enum StatusFlow {
            Unknown = 0,
            Pending = 1,
            Proposed = 2,
            Committed = 3,
        }

        impl StatusFlow {
            fn from_tx_status(status: TxStatus) -> Result<StatusFlow, TxStatus> {
                let flow = match status {
                    Unknown => Self::Unknown,
                    Pending => Self::Pending,
                    Proposed => Self::Proposed,
                    Committed => Self::Committed,
                    _ => return Err(status),
                };
                Ok(flow)
            }
        }

        let until = StatusFlow::from_tx_status(until);
        debug_assert!(until.is_ok());
        let until = until.unwrap();

        let rpc_client = &self.rpc_client;
        loop {
            match rpc_client.ckb.get_transaction_status(*tx_hash).await? {
                Some(status) => {
                    tracing::trace!(tx=%tx_hash.pack(), "wait tx, current {:?} until {:?}", status, until);

                    let current = match StatusFlow::from_tx_status(status.clone()) {
                        Ok(status) => status,
                        Err(err_status) => bail!("finalize withdrawal tx status {:?}", err_status),
                    };

                    if (current as u8) < (until as u8) {
                        tokio::time::sleep(LOOP_TX_STATUS_INTERVAL).await;
                        continue;
                    }
                    return Ok(());
                }
                None => bail!("finalize withdrawal tx {:x} not found", tx_hash.pack()),
            }
        }
    }
}

#[async_trait]
impl FinalizeWithdrawalToOwner for DefaultFinalizer {
    fn rollup_context(&self) -> &RollupContext {
        &self.rpc_client.rollup_context
    }

    fn contracts_dep(&self) -> Guard<Arc<ContractsCellDep>> {
        self.contracts_dep_manager.load()
    }

    fn rollup_deps(&self) -> Vec<CellDep> {
        vec![
            self.contracts_dep().rollup_cell_type.clone().into(),
            self.contracts_dep().omni_lock.clone().into(),
            self.rollup_config_cell_dep.clone(),
        ]
    }

    fn transaction_skeleton(&self) -> TransactionSkeleton {
        let omni_lock_code_hash = self.contracts_dep_manager.load_scripts().omni_lock.hash();
        let mut tx_skeleton = TransactionSkeleton::new(omni_lock_code_hash.0);
        if let Some(data) = self.last_rollup_data.lock().expect("lock").as_ref() {
            { tx_skeleton.excluded_out_points_mut() }.extend(data.used_inputs.clone());
            { tx_skeleton.live_cells_mut() }.extend(data.payment_outputs.clone());
        }
        tx_skeleton
    }

    fn generate_block_proof(
        &self,
        block_withdrawals: &[BlockWithdrawals],
    ) -> Result<CompiledMerkleProof> {
        let tx_db = self.store.begin_transaction();
        let mut block_smt = tx_db.block_smt()?;

        let block_root = {
            let (block, global_state) = {
                let data = self.last_rollup_data.lock().expect("lock");
                data.as_ref()
                    .map(|d| (d.block.clone(), d.global_state.clone()))
                    .ok_or_else(|| anyhow::anyhow!("last rollup data not found"))?
            };
            let expected_root: H256 = global_state.block().merkle_root().unpack();

            block_smt.update(block.smt_key().into(), block.hash().into())?;
            let root = block_smt.root();

            if root != &expected_root {
                bail!("block {} diff db smt and global state", block.number());
            }
            root
        };

        let blocks = block_withdrawals.iter().map(|bw| bw.block());
        let proof = generate_block_proof(&block_smt, blocks)?;

        // Ensure valid block proof
        let leaves = { block_withdrawals.iter() }
            .map(|bw| (bw.block().smt_key().into(), bw.block().hash().into()))
            .collect();
        proof.verify::<Blake2bHasher>(block_root, leaves)?;

        Ok(proof)
    }

    fn get_withdrawal_extras(
        &self,
        block_withdrawals: &[BlockWithdrawals],
    ) -> Result<HashMap<H256, WithdrawalRequestExtra>> {
        let mut extra_map = HashMap::new();

        for withdrawal in block_withdrawals.iter().flat_map(|bw| bw.withdrawals()) {
            let hash: H256 = withdrawal.hash().into();

            match self.store.get_withdrawal(&hash)? {
                Some(extra) => extra_map.insert(hash, extra),
                None => bail!("withdrawal extra {:x} not found", hash.pack()),
            };
        }

        Ok(extra_map)
    }

    fn get_sudt_scripts(
        &self,
        block_withdrawals: &[BlockWithdrawals],
    ) -> Result<HashMap<H256, Script>> {
        let mut sudt_scripts = HashMap::new();

        let sudt_script_hashes: HashSet<_> = { block_withdrawals.iter() }
            .flat_map(|bw| bw.withdrawals())
            .filter_map(|w| {
                let script_hash: [u8; 32] = w.raw().sudt_script_hash().unpack();
                if script_hash != CKB_SUDT_SCRIPT_ARGS {
                    Some(H256::from(script_hash))
                } else {
                    None
                }
            })
            .collect();

        for script_hash in sudt_script_hashes {
            match self.store.get_asset_script(&script_hash)? {
                Some(script) => sudt_scripts.insert(script_hash, script),
                None => bail!("sudt script {:x} not found", script_hash.pack()),
            };
        }

        Ok(sudt_scripts)
    }

    fn get_pending_finalized_withdrawals(
        &self,
        last_finalized_withdrawal: &LastFinalizedWithdrawal,
        last_finalized_block_number: u64,
    ) -> Result<Option<Vec<BlockWithdrawals>>> {
        get_pending_finalized_withdrawals(
            &self.store,
            &self.pending,
            last_finalized_withdrawal,
            last_finalized_block_number,
        )
    }

    async fn query_rollup_cell(&self) -> Result<Option<InputCellInfo>> {
        use gw_types::bytes::Bytes;
        use TxStatus::*;

        let rpc_client = &self.rpc_client.ckb;
        let last_block_tx = { *self.last_block_submitted_tx.read().await };
        tracing::debug!(tx_hash = %last_block_tx.pack(), "get rollup from last submitted tx hash");

        if let Err(err_status) = self.wait(&last_block_tx, Pending).await {
            tracing::debug!(status=?err_status, "got last submitted tx status");
            return Ok(None);
        }

        let tx = match rpc_client.get_transaction(last_block_tx).await? {
            Some(tx) => tx,
            None => {
                tracing::debug!(tx_hash = %last_block_tx.pack(), "last rollup submitted tx not found");
                return Ok(None);
            }
        };

        // find rollup state cell from outputs
        let rollup_type_hash: [u8; 32] = self.rpc_client.rollup_context.rollup_script_hash.into();
        let outputs = tx.raw().outputs().into_iter();
        let find_rollup_output = outputs.enumerate().find(|(_i, output)| {
            output.type_().to_opt().map(|type_| type_.hash()) == Some(rollup_type_hash)
        });
        let (idx, output) = match find_rollup_output {
            Some((idx, output)) => (idx, output),
            None => {
                tracing::debug!(tx_hash = %last_block_tx.pack(), "rollup output not found");
                return Ok(None);
            }
        };

        // Check no input custodians
        // Merge custodians may invalid our finalized custodian queried from indexer.
        let custodian_cell_dep: CellDep = self.contracts_dep().custodian_cell_lock.clone().into();
        let custodian_cell_dep_out_point = custodian_cell_dep.out_point();
        for cell_dep in tx.raw().cell_deps().into_iter() {
            if cell_dep.out_point().as_slice() == custodian_cell_dep_out_point.as_slice() {
                tracing::debug!(tx_hash = %last_block_tx.pack(), "merge custodians, skip");
                return Ok(None);
            }
        }

        let data: gw_types::bytes::Bytes = match tx.raw().outputs_data().get(idx) {
            Some(data) => data.unpack(),
            None => {
                tracing::debug!(tx_hash = %last_block_tx.pack(), idx = idx, "rollup data not found");
                return Ok(None);
            }
        };

        // extra block and global state
        let block = {
            let witness: Option<Bytes> = tx.witnesses().get(idx).map(|w| w.unpack());
            let witness_args = witness.and_then(|w| WitnessArgs::from_slice(&w).ok());
            let output_type = witness_args.and_then(|w| w.output_type().to_opt());
            let rollup_action =
                output_type.and_then(|ot| RollupAction::from_slice(&ot.raw_data()).ok());

            match rollup_action.map(|action| action.to_enum()) {
                Some(RollupActionUnion::RollupSubmitBlock(submitted)) => submitted.block(),
                _ => return Ok(None),
            }
        };
        let global_state = match GlobalState::from_slice(&data) {
            Ok(state) => state,
            Err(_) => return Ok(None),
        };
        let used_inputs = { tx.raw().inputs().into_iter() }
            .map(|i| i.previous_output())
            .collect();
        let payment_outputs = {
            let wallet_lock_hash = self.wallet.lock_script().hash();
            let payment_outputs =
                { tx.raw().outputs().into_iter().enumerate() }.filter_map(|(i, output)| {
                    if output.lock().hash() != wallet_lock_hash {
                        return None;
                    }

                    let data: Bytes = tx.raw().outputs_data().get(i)?.unpack();

                    let out_point = OutPoint::new_builder()
                        .tx_hash(last_block_tx.pack())
                        .index((i as u32).pack())
                        .build();

                    let cell = CellInfo {
                        out_point,
                        output,
                        data,
                    };

                    Some(cell)
                });
            payment_outputs.collect()
        };

        {
            *self.last_rollup_data.lock().expect("lock") = Some(LastRollupData {
                block,
                global_state,
                used_inputs,
                payment_outputs,
            });
        }

        let since = {
            let input = tx.raw().inputs().get(idx);
            input.map(|i| i.since().unpack()).unwrap_or_default()
        };

        let out_point = OutPoint::new_builder()
            .tx_hash(last_block_tx.pack())
            .index((idx as u32).pack())
            .build();

        let input = CellInput::new_builder()
            .previous_output(out_point.clone())
            .since(since.pack())
            .build();

        let cell = CellInfo {
            out_point,
            output,
            data,
        };

        Ok(Some(InputCellInfo { input, cell }))
    }

    async fn query_finalized_custodians(
        &self,
        last_finalized_block_number: u64,
        block_withdrawals: &[BlockWithdrawals],
    ) -> Result<CollectedCustodianCells> {
        query_finalized_custodians(
            &self.rpc_client,
            &self.store.begin_transaction(),
            { block_withdrawals.iter() }.flat_map(BlockWithdrawals::withdrawals),
            self.rollup_context(),
            last_finalized_block_number,
        )
        .await?
        .expect_full("finalized custodian not enough")
    }

    async fn complete_tx(&self, mut tx_skeleton: TransactionSkeleton) -> Result<Transaction> {
        let owner_lock_dep = self.ckb_genesis_info.sighash_dep();
        tx_skeleton.cell_deps_mut().push(owner_lock_dep);

        let owner_lock = self.wallet.lock_script().to_owned();
        fill_tx_fee(&mut tx_skeleton, &self.rpc_client.indexer, owner_lock).await?;
        self.wallet.sign_tx_skeleton(tx_skeleton)
    }
}

#[instrument(skip_all)]
fn get_pending_finalized_withdrawals(
    store: &impl ChainStore,
    pending: &PendingFinalizedWithdrawal,
    last_finalized_withdrawal: &LastFinalizedWithdrawal,
    last_finalized_block_number: u64,
) -> Result<Option<Vec<BlockWithdrawals>>> {
    let (last_wthdr_bn, last_wthdr_idx) = last_finalized_withdrawal.unpack_block_index();
    tracing::debug!(finalized_withdrawal = ?(last_wthdr_bn, last_wthdr_idx), "get pending finalized");

    let ensure_get_block = |num: u64| -> Result<L2Block> {
        match store.get_block_by_number(num)? {
            Some(b) => Ok(b),
            None => bail!("block {} not found", num),
        }
    };

    let next_pending_blk_wthdrs_on_chain = match BlockWithdrawals::from_rest(
        ensure_get_block(last_wthdr_bn)?,
        last_finalized_withdrawal,
    )? {
        Some(blk_wthdrs) => blk_wthdrs,
        None => match store.get_block_by_number(last_wthdr_bn + 1)? {
            Some(blk) => BlockWithdrawals::new(blk),
            None => {
                tracing::debug!(blk = last_wthdr_bn + 1, "pending finalized block not found");
                return Ok(None);
            }
        },
    };
    let next_pending_blk_num_on_chain = next_pending_blk_wthdrs_on_chain.block_number();
    if next_pending_blk_num_on_chain > last_finalized_block_number {
        return Ok(None);
    }

    loop {
        let next_pending_blk_wthdrs = match pending.block_range() {
            Some(range) if *range.start() != next_pending_blk_num_on_chain => {
                // Maybe L1 reorg, reset pending queue state
                pending.reset();
                return Ok(None);
            }
            Some(range) if *range.end() > last_finalized_block_number => {
                // Maybe L1 reorg, reset pending queue state
                pending.reset();
                return Ok(None);
            }
            Some(range) if *range.end() == last_finalized_block_number => {
                return Ok(None);
            }
            Some(range) => {
                // Push next available block
                let next_blk_num = *range.end() + 1;
                match store.get_block_by_number(next_blk_num)? {
                    Some(blk) => BlockWithdrawals::new(blk),
                    None => {
                        tracing::debug!(blk = next_blk_num, "pending finalized block not found");
                        return Ok(None);
                    }
                }
            }
            None => next_pending_blk_wthdrs_on_chain.clone(),
        };

        let next_pending_blk_range = next_pending_blk_wthdrs.block_num_wthdrs_range();
        match pending.push(next_pending_blk_wthdrs) {
            Ok(limit_reached) if limit_reached => return Ok(Some(pending.take())),
            Ok(_unfulfilled) => continue,
            Err(err) => {
                tracing::warn!(
                    blk_range = ?next_pending_blk_range,
                    error = ?err,
                    "push pending block withdrawals"
                );

                // try again later
                pending.reset();
                return Ok(None);
            }
        }
    }
}

#[derive(Debug)]
struct PendingFinalizedWithdrawal {
    inner: Mutex<Vec<BlockWithdrawals>>,
    max_block: u32,
    max_withdrawals: u32,
}

impl PendingFinalizedWithdrawal {
    fn new(max_block: u32, max_withdrawals: u32) -> Self {
        Self {
            inner: Mutex::new(Vec::with_capacity(max_block as usize)),
            max_block,
            max_withdrawals,
        }
    }

    fn block_range(&self) -> Option<RangeInclusive<u64>> {
        let inner = self.inner.lock().expect("lock");
        match (
            inner.first().map(|bw| bw.block_number()),
            inner.last().map(|bw| bw.block_number()),
        ) {
            (Some(start), Some(end)) => Some(start..=end),
            _ => None,
        }
    }

    // this func return `true` if it reaches either max_block or max_withdrawals limit
    fn push(&self, block_withdrawals: BlockWithdrawals) -> Result<bool> {
        let mut inner = self.inner.lock().expect("lock success");

        let mut block_left = self.max_block.saturating_sub(inner.len() as u32);
        if 0 == block_left {
            return Ok(true);
        }

        let withdrawals_count = inner.iter().map(|bw| bw.len()).sum();
        let mut wthdr_left = self.max_withdrawals.saturating_sub(withdrawals_count);
        if 0 == wthdr_left {
            return Ok(true);
        }

        let blk_hash = block_withdrawals.block().hash();
        let parent_blk_hash: [u8; 32] =
            block_withdrawals.block().raw().parent_block_hash().unpack();
        match inner.last().map(|bw| bw.block().hash()) {
            Some(last_blk_hash) if last_blk_hash == blk_hash => return Ok(false),
            Some(last_blk_hash) if last_blk_hash != parent_blk_hash => {
                bail!("block withdrawals no seq")
            }
            Some(_) | None => block_left -= 1,
        }

        if block_withdrawals.len() >= wthdr_left {
            let shrinked = block_withdrawals
                .take(wthdr_left)
                .expect("shrinked block withdrawals");

            wthdr_left = 0;
            inner.push(shrinked);
        } else {
            wthdr_left -= block_withdrawals.len();
            inner.push(block_withdrawals);
        }

        Ok(0 == block_left || 0 == wthdr_left)
    }

    fn take(&self) -> Vec<BlockWithdrawals> {
        let mut inner = self.inner.lock().expect("lock");
        std::mem::replace(&mut inner, Vec::with_capacity(self.max_block as usize))
    }

    fn reset(&self) {
        self.take();
    }
}

#[cfg(test)]
mod tests {
    use gw_db::schema::Col;
    use gw_store::traits::kv_store::KVStoreRead;
    use gw_types::packed::GlobalState;

    use super::*;
    use crate::withdrawal::tests::BlockStore;

    impl ChainStore for BlockStore {
        fn get_block_by_number(&self, number: u64) -> Result<Option<L2Block>, gw_db::error::Error> {
            Ok(self.blocks.get(number as usize).cloned())
        }
    }

    impl KVStoreRead for BlockStore {
        fn get(&self, _col: Col, _key: &[u8]) -> Option<Box<[u8]>> {
            unreachable!()
        }
    }

    mockall::mock! {
        DummyFinalizer {}

        #[async_trait]
        impl FinalizeWithdrawalToOwner for DummyFinalizer {
            fn rollup_context(&self) -> &RollupContext;

            fn contracts_dep(&self) -> Guard<Arc<ContractsCellDep>>;

            fn rollup_deps(&self) -> Vec<CellDep>;

            fn transaction_skeleton(&self) -> TransactionSkeleton;

            fn generate_block_proof(&self, withdrawals: &[BlockWithdrawals])
            -> Result<CompiledMerkleProof>;

            fn get_withdrawal_extras(
                &self,
                block_withdrawals: &[BlockWithdrawals],
            ) -> Result<HashMap<H256, WithdrawalRequestExtra>>;

            fn get_sudt_scripts(
                &self,
                block_withdrawals: &[BlockWithdrawals],
            ) -> Result<HashMap<H256, Script>>;

            fn get_pending_finalized_withdrawals(
                &self,
                last_finalized_withdrawal: &LastFinalizedWithdrawal,
                last_finalized_block_number: u64,
            ) -> Result<Option<Vec<BlockWithdrawals>>>;

            async fn query_rollup_cell(&self) -> Result<Option<InputCellInfo>>;

            async fn query_finalized_custodians(
                &self,
                last_finalized_block_number: u64,
                withdrawals: &[BlockWithdrawals],
            ) -> Result<CollectedCustodianCells>;

            async fn complete_tx(&self, tx_skeleton: TransactionSkeleton) -> Result<Transaction>;
        }
    }

    #[test]
    fn test_get_pending_finalized_withdrawals() {
        let mut store = BlockStore::default();
        let pending = PendingFinalizedWithdrawal::new(4, 5);

        let zero = store.produce_block(0);
        let one = store.produce_block(0);
        let two = store.produce_block(1);
        let three = store.produce_block(2);
        let four = store.produce_block(3);

        let last_finalized = LastFinalizedWithdrawal::pack_block_index(
            zero.number(),
            LastFinalizedWithdrawal::INDEX_ALL_WITHDRAWALS,
        );

        // Next pending block > last_finalized_block_number
        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 0).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), None);

        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 1).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), Some(one.number()..=one.number()));

        // Fetch again without updated last_finalized_block_number
        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 1).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), Some(one.number()..=one.number()));

        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 2).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), Some(one.number()..=two.number()));

        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 3).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), Some(one.number()..=three.number()));

        let fulfilled = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 4)
            .unwrap()
            .unwrap();
        let expected_blk_wthdrs = vec![
            BlockWithdrawals::new(one.clone()),
            BlockWithdrawals::new(two.clone()),
            BlockWithdrawals::new(three.clone()),
            BlockWithdrawals::new(four.clone()).take(2).unwrap(),
        ];
        assert_eq!(fulfilled, expected_blk_wthdrs);
        assert_eq!(pending.block_range(), None);

        // Fetch all in once
        let fulfilled = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 4)
            .unwrap()
            .unwrap();
        assert_eq!(fulfilled, expected_blk_wthdrs);
        assert_eq!(pending.block_range(), None);

        // Max withdrawals
        let last_finalized = LastFinalizedWithdrawal::pack_block_index(
            one.number(),
            LastFinalizedWithdrawal::INDEX_ALL_WITHDRAWALS,
        );
        let fulfilled = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 4)
            .unwrap()
            .unwrap();
        let expected_blk_wthdrs = vec![
            BlockWithdrawals::new(two),
            BlockWithdrawals::new(three),
            BlockWithdrawals::new(four).take(2).unwrap(),
        ];
        assert_eq!(fulfilled, expected_blk_wthdrs);
        assert_eq!(pending.block_range(), None);
    }

    #[test]
    fn test_get_pending_finalized_withdrawals_block_not_found() {
        let mut store = BlockStore::default();
        let pending = PendingFinalizedWithdrawal::new(4, 5);

        let zero = store.produce_block(0);

        // Next pending block on chain not found
        let last_finalized = LastFinalizedWithdrawal::pack_block_index(
            zero.number(),
            LastFinalizedWithdrawal::INDEX_ALL_WITHDRAWALS,
        );

        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 1).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), None);

        // range.end() < last_finalized_block_number, range.end() + 1 not found
        let one = store.produce_block(0);

        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 2).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), Some(one.number()..=one.number()));
    }

    #[test]
    fn test_get_pending_finalized_withdrawals_reset_pending() {
        let mut store = BlockStore::default();
        let pending = PendingFinalizedWithdrawal::new(4, 5);

        let zero = store.produce_block(0);
        let one = store.produce_block(0);
        let two = store.produce_block(1);
        let three = store.produce_block(2);

        let last_finalized = LastFinalizedWithdrawal::pack_block_index(
            one.number(),
            LastFinalizedWithdrawal::INDEX_ALL_WITHDRAWALS,
        );

        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 2).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), Some(two.number()..=two.number()));

        // range.start() != next_pending_blk_num_on_chain
        let reorg_last_finalized = LastFinalizedWithdrawal::pack_block_index(
            zero.number(),
            LastFinalizedWithdrawal::INDEX_ALL_WITHDRAWALS,
        );

        let ret =
            get_pending_finalized_withdrawals(&store, &pending, &reorg_last_finalized, 2).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), None);

        // range.end() > last_finalized_block_number
        let last_finalized = LastFinalizedWithdrawal::pack_block_index(
            one.number(),
            LastFinalizedWithdrawal::INDEX_ALL_WITHDRAWALS,
        );

        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 3).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), Some(two.number()..=three.number()));

        // reduce last_finalized_block_number from 3 to 2
        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 2).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), None);

        // push error
        let last_finalized = LastFinalizedWithdrawal::pack_block_index(
            one.number(),
            LastFinalizedWithdrawal::INDEX_ALL_WITHDRAWALS,
        );

        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 3).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), Some(two.number()..=three.number()));

        // create a invalid four block, which's parent block hash isn't three
        let _four = store.produce_block(2);
        let four_mut = store.blocks.get_mut(4).unwrap();
        let err_raw = { four_mut.raw().as_builder() }
            .parent_block_hash([0u8; 32].pack())
            .build();
        *four_mut = { four_mut.clone() }.as_builder().raw(err_raw).build();

        let ret = get_pending_finalized_withdrawals(&store, &pending, &last_finalized, 4).unwrap();
        assert!(ret.is_none());
        assert_eq!(pending.block_range(), None);
    }

    #[test]
    fn test_pending_finalized_withdrawal() {
        let mut store = BlockStore::default();

        let pending = PendingFinalizedWithdrawal::new(2, 10);
        assert!(pending.block_range().is_none());

        let block = store.produce_block(1);
        let unfulfilled = !pending.push(BlockWithdrawals::new(block.clone())).unwrap();

        assert!(unfulfilled);
        assert_eq!(pending.block_range(), Some(block.number()..=block.number()));

        let other_block = store.produce_block(1);
        let reached = pending
            .push(BlockWithdrawals::new(other_block.clone()))
            .unwrap();

        assert!(reached);
        assert_eq!(
            pending.block_range(),
            Some(block.number()..=other_block.number())
        );

        let blk_wthdrs = pending.take();
        assert!(pending.block_range().is_none());
        assert_eq!(blk_wthdrs.len(), 2);

        let expected_blk_wthdrs = vec![
            BlockWithdrawals::new(block.clone()),
            BlockWithdrawals::new(other_block),
        ];
        assert_eq!(blk_wthdrs, expected_blk_wthdrs);

        pending.push(BlockWithdrawals::new(block)).unwrap();
        pending.reset();
        assert!(pending.block_range().is_none());
    }

    #[test]
    fn test_pending_finalized_withdrawal_max_withdrawals_limit() {
        let mut store = BlockStore::default();

        let pending = PendingFinalizedWithdrawal::new(5, 1);
        assert!(pending.block_range().is_none());

        let block = store.produce_block(1);
        let reache_limit = pending.push(BlockWithdrawals::new(block.clone())).unwrap();

        assert!(reache_limit);
        assert_eq!(pending.block_range(), Some(block.number()..=block.number()));

        let blk_wthdrs = pending.take();
        assert!(pending.block_range().is_none());
        assert_eq!(blk_wthdrs.len(), 1);

        let expected_blk_wthdrs = vec![BlockWithdrawals::new(block)];
        assert_eq!(blk_wthdrs, expected_blk_wthdrs);
    }

    #[test]
    fn test_pending_finalized_withdrawal_push_after_reach_limit() {
        let mut store = BlockStore::default();

        let pending = PendingFinalizedWithdrawal::new(5, 1);
        assert!(pending.block_range().is_none());

        let block = store.produce_block(1);
        let reache_limit = pending.push(BlockWithdrawals::new(block.clone())).unwrap();

        assert!(reache_limit);
        assert_eq!(pending.block_range(), Some(block.number()..=block.number()));

        let other_block = store.produce_block(0);
        let reache_limit = pending.push(BlockWithdrawals::new(other_block)).unwrap();

        // Block range should not be changed
        assert!(reache_limit);
        assert_eq!(pending.block_range(), Some(block.number()..=block.number()));

        let blk_wthdrs = pending.take();
        assert!(pending.block_range().is_none());
        assert_eq!(blk_wthdrs.len(), 1);

        let expected_blk_wthdrs = vec![BlockWithdrawals::new(block)];
        assert_eq!(blk_wthdrs, expected_blk_wthdrs);
    }

    #[test]
    fn test_pending_finalized_withdrawal_shrink_withdrawals_to_fit_max_withdrawals_limit() {
        let mut store = BlockStore::default();

        let pending = PendingFinalizedWithdrawal::new(5, 10);
        assert!(pending.block_range().is_none());

        let block = store.produce_block(3);
        let unfulfilled = !pending.push(BlockWithdrawals::new(block.clone())).unwrap();

        assert!(unfulfilled);
        assert_eq!(pending.block_range(), Some(block.number()..=block.number()));

        let other_block = store.produce_block(10);
        let reach_limit = pending
            .push(BlockWithdrawals::new(other_block.clone()))
            .unwrap();

        assert!(reach_limit);
        assert_eq!(
            pending.block_range(),
            Some(block.number()..=other_block.number())
        );

        let blk_wthdrs = pending.take();
        assert!(pending.block_range().is_none());
        assert_eq!(blk_wthdrs.len(), 2);

        let expected_blk_wthdrs = vec![
            BlockWithdrawals::new(block),
            BlockWithdrawals::new(other_block).take(7).unwrap(),
        ];
        assert_eq!(blk_wthdrs, expected_blk_wthdrs);
    }

    #[test]
    fn test_pending_finalized_withdrawal_push_same_block() {
        let mut store = BlockStore::default();

        let pending = PendingFinalizedWithdrawal::new(5, 10);
        assert!(pending.block_range().is_none());

        let block = store.produce_block(0);
        let unfulfilled = !pending.push(BlockWithdrawals::new(block.clone())).unwrap();

        assert!(unfulfilled);
        assert_eq!(pending.block_range(), Some(block.number()..=block.number()));

        // Push same block again
        let unfulfilled = !pending.push(BlockWithdrawals::new(block.clone())).unwrap();
        assert!(unfulfilled);
        assert_eq!(pending.block_range(), Some(block.number()..=block.number()));

        let blk_wthdrs = pending.take();
        assert!(pending.block_range().is_none());
        assert_eq!(blk_wthdrs.len(), 1);

        let expected_blk_wthdrs = vec![BlockWithdrawals::new(block)];
        assert_eq!(blk_wthdrs, expected_blk_wthdrs);
    }

    #[test]
    fn test_pending_finalized_withdrawal_invalid_push_block_no_seq() {
        let mut store = BlockStore::default();

        let pending = PendingFinalizedWithdrawal::new(5, 10);
        assert!(pending.block_range().is_none());

        let one = store.produce_block(0);
        let _two = store.produce_block(0);
        let three = store.produce_block(0);

        pending.push(BlockWithdrawals::new(one)).unwrap();
        let err = pending.push(BlockWithdrawals::new(three)).unwrap_err();
        eprintln!("err {}", err);

        assert!(err.to_string().contains("block withdrawals no seq"));
    }

    #[tokio::test]
    async fn test_query_and_finalize_to_owner_rollup_cell_not_found() {
        let mut finalizer = MockDummyFinalizer::new();
        finalizer.expect_query_rollup_cell().returning(|| Ok(None));

        assert!({ finalizer.query_and_finalize_to_owner().await.unwrap() }.is_none());
    }

    #[tokio::test]
    async fn test_query_and_finalize_to_owner_invalid_global_state() {
        let mut finalizer = MockDummyFinalizer::new();

        // Version isn't 2
        let global_state = GlobalState::new_builder().version(1u8.into()).build();
        finalizer.expect_query_rollup_cell().returning(move || {
            let cell_info = CellInfo {
                data: global_state.as_bytes(),
                ..Default::default()
            };
            let input_cell = InputCellInfo {
                input: CellInput::default(),
                cell: cell_info,
            };
            Ok(Some(input_cell))
        });

        assert!({ finalizer.query_and_finalize_to_owner().await.unwrap() }.is_none());

        // Rollup status isn't running
        let global_state = GlobalState::new_builder()
            .status(1u8.into())
            .version(2u8.into())
            .build();
        finalizer.expect_query_rollup_cell().returning(move || {
            let cell_info = CellInfo {
                data: global_state.as_bytes(),
                ..Default::default()
            };
            let input_cell = InputCellInfo {
                input: CellInput::default(),
                cell: cell_info,
            };
            Ok(Some(input_cell))
        });

        assert!({ finalizer.query_and_finalize_to_owner().await.unwrap() }.is_none());

        // Invalid rollup status
        let global_state = GlobalState::new_builder()
            .status(2u8.into())
            .version(2u8.into())
            .build();
        finalizer.expect_query_rollup_cell().returning(move || {
            let cell_info = CellInfo {
                data: global_state.as_bytes(),
                ..Default::default()
            };
            let input_cell = InputCellInfo {
                input: CellInput::default(),
                cell: cell_info,
            };
            Ok(Some(input_cell))
        });

        assert!({ finalizer.query_and_finalize_to_owner().await.unwrap() }.is_none());

        // No pending finalized withdrawals
        let global_state = GlobalState::new_builder()
            .status(0u8.into())
            .version(2u8.into())
            .build();
        finalizer.expect_query_rollup_cell().returning(move || {
            let cell_info = CellInfo {
                data: global_state.as_bytes(),
                ..Default::default()
            };
            let input_cell = InputCellInfo {
                input: CellInput::default(),
                cell: cell_info,
            };
            Ok(Some(input_cell))
        });
        finalizer
            .expect_get_pending_finalized_withdrawals()
            .returning(|_, _| Ok(None));

        assert!({ finalizer.query_and_finalize_to_owner().await.unwrap() }.is_none());
    }
}
