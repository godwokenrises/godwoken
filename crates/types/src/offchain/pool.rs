use ckb_types::bytes::Bytes;
use sparse_merkle_tree::H256;

use crate::packed::{
    AccountMerkleState, L2Block, L2Transaction, WithdrawalCursor, WithdrawalRequestExtra,
};

use super::DepositInfo;

pub struct BlockParam {
    pub number: u64,
    pub block_producer: Bytes,
    pub timestamp: u64,
    pub txs: Vec<L2Transaction>,
    pub deposits: Vec<DepositInfo>,
    pub withdrawals: Vec<WithdrawalRequestExtra>,
    pub state_checkpoint_list: Vec<H256>,
    pub parent_block: L2Block,
    pub txs_prev_state_checkpoint: H256,
    pub prev_merkle_state: AccountMerkleState,
    pub post_merkle_state: AccountMerkleState,
    pub kv_state: Vec<(H256, H256)>,
    pub kv_state_proof: Vec<u8>,
    pub last_finalized_withdrawal: WithdrawalCursor,
}
