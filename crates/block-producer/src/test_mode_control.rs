use anyhow::{anyhow, Result};
use async_trait::async_trait;
use ckb_types::prelude::{Builder, Entity};
use gw_common::h256_ext::H256Ext;
use gw_common::merkle_utils::{calculate_ckb_merkle_root, ckb_merkle_leaf_hash};
use gw_common::smt::Blake2bHasher;
use gw_common::H256;
use gw_generator::ChallengeContext;
use gw_jsonrpc_types::test_mode::ChallengeType;
use gw_jsonrpc_types::{
    godwoken::GlobalState as JsonGlobalState,
    test_mode::{ShouldProduceBlock, TestModePayload},
};
use gw_poa::{PoA, ShouldIssueBlock};
use gw_rpc_client::rpc_client::RPCClient;
use gw_rpc_server::registry::TestModeRPC;
use gw_store::traits::chain_store::ChainStore;
use gw_store::Store;
use gw_types::core::{ChallengeTargetType, Status};
use gw_types::offchain::{global_state_from_slice, InputCellInfo};
use gw_types::packed::{
    BlockMerkleState, ChallengeTarget, ChallengeWitness, GlobalState, L2Block, L2Transaction,
    SubmitWithdrawals, WithdrawalRequest,
};
use gw_types::prelude::{Pack, PackVec};
use gw_types::{bytes::Bytes, packed::CellInput, prelude::Unpack};
use tokio::sync::Mutex;

use std::sync::Arc;

#[derive(Clone)]
pub struct TestModeControl {
    payload: Arc<Mutex<Option<TestModePayload>>>,
    rpc_client: RPCClient,
    poa: Arc<Mutex<PoA>>,
    store: Store,
}

impl TestModeControl {
    pub fn new(rpc_client: RPCClient, poa: Arc<Mutex<PoA>>, store: Store) -> Self {
        TestModeControl {
            payload: Arc::new(Mutex::new(None)),
            rpc_client,
            poa,
            store,
        }
    }

    pub async fn payload(&self) -> Option<TestModePayload> {
        self.payload.lock().await.to_owned()
    }

    pub async fn clear_none(&self) -> Result<()> {
        let mut payload = self.payload.lock().await;
        if Some(TestModePayload::None) != *payload {
            return Err(anyhow!("not none payload"));
        }

        payload.take(); // Consume payload
        Ok(())
    }

    pub async fn generate_a_bad_block(
        &self,
        block: L2Block,
        global_state: GlobalState,
    ) -> Result<(L2Block, GlobalState)> {
        let (target_index, target_type) = {
            let mut payload = self.payload.lock().await;

            let (target_index, target_type) = match *payload {
                Some(TestModePayload::BadBlock {
                    target_index,
                    target_type,
                }) => (target_index.value(), target_type),
                _ => return Err(anyhow!("not bad block payload")),
            };

            payload.take(); // Consume payload
            (target_index, target_type)
        };

        let bad_block =
            match target_type {
                ChallengeType::TxExecution => {
                    let tx_count: u32 = block.raw().submit_transactions().tx_count().unpack();
                    if target_index >= tx_count {
                        return Err(anyhow!("target index out of bound, total {}", tx_count));
                    }

                    let tx = block.transactions().get_unchecked(target_index as usize);
                    let bad_tx = {
                        let raw_tx = tx
                            .raw()
                            .as_builder()
                            .nonce(99999999u32.pack())
                            .to_id(99999999u32.pack())
                            .args(Bytes::copy_from_slice("break tx execution".as_bytes()).pack())
                            .build();

                        tx.as_builder().raw(raw_tx).build()
                    };

                    let mut txs: Vec<L2Transaction> = block.transactions().into_iter().collect();
                    *txs.get_mut(target_index as usize).expect("exists") = bad_tx;

                    let tx_witness_root = {
                        let witnesses = txs.iter().enumerate().map(|(id, tx)| {
                            ckb_merkle_leaf_hash(id as u32, &tx.witness_hash().into())
                        });
                        calculate_ckb_merkle_root(witnesses.collect())?
                    };

                    let submit_txs = {
                        let builder = block.raw().submit_transactions().as_builder();
                        builder.tx_witness_root(tx_witness_root.pack()).build()
                    };

                    let raw_block = block
                        .raw()
                        .as_builder()
                        .submit_transactions(submit_txs)
                        .build();

                    block
                        .as_builder()
                        .raw(raw_block)
                        .transactions(txs.pack())
                        .build()
                }
                ChallengeType::TxSignature => {
                    let tx_count: u32 = block.raw().submit_transactions().tx_count().unpack();
                    if target_index >= tx_count {
                        return Err(anyhow!("target index out of bound, total {}", tx_count));
                    }

                    let tx = block.transactions().get_unchecked(target_index as usize);
                    let bad_tx = tx.as_builder().signature(Bytes::default().pack()).build();

                    let mut txs: Vec<L2Transaction> = block.transactions().into_iter().collect();
                    *txs.get_mut(target_index as usize).expect("exists") = bad_tx;

                    let tx_witness_root = {
                        let witnesses = txs.iter().enumerate().map(|(id, tx)| {
                            ckb_merkle_leaf_hash(id as u32, &tx.witness_hash().into())
                        });
                        calculate_ckb_merkle_root(witnesses.collect())?
                    };

                    let submit_txs = {
                        let builder = block.raw().submit_transactions().as_builder();
                        builder.tx_witness_root(tx_witness_root.pack()).build()
                    };

                    let raw_block = block
                        .raw()
                        .as_builder()
                        .submit_transactions(submit_txs)
                        .build();

                    block
                        .as_builder()
                        .raw(raw_block)
                        .transactions(txs.pack())
                        .build()
                }
                ChallengeType::WithdrawalSignature => {
                    let count: u32 = block.raw().submit_withdrawals().withdrawal_count().unpack();
                    if target_index >= count {
                        return Err(anyhow!("target index out of bound, total {}", count));
                    }

                    let withdrawal = block.withdrawals().get_unchecked(target_index as usize);
                    let bad_withdrawal = withdrawal
                        .as_builder()
                        .signature(Bytes::default().pack())
                        .build();

                    let mut withdrawals: Vec<WithdrawalRequest> =
                        block.withdrawals().into_iter().collect();
                    *withdrawals.get_mut(target_index as usize).expect("exists") = bad_withdrawal;

                    let withdrawal_witness_root = {
                        let witnesses = withdrawals.iter().enumerate().map(|(idx, t)| {
                            ckb_merkle_leaf_hash(idx as u32, &t.witness_hash().into())
                        });
                        calculate_ckb_merkle_root(witnesses.collect())?
                    };

                    let submit_withdrawals = SubmitWithdrawals::new_builder()
                        .withdrawal_witness_root(withdrawal_witness_root.pack())
                        .withdrawal_count((withdrawals.len() as u32).pack())
                        .build();

                    let raw_block = block
                        .raw()
                        .as_builder()
                        .submit_withdrawals(submit_withdrawals)
                        .build();

                    block
                        .as_builder()
                        .raw(raw_block)
                        .withdrawals(withdrawals.pack())
                        .build()
                }
            };

        let block_number = bad_block.raw().number().unpack();
        let bad_global_state = {
            let db = self.store.begin_transaction();

            let bad_block_proof = db
                .block_smt()?
                .merkle_proof(vec![H256::from_u64(block_number)])?
                .compile(vec![(H256::from_u64(block_number), H256::zero())])?;

            // Generate new block smt for global state
            let bad_block_smt = {
                let bad_block_root: [u8; 32] = bad_block_proof
                    .compute_root::<Blake2bHasher>(vec![(
                        bad_block.smt_key().into(),
                        bad_block.hash().into(),
                    )])?
                    .into();

                BlockMerkleState::new_builder()
                    .merkle_root(bad_block_root.pack())
                    .count((block_number + 1).pack())
                    .build()
            };

            global_state
                .as_builder()
                .block(bad_block_smt)
                .tip_block_hash(bad_block.hash().pack())
                .build()
        };

        Ok((bad_block, bad_global_state))
    }

    pub async fn challenge(&self) -> Result<ChallengeContext> {
        let (block_number, target_index, target_type) = {
            let mut payload = self.payload.lock().await;

            let (block_number, target_index, target_type) = match *payload {
                Some(TestModePayload::Challenge {
                    block_number,
                    target_index,
                    target_type,
                }) => (block_number.value(), target_index.value(), target_type),
                _ => return Err(anyhow!("not challenge payload")),
            };

            payload.take(); // Consume payload
            (block_number, target_index, target_type)
        };

        let db = self.store.begin_transaction();
        let block_hash = db.get_block_hash_by_number(block_number)?;
        let block =
            db.get_block(&block_hash.ok_or_else(|| anyhow!("block {} not found", block_number))?)?;
        let raw_l2block = block
            .ok_or_else(|| anyhow!("block {} not found", block_number))?
            .raw();

        let block_proof = db
            .block_smt()?
            .merkle_proof(vec![raw_l2block.smt_key().into()])?
            .compile(vec![(
                raw_l2block.smt_key().into(),
                raw_l2block.hash().into(),
            )])?;

        let target_type = match target_type {
            ChallengeType::TxExecution => ChallengeTargetType::TxExecution,
            ChallengeType::TxSignature => ChallengeTargetType::TxSignature,
            ChallengeType::WithdrawalSignature => ChallengeTargetType::Withdrawal,
        };

        let challenge_target = ChallengeTarget::new_builder()
            .block_hash(raw_l2block.hash().pack())
            .target_index(target_index.pack())
            .target_type(target_type.into())
            .build();

        let challenge_witness = ChallengeWitness::new_builder()
            .raw_l2block(raw_l2block)
            .block_proof(block_proof.0.pack())
            .build();

        Ok(ChallengeContext {
            target: challenge_target,
            witness: challenge_witness,
        })
    }

    pub async fn wait_for_challenge_maturity(&self, rollup_status: Status) -> Result<()> {
        let mut payload = self.payload.lock().await;
        if Some(TestModePayload::WaitForChallengeMaturity) != *payload {
            return Err(anyhow!("not wait for challenge maturity payload"));
        }

        // Only consume payload after rollup change back to running
        if Status::Running == rollup_status {
            payload.take();
        }

        Ok(())
    }
}

#[async_trait]
impl TestModeRPC for TestModeControl {
    async fn get_global_state(&self) -> Result<JsonGlobalState> {
        let rollup_cell = {
            let opt = self.rpc_client.query_rollup_cell().await?;
            opt.ok_or_else(|| anyhow!("rollup cell not found"))?
        };

        let global_state = global_state_from_slice(&rollup_cell.data)
            .map_err(|_| anyhow!("parse rollup up global state"))?;

        Ok(global_state.into())
    }

    async fn produce_block(&self, payload: TestModePayload) -> Result<()> {
        log::info!("receive tests produce block payload: {:?}", payload);

        *self.payload.lock().await = Some(payload);

        Ok(())
    }

    async fn should_produce_block(&self) -> Result<ShouldProduceBlock> {
        let rollup_cell = {
            let opt = self.rpc_client.query_rollup_cell().await?;
            opt.ok_or_else(|| anyhow!("rollup cell not found"))?
        };

        let tip_hash: H256 = {
            let l1_tip_hash_number = self.rpc_client.get_tip().await?;
            let tip_hash: [u8; 32] = l1_tip_hash_number.block_hash().unpack();
            tip_hash.into()
        };

        let ret = {
            let median_time = match self.rpc_client.get_block_median_time(tip_hash).await? {
                Some(median_time) => median_time,
                None => return Ok(ShouldProduceBlock::No),
            };
            let poa_cell_input = InputCellInfo {
                input: CellInput::new_builder()
                    .previous_output(rollup_cell.out_point.clone())
                    .build(),
                cell: rollup_cell.clone(),
            };

            let mut poa = self.poa.lock().await;
            poa.should_issue_next_block(median_time, &poa_cell_input)
                .await?
        };

        Ok(match ret {
            ShouldIssueBlock::Yes => ShouldProduceBlock::Yes,
            ShouldIssueBlock::YesIfFull => ShouldProduceBlock::YesIfFull,
            ShouldIssueBlock::No => ShouldProduceBlock::No,
        })
    }
}
