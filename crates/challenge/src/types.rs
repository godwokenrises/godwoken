use gw_common::{sparse_merkle_tree::CompiledMerkleProof, H256};
use gw_types::offchain::RecoverAccount;
use gw_types::packed::{
    Bytes, RawL2Block, RawL2BlockVec, Script, VerifyTransactionSignatureWitness,
    VerifyTransactionWitness, VerifyWithdrawalWitness,
};

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum VerifyWitness {
    TxExecution {
        load_data: HashMap<H256, Bytes>,
        recover_accounts: Vec<RecoverAccount>,
        witness: VerifyTransactionWitness,
    },
    TxSignature(VerifyTransactionSignatureWitness),
    Withdrawal(VerifyWithdrawalWitness),
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
