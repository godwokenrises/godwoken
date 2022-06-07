use std::collections::{hash_map::Entry, HashMap};

use ckb_types::bytes::Bytes;
use sparse_merkle_tree::H256;

use crate::packed::{AccountMerkleState, L2Block, L2Transaction, Script, WithdrawalRequestExtra};

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
    pub remaining_capacity: FinalizedCustodianCapacity,
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
}

impl core::ops::Add<FinalizedCustodianCapacity> for FinalizedCustodianCapacity {
    type Output = FinalizedCustodianCapacity;

    fn add(self, rhs: FinalizedCustodianCapacity) -> Self::Output {
        let mut sudt = self.sudt;
        for (h, (amount, script)) in rhs.sudt {
            match sudt.entry(h) {
                Entry::Occupied(mut occupied) => {
                    let pointer = occupied.get_mut();
                    pointer.0 += amount;
                    pointer.1 = script;
                }
                Entry::Vacant(vacant) => {
                    vacant.insert((amount, script));
                }
            }
        }
        FinalizedCustodianCapacity {
            capacity: self.capacity + rhs.capacity,
            sudt,
        }
    }
}
