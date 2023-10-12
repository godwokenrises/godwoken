use crate::types::ChainEvent;
use gw_utils::genesis_info::CKBGenesisInfo;
use gw_utils::transaction_skeleton::TransactionSkeleton;
use gw_utils::{fee::fill_tx_fee, wallet::Wallet};

use anyhow::{anyhow, Result};
use ckb_types::prelude::{Builder, Entity};
use gw_challenge::cancel_challenge::RecoverAccountsContext;
use gw_common::H256;
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::core::Status;
use gw_types::offchain::{global_state_from_slice, CellInfo, InputCellInfo};
use gw_types::packed::{CellDep, CellInput, Transaction, WitnessArgs};
use gw_types::prelude::Unpack;

use std::collections::HashSet;
use std::convert::TryFrom;
use std::sync::Arc;
use tokio::sync::Mutex;

const L1_FINALITY_BLOCKS: u64 = 100;

#[derive(Clone)]
pub struct Verifier {
    load_data_inputs: Vec<InputCellInfo>,
    recover_accounts_context: Option<RecoverAccountsContext>,
    cell_dep: CellDep,
    input: InputCellInfo,
    witness: Option<WitnessArgs>,
}

impl Verifier {
    pub fn new(
        load_data_inputs: Vec<InputCellInfo>,
        recover_accounts_context: Option<RecoverAccountsContext>,
        cell_dep: CellDep,
        input: InputCellInfo,
        witness: Option<WitnessArgs>,
    ) -> Self {
        Verifier {
            load_data_inputs,
            recover_accounts_context,
            cell_dep,
            input,
            witness,
        }
    }

    pub fn tx_hash(&self) -> H256 {
        self.input.input.previous_output().tx_hash().unpack()
    }
}

type ConsumedVerifiers = Arc<Mutex<Vec<(Verifier, Option<H256>)>>>;

// TODO: verifier persistent, signature verifier needs witness to unlock, but verifier itself
// doesn't provides context to restore this witness.
pub struct Cleaner {
    rpc_client: RPCClient,
    ckb_genesis_info: CKBGenesisInfo,
    wallet: Wallet,
    consumed_verifiers: ConsumedVerifiers,
}

impl Cleaner {
    pub fn new(rpc_client: RPCClient, ckb_genesis_info: CKBGenesisInfo, wallet: Wallet) -> Self {
        Cleaner {
            rpc_client,
            ckb_genesis_info,
            wallet,
            consumed_verifiers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn handle_event(&self, _event: ChainEvent) -> Result<()> {
        if matches!(self.query_rollup_status().await?, Status::Halting) {
            return Ok(());
        }

        self.reclaim_uncomsumed_verifiers().await?;
        self.prune().await?;

        Ok(())
    }

    pub async fn watch_verifier(&self, verifier: Verifier, consumed_tx: Option<H256>) {
        self.consumed_verifiers
            .lock()
            .await
            .push((verifier, consumed_tx));
    }

    pub async fn prune(&self) -> Result<()> {
        let consumed_txs: Vec<H256> = {
            let verifiers = self.consumed_verifiers.lock().await;
            verifiers
                .iter()
                .filter_map(|(_, consumed_tx_hash)| *consumed_tx_hash)
                .collect()
        };

        let mut confirmed = HashSet::new();
        let rpc_client = &self.rpc_client;
        let tip_l1_block_number = rpc_client.get_tip().await?.number().unpack();
        for tx_hash in consumed_txs {
            let tx_status = rpc_client.get_transaction_status(tx_hash).await?;
            if !matches!(
                tx_status,
                Some(gw_jsonrpc_types::ckb_jsonrpc_types::Status::Committed)
            ) {
                continue;
            }

            if let Some(block_nubmer) = rpc_client.get_transaction_block_number(tx_hash).await? {
                if block_nubmer < tip_l1_block_number.saturating_sub(L1_FINALITY_BLOCKS) {
                    confirmed.insert(tx_hash);
                }
            }
        }

        {
            self.consumed_verifiers
                .lock()
                .await
                .retain(|(_, consumed_tx_hash)| match consumed_tx_hash {
                    None => true,
                    Some(consumed_tx_hash) => !confirmed.contains(consumed_tx_hash),
                });
        }

        Ok(())
    }

    async fn reclaim_uncomsumed_verifiers(&self) -> Result<()> {
        let consumed_txs: Vec<(usize, Option<H256>)> = {
            let verifiers = self.consumed_verifiers.lock().await;
            let to_iter = verifiers.iter().enumerate();
            to_iter
                .map(|(idx, (_, consumed_tx_hash))| (idx, consumed_tx_hash.to_owned()))
                .collect()
        };

        let rpc_client = &self.rpc_client;
        for (idx, tx_hash) in consumed_txs {
            let consumed = match tx_hash {
                Some(tx_hash) => !matches!(rpc_client.get_transaction_status(tx_hash).await?, None),
                None => false,
            };
            if consumed {
                continue;
            }

            let verifier = {
                let verifiers = self.consumed_verifiers.lock().await;
                verifiers.get(idx).expect("exists").to_owned().0
            };
            let verifier_status = rpc_client
                .get_transaction_status(verifier.tx_hash())
                .await?;
            if !matches!(
                verifier_status,
                Some(gw_jsonrpc_types::ckb_jsonrpc_types::Status::Committed)
            ) {
                continue;
            }

            let verifier_tx = verifier.tx_hash();
            let tx = self.build_reclaim_verifier_tx(verifier).await?;
            let tx_hash = rpc_client.send_transaction(&tx).await?;

            {
                let mut verifiers = self.consumed_verifiers.lock().await;
                verifiers.get_mut(idx).expect("exists").1 = Some(tx_hash);
            }

            log::info!(
                "reclaim verifier {} in tx {}",
                hex::encode(verifier_tx.as_slice()),
                hex::encode(tx_hash.as_slice())
            );
        }

        Ok(())
    }

    async fn query_rollup_status(&self) -> Result<Status> {
        let query_cell = self.rpc_client.query_rollup_cell().await?;
        let rollup_cell = query_cell.ok_or_else(|| anyhow!("rollup cell not found"))?;
        let global_state = global_state_from_slice(&rollup_cell.data)?;

        let status: u8 = global_state.status().into();
        Status::try_from(status).map_err(|n| anyhow!("invalid status {}", n))
    }

    async fn build_reclaim_verifier_tx(&self, verifier: Verifier) -> Result<Transaction> {
        let mut tx_skeleton = TransactionSkeleton::default();

        if let Some(recover_accounts_context) = verifier.recover_accounts_context {
            let RecoverAccountsContext {
                cell_deps,
                inputs,
                witnesses,
            } = recover_accounts_context;

            tx_skeleton.cell_deps_mut().extend(cell_deps);
            tx_skeleton.inputs_mut().extend(inputs);
            tx_skeleton.witnesses_mut().extend(witnesses);
        }

        tx_skeleton.cell_deps_mut().push(verifier.cell_dep);
        tx_skeleton.inputs_mut().push(verifier.input);
        tx_skeleton.inputs_mut().extend(verifier.load_data_inputs);
        if let Some(verifier_witness) = verifier.witness {
            tx_skeleton.witnesses_mut().push(verifier_witness);
        }

        // Verifier cell need an owner cell to unlock
        let owner_lock = self.wallet.lock_script().to_owned();
        let rpc_client = &self.rpc_client;
        let owner_input = {
            let query = rpc_client.query_owner_cell(owner_lock, None).await?;
            query.ok_or_else(|| anyhow!("owner cell not found for reclaim verifier"))?
        };

        let owner_lock_dep = self.ckb_genesis_info.sighash_dep();
        tx_skeleton.cell_deps_mut().push(owner_lock_dep);
        tx_skeleton
            .inputs_mut()
            .push(to_input_cell_info(owner_input));

        let owner_lock = self.wallet.lock_script().to_owned();
        fill_tx_fee(&mut tx_skeleton, &self.rpc_client.indexer, owner_lock).await?;
        self.wallet.sign_tx_skeleton(tx_skeleton)
    }
}

fn to_input_cell_info(cell_info: CellInfo) -> InputCellInfo {
    InputCellInfo {
        input: CellInput::new_builder()
            .previous_output(cell_info.out_point.clone())
            .build(),
        cell: cell_info,
    }
}
