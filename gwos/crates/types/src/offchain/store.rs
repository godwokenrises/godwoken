use crate::h256::H256;

pub struct SMTRevertedBlockHashes {
    pub prev_smt_root: H256,
    pub block_hashes: Vec<H256>,
}
