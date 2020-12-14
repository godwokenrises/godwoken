//! state context
//! supports read / write to global state

use alloc::collections::BTreeMap;
use gw_common::smt::Blake2bHasher;
use gw_common::sparse_merkle_tree::{CompiledMerkleProof, H256};
use gw_common::state::{Error as StateError, State};

pub struct Context {
    pub number: u64,
    pub aggregator_id: u32,
    pub kv_pairs: BTreeMap<H256, H256>,
    pub kv_merkle_proof: CompiledMerkleProof,
    pub account_count: u32,
    pub rollup_type_hash: [u8; 32],
    pub block_hash: [u8; 32],
}

impl State for Context {
    fn get_raw(&self, raw_key: &[u8; 32]) -> Result<[u8; 32], StateError> {
        let v = self
            .kv_pairs
            .get(&(*raw_key).into())
            .cloned()
            .unwrap_or(H256::zero());
        Ok(v.into())
    }

    fn update_raw(&mut self, key: [u8; 32], value: [u8; 32]) -> Result<(), StateError> {
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

    fn calculate_root(&self) -> Result<[u8; 32], StateError> {
        let root = self
            .kv_merkle_proof
            .compute_root::<Blake2bHasher>(self.kv_pairs.iter().map(|(k, v)| (*k, *v)).collect())
            .map_err(|_err| StateError::MerkleProof)?;
        Ok(root.into())
    }
}
