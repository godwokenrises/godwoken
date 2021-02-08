//! state context
//! supports read / write to global state

use alloc::collections::BTreeMap;
use gw_common::smt::Blake2bHasher;
use gw_common::sparse_merkle_tree::{CompiledMerkleProof, H256};
use gw_common::{error::Error as StateError, state::State};
use gw_types::packed::{
    ChallengeLockArgs, CustodianLockArgs, DepositionLockArgs, StakeLockArgs, WithdrawalLockArgs,
};

#[derive(Clone)]
pub struct DepositionRequest {
    // CKB amount
    pub capacity: u64,
    // SUDT amount
    pub amount: u128,
    pub sudt_script_hash: H256,
    pub account_script_hash: H256,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct CellValue {
    pub sudt_script_hash: H256,
    pub amount: u128,
    pub capacity: u64,
}

#[derive(Debug)]
pub struct WithdrawalCell {
    pub index: usize,
    pub args: WithdrawalLockArgs,
    pub value: CellValue,
}

#[derive(Clone)]
pub struct DepositionRequestCell {
    pub index: usize,
    pub args: DepositionLockArgs,
    pub value: CellValue,
    pub account_script_hash: H256,
}

#[derive(Debug)]
pub struct CustodianCell {
    pub index: usize,
    pub args: CustodianLockArgs,
    pub value: CellValue,
}

pub struct StakeCell {
    pub index: usize,
    pub args: StakeLockArgs,
    pub value: CellValue,
}

pub struct ChallengeCell {
    pub index: usize,
    pub args: ChallengeLockArgs,
    pub value: CellValue,
}

pub struct BurnCell {
    pub index: usize,
    pub value: CellValue,
}

#[derive(Clone)]
pub struct WithdrawalRequest {
    pub nonce: u32,
    // CKB amount
    pub capacity: u64,
    // SUDT amount
    pub amount: u128,
    pub sudt_script_hash: H256,
    // layer2 account_script_hash
    pub account_script_hash: H256,
    // Withdrawal request hash
    pub hash: H256,
}
pub struct BlockContext {
    pub number: u64,
    pub finalized_number: u64,
    pub block_hash: H256,
    pub rollup_type_hash: H256,
    pub kv_pairs: BTreeMap<H256, H256>,
    pub kv_merkle_proof: CompiledMerkleProof,
    pub account_count: u32,
    pub prev_account_root: H256,
}

impl State for BlockContext {
    fn get_raw(&self, raw_key: &H256) -> Result<H256, StateError> {
        let v = self
            .kv_pairs
            .get(&(*raw_key).into())
            .cloned()
            .unwrap_or(H256::zero());
        Ok(v.into())
    }

    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), StateError> {
        self.kv_pairs.insert(key.into(), value.into());
        Ok(())
    }

    fn get_account_count(&self) -> Result<u32, StateError> {
        Ok(self.account_count)
    }

    fn set_account_count(&mut self, count: u32) -> Result<(), StateError> {
        self.account_count = count;
        Ok(())
    }

    fn calculate_root(&self) -> Result<H256, StateError> {
        if self.kv_pairs.is_empty() && self.kv_merkle_proof.0.is_empty() {
            return Ok(self.prev_account_root.into());
        }
        let root = self
            .kv_merkle_proof
            .compute_root::<Blake2bHasher>(self.kv_pairs.iter().map(|(k, v)| (*k, *v)).collect())
            .map_err(|_err| StateError::MerkleProof)?;
        Ok(root.into())
    }
}
