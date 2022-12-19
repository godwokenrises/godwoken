use crate::h256::H256;
use crate::packed::Script;

pub struct SudtCustodian {
    pub script_hash: H256,
    pub amount: u128,
    pub script: Script,
}
