use crate::types::{VerifyContext, VerifyWitness};

use anyhow::{anyhow, bail, Context, Result};
use gw_common::blake2b::new_blake2b;
use gw_common::h256_ext::H256Ext;
use gw_common::merkle_utils::{
    calculate_ckb_merkle_root, calculate_state_checkpoint, ckb_merkle_leaf_hash, CBMT,
};
use gw_common::smt::{Blake2bHasher, SMT};
use gw_common::sparse_merkle_tree::default_store::DefaultStore;
use gw_common::state::{
    build_account_field_key, State, GW_ACCOUNT_NONCE_TYPE, GW_ACCOUNT_SCRIPT_HASH_TYPE,
};
use gw_common::H256;
use gw_generator::traits::StateExt;
use gw_store::state::mem_state_db::MemStateTree;
use gw_store::transaction::StoreTransaction;
use gw_traits::CodeStore;
use gw_types::core::{ChallengeTargetType, Status};
use gw_types::offchain::{RollupContext, RunResult};
use gw_types::packed::{
    AccountMerkleState, BlockMerkleState, Byte32, Bytes, CKBMerkleProof, ChallengeTarget,
    GlobalState, L2Block, L2Transaction, RawL2Block, Script, ScriptReader, ScriptVec,
    SubmitTransactions, SubmitWithdrawals, Uint32, Uint64, VerifyTransactionContext,
    VerifyTransactionSignatureContext, VerifyTransactionSignatureWitness, VerifyTransactionWitness,
    VerifyWithdrawalWitness, WithdrawalRequest,
};
use gw_types::prelude::*;

use std::collections::HashMap;

type MemTree<'a> = MemStateTree<'a>;

#[derive(thiserror::Error, Debug)]
#[error("{:?}", {0})]
pub struct RollBackSavePointError(gw_db::error::Error);

pub struct MockBlockParam {
    rollup_context: RollupContext,
    finality_blocks: u64,
    number: u64,
    rollup_config_hash: Byte32,
    block_producer_id: Uint32,
    parent_block_hash: Byte32,
    stake_cell_owner_lock_hash: Byte32,
    timestamp: Uint64,
    reverted_block_root: Byte32,
    prev_account: AccountMerkleState,
    state_checkpoint_list: Vec<Byte32>,
    transactions: RawBlockTransactions,
    withdrawals: RawBlockWithdrawalRequests,
}

pub struct MockChallengeOutput {
    pub raw_block_size: u64,
    pub global_state: GlobalState,
    pub challenge_target: ChallengeTarget,
    pub verify_context: VerifyContext,
}

impl MockBlockParam {
    pub fn new(
        rollup_context: RollupContext,
        block_producer_id: Uint32,
        parent_block: &L2Block,
        timestamp: u64,
        reverted_block_root: H256,
    ) -> Self {
        MockBlockParam {
            finality_blocks: rollup_context.rollup_config.finality_blocks().unpack(),
            rollup_config_hash: rollup_context.rollup_config.hash().pack(),
            rollup_context,
            block_producer_id,
            number: parent_block.raw().number().unpack() + 1,
            parent_block_hash: parent_block.hash().pack(),
            // NOTE: cancel challenge don't check stake cell owner lock hash, so we can
            // use one from parent block.
            stake_cell_owner_lock_hash: parent_block.raw().stake_cell_owner_lock_hash(),
            timestamp: timestamp.pack(),
            reverted_block_root: reverted_block_root.pack(),
            prev_account: parent_block.raw().post_account(),
            state_checkpoint_list: Vec::new(),
            transactions: RawBlockTransactions::new(),
            withdrawals: RawBlockWithdrawalRequests::new(),
        }
    }

    pub fn reset(&mut self, parent_block: &L2Block, timestamp: u64, reverted_block_root: H256) {
        self.number = parent_block.raw().number().unpack() + 1;
        self.parent_block_hash = parent_block.hash().pack();
        self.timestamp = timestamp.pack();
        self.reverted_block_root = reverted_block_root.pack();
        self.prev_account = parent_block.raw().post_account();
        self.state_checkpoint_list.clear();
        self.transactions = RawBlockTransactions::new();
        self.withdrawals = RawBlockWithdrawalRequests::new();
    }

    pub fn push_withdrawal_request(
        &mut self,
        mem_tree: &mut MemTree<'_>,
        req: WithdrawalRequest,
    ) -> Result<()> {
        if self.withdrawals.contains(&req) {
            bail!("duplicate withdrawal {}", req.hash().pack());
        }

        mem_tree.apply_withdrawal_request(
            &self.rollup_context,
            self.block_producer_id.unpack(),
            &req,
        )?;
        let post_account = mem_tree.merkle_state()?;
        let checkpoint = calculate_state_checkpoint(
            &post_account.merkle_root().unpack(),
            post_account.count().unpack(),
        );

        self.state_checkpoint_list.push(checkpoint.pack());
        self.withdrawals.push(req, post_account);

        Ok(())
    }

    pub fn pop_withdrawal_request(&mut self) {
        self.state_checkpoint_list.pop();
        self.withdrawals.pop();
    }

    pub fn push_transaction(
        &mut self,
        mem_tree: &mut MemTree<'_>,
        tx: L2Transaction,
        run_result: &RunResult,
    ) -> Result<()> {
        if self.transactions.contains(&tx) {
            bail!("duplicate transaction {}", tx.hash().pack());
        }

        if self.transactions.inner.is_empty() {
            let checkpoint = {
                let prev_txs_state = mem_tree.get_merkle_state();
                calculate_state_checkpoint(
                    &prev_txs_state.merkle_root().unpack(),
                    prev_txs_state.count().unpack(),
                )
            };

            self.transactions.set_prev_txs_checkpoint(checkpoint);
        }

        mem_tree.apply_run_result(run_result)?;
        let post_account = mem_tree.merkle_state()?;
        let checkpoint = calculate_state_checkpoint(
            &post_account.merkle_root().unpack(),
            post_account.count().unpack(),
        );

        self.state_checkpoint_list.push(checkpoint.pack());
        self.transactions.push(tx, post_account);
        Ok(())
    }

    pub fn pop_transaction(&mut self) {
        self.state_checkpoint_list.pop();
        self.transactions.pop();
    }

    pub fn set_prev_txs_checkpoint(&mut self, checkpoint: H256) {
        self.transactions.set_prev_txs_checkpoint(checkpoint);
    }

    pub fn challenge_last_withdrawal(
        &self,
        db: &StoreTransaction,
        mem_tree: &mut MemTree<'_>,
    ) -> Result<MockChallengeOutput> {
        let target_index = self.withdrawals.inner.len().saturating_sub(1);
        let target_type = ChallengeTargetType::Withdrawal as u8;
        let post_account = {
            let last = self.withdrawals.post_accounts.last();
            last.cloned().expect("exists")
        };
        let raw_block = self.build_block(post_account.clone())?;
        let raw_block_size = raw_block.as_slice().len() as u64;

        let sender_script = {
            let req = self.withdrawals.inner.last().expect("should exists");
            let sender_script_hash = req.raw().account_script_hash().unpack();
            let get = mem_tree.get_script(&sender_script_hash);
            get.ok_or_else(|| anyhow!("withdrawal sender script not found"))?
        };

        let global_state = self.build_global_state(db, post_account, &raw_block)?;
        let challenge_target = ChallengeTarget::new_builder()
            .block_hash(raw_block.hash().pack())
            .target_index(target_index.pack())
            .target_type(target_type.into())
            .build();
        let verify_context = self.build_withdrawal_verify_context(raw_block, sender_script)?;

        Ok(MockChallengeOutput {
            raw_block_size,
            global_state,
            challenge_target,
            verify_context,
        })
    }

    pub fn challenge_last_tx_signature(
        &self,
        db: &StoreTransaction,
        mem_tree: &mut MemTree<'_>,
    ) -> Result<MockChallengeOutput> {
        let target_index = self.transactions.inner.len().saturating_sub(1);
        let target_type = ChallengeTargetType::TxSignature as u8;
        let tx = self.transactions.inner.last().cloned().expect("exists");
        let post_account = {
            let last = self.transactions.post_accounts.last();
            last.cloned().expect("exists")
        };

        let raw_block = self.build_block(post_account.clone())?;
        let raw_block_size = raw_block.as_slice().len() as u64;

        let global_state = self.build_global_state(db, post_account, &raw_block)?;
        let challenge_target = ChallengeTarget::new_builder()
            .block_hash(raw_block.hash().pack())
            .target_index(target_index.pack())
            .target_type(target_type.into())
            .build();

        let verify_context =
            self.build_transaction_signature_verify_context(mem_tree, tx, raw_block)?;

        Ok(MockChallengeOutput {
            raw_block_size,
            global_state,
            challenge_target,
            verify_context,
        })
    }

    pub fn challenge_last_tx_execution(
        &self,
        db: &StoreTransaction,
        mem_tree: &mut MemTree<'_>,
        run_result: &RunResult,
    ) -> Result<MockChallengeOutput> {
        let target_index = self.transactions.inner.len().saturating_sub(1);
        let target_type = ChallengeTargetType::TxExecution as u8;
        let tx = self.transactions.inner.last().cloned().expect("exists");
        let post_account = {
            let last = self.transactions.post_accounts.last();
            last.cloned().expect("exists")
        };

        let raw_block = self.build_block(post_account.clone())?;
        let raw_block_size = raw_block.as_slice().len() as u64;

        let global_state = self.build_global_state(db, post_account, &raw_block)?;
        let challenge_target = ChallengeTarget::new_builder()
            .block_hash(raw_block.hash().pack())
            .target_index(target_index.pack())
            .target_type(target_type.into())
            .build();

        let verify_context =
            self.build_transaction_execution_verify_context(mem_tree, tx, raw_block, run_result)?;

        Ok(MockChallengeOutput {
            raw_block_size,
            global_state,
            challenge_target,
            verify_context,
        })
    }

    fn build_block(&self, post_account: AccountMerkleState) -> Result<RawL2Block> {
        let submit_txs = self.transactions.submit_transactions()?;
        let submit_withdrawals = self.withdrawals.submit_withdrawals()?;

        let raw_block = RawL2Block::new_builder()
            .number(self.number.pack())
            .block_producer_id(self.block_producer_id.clone())
            .stake_cell_owner_lock_hash(self.stake_cell_owner_lock_hash.clone())
            .timestamp(self.timestamp.clone())
            .parent_block_hash(self.parent_block_hash.clone())
            .post_account(post_account)
            .prev_account(self.prev_account.clone())
            .submit_transactions(submit_txs)
            .submit_withdrawals(submit_withdrawals)
            .state_checkpoint_list(self.state_checkpoint_list.clone().pack())
            .build();

        Ok(raw_block)
    }

    fn build_global_state(
        &self,
        db: &StoreTransaction,
        post_account: AccountMerkleState,
        raw_block: &RawL2Block,
    ) -> Result<GlobalState> {
        let block_smt = db.block_smt()?;
        let block_proof = block_smt
            .merkle_proof(vec![H256::from_u64(self.number)])
            .map_err(|err| anyhow!("merkle proof error: {:?}", err))?
            .compile(vec![(H256::from_u64(self.number), H256::zero())])?;
        let post_block = {
            let post_block_root = block_proof.compute_root::<Blake2bHasher>(vec![(
                raw_block.smt_key().into(),
                raw_block.hash().into(),
            )])?;
            let block_count = self.number + 1;
            BlockMerkleState::new_builder()
                .merkle_root(post_block_root.pack())
                .count(block_count.pack())
                .build()
        };

        let last_finalized_block_number = self.number.saturating_sub(self.finality_blocks);

        let global_state = GlobalState::new_builder()
            .account(post_account)
            .block(post_block)
            .tip_block_hash(raw_block.hash().pack())
            .last_finalized_block_number(last_finalized_block_number.pack())
            .reverted_block_root(self.reverted_block_root.clone())
            .rollup_config_hash(self.rollup_config_hash.clone())
            .status((Status::Halting as u8).into())
            .build();

        Ok(global_state)
    }

    fn build_withdrawal_verify_context(
        &self,
        raw_block: RawL2Block,
        sender_script: Script,
    ) -> Result<VerifyContext> {
        let mut tree: SMT<DefaultStore<H256>> = Default::default();
        for (index, witness_hash) in self.withdrawals.witness_hashes.iter().enumerate() {
            tree.update(H256::from_u32(index as u32), witness_hash.to_owned())?;
        }

        let withdrawal_index = self.withdrawals.witness_hashes.len().saturating_sub(1) as u32;
        let withdrawal = {
            let last_withdrawal = &self.withdrawals.inner.last();
            last_withdrawal.cloned().expect("should exists")
        };

        let withdrawal_proof = {
            let indices = &[withdrawal_index];
            let leaves = &self.withdrawals.merkle_leaf_hashes;
            build_cbmt_merkle_proof(leaves, indices).with_context(|| "withdrawal proof")?
        };

        let verify_witness = VerifyWithdrawalWitness::new_builder()
            .raw_l2block(raw_block)
            .withdrawal_request(withdrawal)
            .withdrawal_proof(withdrawal_proof)
            .build();

        Ok(VerifyContext {
            sender_script,
            receiver_script: None,
            verify_witness: VerifyWitness::Withdrawal(verify_witness),
        })
    }

    fn build_transaction_signature_verify_context(
        &self,
        mem_tree: &mut MemTree<'_>,
        tx: L2Transaction,
        raw_block: RawL2Block,
    ) -> Result<VerifyContext> {
        let sender_id = tx.raw().from_id().unpack();
        let receiver_id = tx.raw().to_id().unpack();

        let sender_script = get_script(mem_tree, sender_id)?;
        let receiver_script = get_script(mem_tree, receiver_id)?;

        let kv_state: Vec<(H256, H256)> = vec![
            (
                build_account_field_key(sender_id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
                sender_script.hash().into(),
            ),
            (
                build_account_field_key(receiver_id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
                receiver_script.hash().into(),
            ),
            (
                build_account_field_key(sender_id, GW_ACCOUNT_NONCE_TYPE),
                H256::from_u32(tx.raw().nonce().unpack()),
            ),
        ];
        assert_eq!(
            mem_tree.get_nonce(sender_id)?,
            Unpack::<u32>::unpack(&tx.raw().nonce())
        );

        let touched_keys = kv_state.iter().map(|(key, _)| key.to_owned()).collect();
        let kv_state_proof = {
            let smt = mem_tree.smt();
            smt.merkle_proof(touched_keys)?.compile(kv_state.clone())?
        };

        let scripts = ScriptVec::new_builder()
            .push(sender_script.clone())
            .push(receiver_script.clone())
            .build();

        let account_count = mem_tree.get_account_count()?;
        let context = VerifyTransactionSignatureContext::new_builder()
            .account_count(account_count.pack())
            .kv_state(kv_state.pack())
            .scripts(scripts)
            .build();

        let tx_proof = {
            let indices = &[self.transactions.inner.len().saturating_sub(1) as u32];
            let leaves = &self.transactions.merkle_leaf_hashes;
            build_cbmt_merkle_proof(leaves, indices).with_context(|| "tx proof")?
        };

        let verify_witness = VerifyTransactionSignatureWitness::new_builder()
            .raw_l2block(raw_block)
            .l2tx(tx)
            .tx_proof(tx_proof)
            .kv_state_proof(kv_state_proof.0.pack())
            .context(context)
            .build();

        Ok(VerifyContext {
            sender_script,
            receiver_script: Some(receiver_script),
            verify_witness: VerifyWitness::TxSignature(verify_witness),
        })
    }

    fn build_transaction_execution_verify_context(
        &self,
        mem_tree: &mut MemTree<'_>,
        tx: L2Transaction,
        raw_block: RawL2Block,
        run_result: &RunResult,
    ) -> Result<VerifyContext> {
        let sender_id = tx.raw().from_id().unpack();
        let receiver_id = tx.raw().to_id().unpack();

        let sender_script = get_script(mem_tree, sender_id)?;
        let receiver_script = get_script(mem_tree, receiver_id)?;

        let mut kv_state: HashMap<H256, H256> = HashMap::new();
        kv_state.insert(
            build_account_field_key(sender_id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
            sender_script.hash().into(),
        );
        kv_state.insert(
            build_account_field_key(receiver_id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
            receiver_script.hash().into(),
        );
        kv_state.insert(
            build_account_field_key(sender_id, GW_ACCOUNT_NONCE_TYPE),
            H256::from_u32(tx.raw().nonce().unpack()),
        );
        assert_eq!(
            mem_tree.get_nonce(sender_id)?,
            Unpack::<u32>::unpack(&tx.raw().nonce())
        );

        for key in run_result.read_values.keys() {
            if kv_state.contains_key(key) {
                continue;
            }

            let value = mem_tree.get_raw(key)?;
            kv_state.insert(key.to_owned(), value);
        }

        for key in run_result.write_values.keys() {
            if kv_state.contains_key(key) {
                continue;
            }

            let value = mem_tree.get_raw(key)?;
            kv_state.insert(key.to_owned(), value);
        }

        let touched_keys = kv_state.iter().map(|(key, _)| key.to_owned()).collect();
        let kv_state: Vec<(H256, H256)> = kv_state.into_iter().collect();
        let kv_state_proof = {
            let smt = mem_tree.smt();
            smt.merkle_proof(touched_keys)?.compile(kv_state.clone())?
        };

        let scripts = {
            let mut builder = ScriptVec::new_builder()
                .push(sender_script.clone())
                .push(receiver_script.clone());

            let sender_script_hash = sender_script.hash();
            let receiver_script_hash = receiver_script.hash();

            for slice in run_result.get_scripts.iter() {
                let script = ScriptReader::from_slice_should_be_ok(slice);

                let script_hash = script.hash();
                if script_hash == sender_script_hash || script_hash == receiver_script_hash {
                    continue;
                }

                builder = builder.push(script.to_entity());
            }

            builder.build()
        };

        let load_data: HashMap<H256, Bytes> = {
            let data = run_result.read_data.iter();
            data.map(|(k, v)| (*k, v.pack())).collect()
        };

        let recover_accounts = run_result.recover_accounts.iter().cloned().collect();

        let return_data_hash = {
            let return_data_hash: [u8; 32] = {
                let mut hasher = new_blake2b();
                hasher.update(run_result.return_data.as_slice());
                let mut hash = [0u8; 32];
                hasher.finalize(&mut hash);
                hash
            };

            return_data_hash.pack()
        };

        let account_count = mem_tree.get_account_count()?;
        let context = VerifyTransactionContext::new_builder()
            .account_count(account_count.pack())
            .kv_state(kv_state.pack())
            .scripts(scripts)
            .return_data_hash(return_data_hash)
            .build();

        let tx_proof = {
            let indices = &[self.transactions.inner.len().saturating_sub(1) as u32];
            let leaves = &self.transactions.merkle_leaf_hashes;
            build_cbmt_merkle_proof(leaves, indices).with_context(|| "tx proof")?
        };

        let verify_witness = VerifyTransactionWitness::new_builder()
            .l2tx(tx)
            .raw_l2block(raw_block)
            .tx_proof(tx_proof)
            .kv_state_proof(kv_state_proof.0.pack())
            .context(context)
            .build();

        Ok(VerifyContext {
            sender_script,
            receiver_script: Some(receiver_script),
            verify_witness: VerifyWitness::TxExecution {
                load_data,
                recover_accounts,
                witness: verify_witness,
            },
        })
    }
}

struct RawBlockWithdrawalRequests {
    inner: Vec<WithdrawalRequest>,
    witness_hashes: Vec<H256>,
    merkle_leaf_hashes: Vec<H256>,
    post_accounts: Vec<AccountMerkleState>,
}

impl RawBlockWithdrawalRequests {
    fn new() -> Self {
        RawBlockWithdrawalRequests {
            inner: Vec::new(),
            witness_hashes: Vec::new(),
            merkle_leaf_hashes: Vec::new(),
            post_accounts: Vec::new(),
        }
    }

    fn contains(&self, req: &WithdrawalRequest) -> bool {
        self.witness_hashes.contains(&req.witness_hash().into())
    }

    fn push(&mut self, req: WithdrawalRequest, post_account: AccountMerkleState) {
        let wth_index = self.witness_hashes.len() as u32;
        let witness_hash: H256 = req.witness_hash().into();
        let merkle_leaf_hash = ckb_merkle_leaf_hash(wth_index, &witness_hash);

        self.witness_hashes.push(witness_hash);
        self.merkle_leaf_hashes.push(merkle_leaf_hash);
        self.inner.push(req);
        self.post_accounts.push(post_account);
    }

    fn submit_withdrawals(&self) -> Result<SubmitWithdrawals> {
        let root = calculate_ckb_merkle_root(self.merkle_leaf_hashes.clone())
            .map_err(|err| anyhow!("mock submit withdrawal error: {}", err))?;
        let count = self.inner.len() as u32;

        Ok(SubmitWithdrawals::new_builder()
            .withdrawal_witness_root(root.pack())
            .withdrawal_count(count.pack())
            .build())
    }

    fn pop(&mut self) {
        self.inner.pop();
        self.witness_hashes.pop();
        self.merkle_leaf_hashes.pop();
        self.post_accounts.pop();
    }
}

struct RawBlockTransactions {
    prev_txs_state_checkpoint: Byte32,
    inner: Vec<L2Transaction>,
    post_accounts: Vec<AccountMerkleState>,
    witness_hashes: Vec<H256>,
    merkle_leaf_hashes: Vec<H256>,
}

impl RawBlockTransactions {
    fn new() -> Self {
        RawBlockTransactions {
            prev_txs_state_checkpoint: Byte32::default(),
            inner: Vec::new(),
            post_accounts: Vec::new(),
            witness_hashes: Vec::new(),
            merkle_leaf_hashes: Vec::new(),
        }
    }

    fn set_prev_txs_checkpoint(&mut self, checkpoint: H256) {
        self.prev_txs_state_checkpoint = checkpoint.pack();
    }

    fn contains(&self, tx: &L2Transaction) -> bool {
        self.witness_hashes.contains(&tx.witness_hash().into())
    }

    fn push(&mut self, tx: L2Transaction, post_account: AccountMerkleState) {
        let tx_index = self.merkle_leaf_hashes.len() as u32;
        let witness_hash: H256 = tx.witness_hash().into();
        let merkle_leaf_hash = ckb_merkle_leaf_hash(tx_index, &witness_hash);

        self.witness_hashes.push(witness_hash);
        self.merkle_leaf_hashes.push(merkle_leaf_hash);
        self.post_accounts.push(post_account);
        self.inner.push(tx);
    }

    fn submit_transactions(&self) -> Result<SubmitTransactions> {
        let root = calculate_ckb_merkle_root(self.merkle_leaf_hashes.clone())
            .map_err(|err| anyhow!("mock submit transaction error: {}", err))?;
        let count = self.inner.len() as u32;

        Ok(SubmitTransactions::new_builder()
            .tx_witness_root(root.pack())
            .tx_count(count.pack())
            .prev_state_checkpoint(self.prev_txs_state_checkpoint.clone())
            .build())
    }

    fn pop(&mut self) {
        self.inner.pop();
        self.post_accounts.pop();
        self.witness_hashes.pop();
        self.merkle_leaf_hashes.pop();
    }
}

fn get_script(state: &MemStateTree<'_>, account_id: u32) -> Result<Script> {
    let script_hash = state.get_script_hash(account_id)?;
    state
        .get_script(&script_hash)
        .ok_or_else(|| anyhow!("tx script not found"))
}

fn build_cbmt_merkle_proof(leaves: &[H256], leaf_indices: &[u32]) -> Result<CKBMerkleProof> {
    let proof = CBMT::build_merkle_proof(leaves, leaf_indices)
        .ok_or_else(|| anyhow!("build cbmt proof fail"))?;

    Ok(CKBMerkleProof::new_builder()
        .lemmas(proof.lemmas().pack())
        .indices(proof.indices().pack())
        .build())
}
