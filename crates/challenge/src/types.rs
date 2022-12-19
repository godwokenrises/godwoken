use gw_smt::sparse_merkle_tree::CompiledMerkleProof;
use gw_types::h256::*;
use gw_types::offchain::RecoverAccount;
use gw_types::packed::{
    Bytes, CCTransactionSignatureWitness, CCTransactionWitness, CCWithdrawalWitness, RawL2Block,
    RawL2BlockVec, Script,
};

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum VerifyWitness {
    TxExecution {
        load_data: HashMap<H256, Bytes>,
        recover_accounts: Vec<RecoverAccount>,
        witness: CCTransactionWitness,
    },
    TxSignature(CCTransactionSignatureWitness),
    Withdrawal(CCWithdrawalWitness),
}

#[derive(Debug, Clone)]
pub struct VerifyContext {
    pub sender_script: Script,
    pub receiver_script: Option<Script>,
    pub verify_witness: VerifyWitness,
}

#[derive(Debug, Clone)]
pub struct RevertWitness {
    pub new_tip_block: RawL2Block,
    pub reverted_blocks: RawL2BlockVec, // sorted by block number
    pub block_proof: CompiledMerkleProof,
    pub reverted_block_proof: CompiledMerkleProof,
}

#[derive(Debug, Clone)]
pub struct RevertContext {
    pub post_reverted_block_root: H256,
    pub revert_witness: RevertWitness,
}
