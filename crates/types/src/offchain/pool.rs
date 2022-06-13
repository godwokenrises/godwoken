use std::collections::{hash_map::Entry, HashMap};

use ckb_types::bytes::Bytes;
use sparse_merkle_tree::H256;

use crate::packed::{AccountMerkleState, L2Block, L2Transaction, Script, WithdrawalRequestExtra};

use super::{CollectedCustodianCells, DepositInfo};

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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FinalizedCustodianCapacity {
    pub capacity: u128,
    pub sudt: HashMap<[u8; 32], (u128, Script)>,
}

impl FinalizedCustodianCapacity {
    pub fn is_empty(&self) -> bool {
        self.capacity == 0 && self.sudt.is_empty()
    }

    /// Add sudt amount with overflow check.
    ///
    /// Returns new amount of the sudt if not overflow.
    pub fn checked_add_sudt(
        &mut self,
        hash: [u8; 32],
        amount: u128,
        script: Script,
    ) -> Option<u128> {
        match self.sudt.entry(hash) {
            Entry::Occupied(mut e) => {
                let pointer = e.get_mut();
                pointer.0 = pointer.0.checked_add(amount)?;
                pointer.1 = script;
                Some(pointer.0)
            }
            Entry::Vacant(v) => {
                v.insert((amount, script));
                Some(amount)
            }
        }
    }
}

impl From<CollectedCustodianCells> for FinalizedCustodianCapacity {
    fn from(c: CollectedCustodianCells) -> Self {
        Self {
            capacity: c.capacity,
            sudt: c.sudt,
        }
    }
}
