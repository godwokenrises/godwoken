use sparse_merkle_tree::H256;

use crate::{
    packed::{DepositRequest, GlobalState, L2Block, Script, WithdrawalRequestExtra},
    prelude::{Entity, Unpack},
};

#[derive(Debug)]
pub struct ExportedBlock {
    pub block: L2Block,
    pub post_global_state: GlobalState,
    pub deposit_requests: Vec<DepositRequest>,
    pub deposit_asset_scripts: Vec<Script>,
    pub withdrawals: Vec<WithdrawalRequestExtra>,
    pub bad_block_hashes: Option<Vec<Vec<H256>>>,
}

impl ExportedBlock {
    pub fn block_number(&self) -> u64 {
        self.block.raw().number().unpack()
    }

    pub fn block_hash(&self) -> H256 {
        self.block.hash().into()
    }

    pub fn parent_block_hash(&self) -> H256 {
        self.block.raw().parent_block_hash().unpack()
    }
}

impl PartialEq for ExportedBlock {
    fn eq(&self, other: &Self) -> bool {
        let self_deposits = self.deposit_requests.iter().map(|d| d.as_slice());
        let other_deposits = other.deposit_requests.iter().map(|d| d.as_slice());

        self.block.as_slice() == other.block.as_slice()
            && self.post_global_state.as_slice() == other.post_global_state.as_slice()
            && self.bad_block_hashes == other.bad_block_hashes
            && self_deposits.eq(other_deposits)
    }
}

impl Eq for ExportedBlock {}
