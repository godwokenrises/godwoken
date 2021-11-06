use crate::types::{RevertContext, RevertWitness, VerifyContext, VerifyWitness};

use anyhow::{anyhow, Result};
use gw_common::h256_ext::H256Ext;
use gw_common::merkle_utils::{calculate_state_checkpoint, ckb_merkle_leaf_hash, CBMT};
use gw_common::smt::Blake2bHasher;
use gw_common::sparse_merkle_tree::CompiledMerkleProof;
use gw_common::state::State;
use gw_common::{blake2b::new_blake2b, H256};
use gw_generator::constants::L2TX_MAX_CYCLES;
use gw_generator::traits::StateExt;
use gw_generator::{ChallengeContext, Generator};
use gw_store::chain_view::ChainView;
use gw_store::transaction::StoreTransaction;
use gw_traits::CodeStore;
use gw_types::core::ChallengeTargetType;
use gw_types::offchain::RecoverAccount;
use gw_types::packed::{
    BlockHashEntry, BlockHashEntryVec, BlockInfo, Byte32, Bytes, CKBMerkleProof, ChallengeTarget,
    ChallengeWitness, KVPairVec, L2Block, L2Transaction, RawL2Block, RawL2BlockVec,
    RawL2Transaction, Script, ScriptReader, ScriptVec, Uint32, VerifyTransactionContext,
    VerifyTransactionSignatureContext, VerifyTransactionSignatureWitness, VerifyTransactionWitness,
    VerifyWithdrawalWitness,
};
use gw_types::prelude::{Builder, Entity, FromSliceShouldBeOk, Pack, Reader, Unpack};

use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::Arc;

pub fn build_challenge_context(
    db: &StoreTransaction,
    target: ChallengeTarget,
) -> Result<ChallengeContext> {
    let block_hash: H256 = target.block_hash().unpack();
    let block = {
        let opt_ = db.get_block(&block_hash)?;
        opt_.ok_or_else(|| anyhow!("bad block {} not found", hex::encode(block_hash.as_slice())))?
    };

    let block_smt = db.block_smt()?;
    let block_proof = block_smt
        .merkle_proof(vec![block.smt_key().into()])?
        .compile(vec![(block.smt_key().into(), block.hash().into())])?;

    let witness = ChallengeWitness::new_builder()
        .raw_l2block(block.raw())
        .block_proof(block_proof.0.pack())
        .build();

    Ok(ChallengeContext { target, witness })
}

pub fn build_verify_context(
    generator: Arc<Generator>,
    db: &StoreTransaction,
    target: &ChallengeTarget,
) -> Result<VerifyContext> {
    let challenge_type = target.target_type().try_into();
    let block_hash: [u8; 32] = target.block_hash().unpack();
    let target_index = target.target_index().unpack();

    match challenge_type.map_err(|_| anyhow!("invalid challenge type"))? {
        ChallengeTargetType::TxExecution => {
            build_verify_transaction_witness(generator, db, block_hash.into(), target_index)
        }
        ChallengeTargetType::TxSignature => {
            build_verify_transaction_signature_witness(db, block_hash.into(), target_index)
        }
        ChallengeTargetType::Withdrawal => {
            build_verify_withdrawal_witness(db, block_hash.into(), target_index)
        }
    }
}

/// NOTE: Caller should rollback db, only update reverted_block_smt in L1ActionContext::Revert
pub fn build_revert_context(
    db: &StoreTransaction,
    reverted_blocks: &[L2Block],
) -> Result<RevertContext> {
    // Build main chain block proof
    let reverted_blocks = reverted_blocks.iter();
    let reverted_raw_blocks: Vec<RawL2Block> = reverted_blocks.map(|rb| rb.raw()).collect();
    let (_, block_proof) = build_block_proof(db, &reverted_raw_blocks)?;
    log::debug!("build main chain block proof");

    // Build reverted block proof
    let (post_reverted_block_root, reverted_block_proof) = {
        let mut smt = db.reverted_block_smt()?;
        let to_key = |b: &RawL2Block| H256::from(b.hash());
        let to_leave = |b: &RawL2Block| (to_key(b), H256::one());

        let keys: Vec<H256> = reverted_raw_blocks.iter().map(to_key).collect();
        for key in keys.iter() {
            smt.update(key.to_owned(), H256::one())?;
        }

        let root = smt.root().to_owned();
        let leaves = reverted_raw_blocks.iter().map(to_leave).collect();
        let proof = smt.merkle_proof(keys)?.compile(leaves)?;

        (root, proof)
    };
    log::debug!("build reverted block proof");

    let new_tip_block = {
        let first_reverted_block = reverted_raw_blocks.first();
        let tip_block_hash = first_reverted_block.map(|b| b.parent_block_hash().unpack());
        let to_block = tip_block_hash.map(|h| db.get_block(&h)).transpose()?;
        let to_raw = to_block.flatten().map(|b| b.raw());
        to_raw.ok_or_else(|| anyhow!("block not found"))?
    };

    let reverted_blocks = RawL2BlockVec::new_builder()
        .extend(reverted_raw_blocks)
        .build();

    let revert_witness = RevertWitness {
        new_tip_block,
        reverted_blocks,
        block_proof,
        reverted_block_proof,
    };

    Ok(RevertContext {
        post_reverted_block_root,
        revert_witness,
    })
}

fn build_verify_withdrawal_witness(
    db: &StoreTransaction,
    block_hash: H256,
    withdrawal_index: u32,
) -> Result<VerifyContext> {
    let block = db
        .get_block(&block_hash)?
        .ok_or_else(|| anyhow!("block not found"))?;

    // Build withdrawal proof
    let mut target = None;
    let leaves: Vec<H256> = block
        .withdrawals()
        .into_iter()
        .enumerate()
        .map(|(idx, withdrawal)| {
            let hash: H256 = withdrawal.witness_hash().into();
            if idx == withdrawal_index as usize {
                target = Some(withdrawal);
            }
            ckb_merkle_leaf_hash(idx as u32, &hash)
        })
        .collect();
    let withdrawal = target.ok_or_else(|| anyhow!("withdrawal not found in block"))?;
    let proof = build_merkle_proof(&leaves, &[withdrawal_index])?;
    log::debug!("build withdrawal proof");

    // Get sender account script
    let sender_script_hash: [u8; 32] = withdrawal.raw().account_script_hash().unpack();
    let sender_script = {
        let raw_block = block.raw();
        let check_point = CheckPoint::new(raw_block.number().unpack() - 1, SubState::Block);
        let state_db = StateDBTransaction::from_checkpoint(db, check_point, StateDBMode::ReadOnly)?;
        let tree = state_db.state_tree()?;

        tree.get_script(&sender_script_hash.into())
            .ok_or_else(|| anyhow!("sender script not found"))?
    };

    let verify_witness = VerifyWithdrawalWitness::new_builder()
        .raw_l2block(block.raw())
        .withdrawal_request(withdrawal)
        .withdrawal_proof(proof)
        .build();

    Ok(VerifyContext {
        sender_script,
        receiver_script: None,
        verify_witness: VerifyWitness::Withdrawal(verify_witness),
    })
}

fn build_merkle_proof(leaves: &[H256], indices: &[u32]) -> Result<CKBMerkleProof> {
    let proof = CBMT::build_merkle_proof(leaves, indices)
        .ok_or_else(|| anyhow!("Build merkle proof failed."))?;
    let proof = CKBMerkleProof::new_builder()
        .lemmas(proof.lemmas().pack())
        .indices(proof.indices().pack())
        .build();
    Ok(proof)
}

fn build_verify_transaction_signature_witness(
    db: &StoreTransaction,
    block_hash: H256,
    tx_index: u32,
) -> Result<VerifyContext> {
    let block = db
        .get_block(&block_hash)?
        .ok_or_else(|| anyhow!("block not found"))?;

    let (tx, tx_proof) = build_tx_proof(&block, tx_index)?;

    log::debug!("build tx proof");

    let kv_witness = build_tx_kv_witness(db, &block, &tx.raw(), tx_index, TxKvState::Signature)?;
    log::debug!("build kv witness");

    let context = VerifyTransactionSignatureContext::new_builder()
        .account_count(kv_witness.account_count)
        .kv_state(kv_witness.kv_state)
        .scripts(kv_witness.scripts)
        .build();

    let verify_witness = VerifyTransactionSignatureWitness::new_builder()
        .raw_l2block(block.raw())
        .l2tx(tx)
        .tx_proof(tx_proof)
        .kv_state_proof(kv_witness.kv_state_proof.0.pack())
        .context(context)
        .build();

    Ok(VerifyContext {
        sender_script: kv_witness.sender_script,
        receiver_script: Some(kv_witness.receiver_script),
        verify_witness: VerifyWitness::TxSignature(verify_witness),
    })
}

fn build_verify_transaction_witness(
    generator: Arc<Generator>,
    db: &StoreTransaction,
    block_hash: H256,
    tx_index: u32,
) -> Result<VerifyContext> {
    let block = db
        .get_block(&block_hash)?
        .ok_or_else(|| anyhow!("block not found"))?;
    let raw_block = block.raw();

    let (tx, tx_proof) = build_tx_proof(&block, tx_index)?;
    log::debug!("build tx proof");

    let tx_kv_state = TxKvState::Execution { generator };
    let kv_witness = build_tx_kv_witness(db, &block, &tx.raw(), tx_index, tx_kv_state)?;
    log::debug!("build kv witness");

    let return_data_hash = kv_witness
        .return_data_hash
        .expect("execution return data hash not found");

    // TODO: block hashes and proof?
    let context = VerifyTransactionContext::new_builder()
        .account_count(kv_witness.account_count)
        .kv_state(kv_witness.kv_state)
        .scripts(kv_witness.scripts)
        .return_data_hash(return_data_hash)
        .build();

    let verify_witness = VerifyTransactionWitness::new_builder()
        .l2tx(tx)
        .raw_l2block(raw_block)
        .tx_proof(tx_proof)
        .kv_state_proof(kv_witness.kv_state_proof.0.pack())
        .context(context)
        .build();

    Ok(VerifyContext {
        sender_script: kv_witness.sender_script,
        receiver_script: Some(kv_witness.receiver_script),
        verify_witness: VerifyWitness::TxExecution {
            load_data: kv_witness.load_data.unwrap_or_else(HashMap::default),
            recover_accounts: kv_witness.recover_accounts.unwrap_or_else(Vec::default),
            witness: verify_witness,
        },
    })
}

// Build proof with ckb merkle tree.
fn build_tx_proof(block: &L2Block, tx_index: u32) -> Result<(L2Transaction, CKBMerkleProof)> {
    let mut target_tx = None;
    let leaves: Vec<H256> = block
        .transactions()
        .into_iter()
        .enumerate()
        .map(|(idx, tx)| {
            let hash: H256 = tx.witness_hash().into();
            if idx == tx_index as usize {
                target_tx = Some(tx);
            }
            ckb_merkle_leaf_hash(idx as u32, &hash)
        })
        .collect();
    let tx = target_tx.ok_or_else(|| anyhow!("tx not found in block"))?;
    let proof = build_merkle_proof(&leaves, &[tx_index])?;
    Ok((tx, proof))
}

enum TxKvState {
    Execution { generator: Arc<Generator> },
    Signature,
}

struct TxKvWitness {
    account_count: Uint32,
    scripts: ScriptVec,
    load_data: Option<HashMap<H256, Bytes>>,
    recover_accounts: Option<Vec<RecoverAccount>>,
    sender_script: Script,
    receiver_script: Script,
    kv_state: KVPairVec,
    kv_state_proof: CompiledMerkleProof,
    return_data_hash: Option<Byte32>,
}

fn build_tx_kv_witness(
    db: &StoreTransaction,
    block: &L2Block,
    raw_tx: &RawL2Transaction,
    tx_index: u32,
    tx_kv_state: TxKvState,
) -> Result<TxKvWitness> {
    let raw_block = block.as_reader().raw();
    let withdrawal_len: u32 = {
        let withdrawals = raw_block.submit_withdrawals();
        withdrawals.withdrawal_count().unpack()
    };

    let (local_prev_tx_checkpoint, block_prev_tx_checkpoint): (CheckPoint, [u8; 32]) = {
        let block_number = raw_block.number().unpack();
        match (tx_index).checked_sub(1) {
            Some(prev_tx_index) => {
                let local_prev_tx_checkpoint =
                    CheckPoint::new(block_number, SubState::Tx(prev_tx_index));

                let block_prev_tx_checkpoint = raw_block
                    .state_checkpoint_list()
                    .get((withdrawal_len + prev_tx_index) as usize)
                    .ok_or_else(|| anyhow!("block prev tx checkpoint not found"))?;

                (local_prev_tx_checkpoint, block_prev_tx_checkpoint.unpack())
            }
            None => {
                let local_prev_tx_checkpoint = CheckPoint::new(block_number, SubState::PrevTxs);
                let block_prev_tx_checkpoint =
                    raw_block.submit_transactions().prev_state_checkpoint();

                (local_prev_tx_checkpoint, block_prev_tx_checkpoint.unpack())
            }
        }
    };

    let state_db =
        StateDBTransaction::from_checkpoint(db, local_prev_tx_checkpoint, StateDBMode::ReadOnly)?;
    let mut tree = state_db.state_tree()?;
    let prev_tx_account_count = tree.get_account_count()?;

    // Check prev tx account state
    {
        let local_checkpoint: [u8; 32] = tree.calculate_state_checkpoint()?.into();
        assert_eq!(local_checkpoint, block_prev_tx_checkpoint);
    }

    tree.tracker_mut().enable();

    let get_script = |state: &StateTree<'_, '_>, account_id: u32| -> Result<Option<Script>> {
        let script_hash = state.get_script_hash(account_id)?;
        Ok(state.get_script(&script_hash))
    };

    let sender_id = raw_tx.from_id().unpack();
    let receiver_id = raw_tx.to_id().unpack();

    let sender_script =
        get_script(&tree, sender_id)?.ok_or_else(|| anyhow!("sender script not found"))?;
    let receiver_script =
        get_script(&tree, receiver_id)?.ok_or_else(|| anyhow!("receiver script not found"))?;

    // To verify transaction signature
    tree.get_nonce(sender_id)?;

    let opt_run_result = match tx_kv_state {
        TxKvState::Execution { ref generator } => {
            let parent_block_hash = db
                .get_block_hash_by_number(raw_block.number().unpack())?
                .ok_or_else(|| anyhow!("parent block not found"))?;
            let chain_view = ChainView::new(db, parent_block_hash);
            let block_info = BlockInfo::new_builder()
                .number(raw_block.number().to_entity())
                .timestamp(raw_block.timestamp().to_entity())
                .block_producer_id(raw_block.block_producer_id().to_entity())
                .build();

            let run_result = generator.execute_transaction(
                &chain_view,
                &tree,
                &block_info,
                raw_tx,
                L2TX_MAX_CYCLES,
            )?;
            tree.apply_run_result(&run_result)?;

            Some(run_result)
        }
        TxKvState::Signature => None,
    };

    let block_post_tx_checkpoint: [u8; 32] = raw_block
        .state_checkpoint_list()
        .get((withdrawal_len + tx_index) as usize)
        .ok_or_else(|| anyhow!("block tx checkpoint not found"))?
        .unpack();

    if matches!(tx_kv_state, TxKvState::Execution { .. }) {
        // Check post tx account state
        let local_checkpoint: [u8; 32] = tree.calculate_state_checkpoint()?.into();
        assert_eq!(local_checkpoint, block_post_tx_checkpoint);
    }

    let touched_keys: Vec<H256> = {
        let opt_keys = tree.tracker_mut().touched_keys();
        let keys = opt_keys.ok_or_else(|| anyhow!("no key touched"))?;
        let clone_keys = keys.borrow().clone().into_iter();
        clone_keys.collect()
    };
    let post_tx_account_count = tree.get_account_count()?;
    let post_kv_state = {
        let keys = touched_keys.iter();
        let to_kv = keys.map(|k| {
            let v = tree.get_raw(k)?;
            Ok((*k, v))
        });
        to_kv.collect::<Result<Vec<(H256, H256)>>>()
    }?;

    // Discard all changes
    drop(tree);
    db.rollback()?;

    tree = state_db.state_tree()?;
    let prev_kv_state = {
        let keys = touched_keys.iter();
        let to_kv = keys.map(|k| {
            let v = tree.get_raw(k)?;
            Ok((*k, v))
        });
        to_kv.collect::<Result<Vec<(H256, H256)>>>()
    }?;

    let kv_state_proof = {
        let smt = state_db.account_smt()?;
        let prev_kv_state = prev_kv_state.clone();
        smt.merkle_proof(touched_keys)?.compile(prev_kv_state)?
    };
    log::debug!("build kv state proof");

    // Check proof
    {
        let proof_root = kv_state_proof.compute_root::<Blake2bHasher>(prev_kv_state.clone())?;
        let proof_checkpoint = calculate_state_checkpoint(&proof_root, prev_tx_account_count);
        assert_eq!(proof_checkpoint, block_prev_tx_checkpoint.into());

        if matches!(tx_kv_state, TxKvState::Execution { .. }) {
            let proof_root = kv_state_proof.compute_root::<Blake2bHasher>(post_kv_state)?;
            let proof_checkpoint = calculate_state_checkpoint(&proof_root, post_tx_account_count);
            assert_eq!(proof_checkpoint, block_post_tx_checkpoint.into());
        }
    }

    let scripts = {
        let mut builder = ScriptVec::new_builder()
            .push(sender_script.clone())
            .push(receiver_script.clone());

        if let Some(ref run_result) = opt_run_result {
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
        }

        builder.build()
    };

    let load_data = {
        let to_read_data = opt_run_result.as_ref().map(|r| r.read_data.iter());
        to_read_data.map(|d| d.map(|(k, v)| (*k, v.pack())).collect())
    };

    let return_data_hash = opt_run_result.as_ref().map(|result| {
        let return_data_hash: [u8; 32] = {
            let mut hasher = new_blake2b();
            hasher.update(result.return_data.as_slice());
            let mut hash = [0u8; 32];
            hasher.finalize(&mut hash);
            hash
        };

        return_data_hash.pack()
    });
    log::debug!("return data hash {:?}", return_data_hash);

    let recover_accounts = opt_run_result.map(|r| r.recover_accounts.into_iter().collect());

    let witness = TxKvWitness {
        account_count: prev_tx_account_count.pack(),
        scripts,
        load_data,
        recover_accounts,
        sender_script,
        receiver_script,
        kv_state: prev_kv_state.pack(),
        kv_state_proof,
        return_data_hash,
    };

    Ok(witness)
}

fn build_block_proof(
    db: &StoreTransaction,
    raw_blocks: &[RawL2Block],
) -> Result<(BlockHashEntryVec, CompiledMerkleProof)> {
    let block_entries = {
        let to_entry = raw_blocks.iter().map(|rb| {
            BlockHashEntry::new_builder()
                .number(rb.number())
                .hash(rb.hash().pack())
                .build()
        });
        to_entry.collect::<Vec<_>>()
    };

    let block_hashes = BlockHashEntryVec::new_builder()
        .extend(block_entries)
        .build();

    let block_proof = {
        let smt = db.block_smt()?;
        let to_leave = |b: &RawL2Block| (b.smt_key().into(), b.hash().into());

        let smt_keys = raw_blocks.iter().map(|rb| rb.smt_key().into());
        let leaves = raw_blocks.iter().map(to_leave);
        smt.merkle_proof(smt_keys.collect())?
            .compile(leaves.collect())?
    };

    Ok((block_hashes, block_proof))
}

#[cfg(test)]
mod tests {
    use gw_common::{
        merkle_utils::{calculate_ckb_merkle_root, ckb_merkle_leaf_hash, CBMTMerkleProof},
        H256,
    };
    use gw_types::{
        packed::{L2Block, L2Transaction, RawL2Transaction},
        prelude::*,
    };

    use crate::context::build_tx_proof;

    #[test]
    fn build_tx_proof_test() {
        // mock block
        let leaves = vec![2u32, 3, 5, 7, 11];
        let tx_vec: Vec<L2Transaction> = leaves
            .iter()
            .map(move |v| {
                L2Transaction::new_builder()
                    .raw(
                        RawL2Transaction::new_builder()
                            .from_id(v.pack())
                            .to_id(v.pack())
                            .build(),
                    )
                    .build()
            })
            .collect();
        let block = L2Block::new_builder()
            .transactions(tx_vec.clone().pack())
            .build();
        // gerenate proof
        let proof = build_tx_proof(&block, 4);
        assert!(proof.is_ok());

        // rebuild proof
        if let Ok((tx, mk_proof)) = proof {
            assert_eq!(&tx, &tx_vec[4]);
            let index: u32 = 4;
            let hash: H256 = tx.witness_hash().into();
            let hash = ckb_merkle_leaf_hash(index, &hash);
            let proof_leaves = vec![hash];
            let indices = mk_proof
                .indices()
                .into_iter()
                .map(|i| i.unpack())
                .collect::<_>();
            let lemmas = mk_proof
                .lemmas()
                .into_iter()
                .map(|v| v.unpack())
                .collect::<_>();
            let proof = CBMTMerkleProof::new(indices, lemmas);

            // get root
            let root = calculate_ckb_merkle_root(
                tx_vec
                    .into_iter()
                    .enumerate()
                    .map(|(id, l)| ckb_merkle_leaf_hash(id as u32, &l.witness_hash().into()))
                    .collect(),
            );
            assert!(root.is_ok());

            // verify
            assert!(proof.verify(&root.unwrap(), &proof_leaves));
        }
    }
}
