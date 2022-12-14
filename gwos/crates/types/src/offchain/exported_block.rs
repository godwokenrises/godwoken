use crate::{
    h256::H256,
    packed::{DepositInfoVec, GlobalState, L2Block, Script, WithdrawalRequestExtra},
    prelude::{Entity, Unpack},
};

#[derive(Debug)]
pub struct ExportedBlock {
    pub block: L2Block,
    pub post_global_state: GlobalState,
    pub deposit_info_vec: DepositInfoVec,
    pub deposit_asset_scripts: Vec<Script>,
    pub withdrawals: Vec<WithdrawalRequestExtra>,
    pub bad_block_hashes: Option<Vec<Vec<H256>>>,
    pub submit_tx_hash: Option<H256>,
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
        self.block.as_slice() == other.block.as_slice()
            && self.post_global_state.as_slice() == other.post_global_state.as_slice()
            && self.bad_block_hashes == other.bad_block_hashes
            && self.deposit_info_vec.as_slice() == other.deposit_info_vec.as_slice()
            && self.submit_tx_hash == other.submit_tx_hash
    }
}

impl Eq for ExportedBlock {}
