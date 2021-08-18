use gw_types::{
    packed::{L2Transaction, WithdrawalRequest},
    prelude::*,
};

#[derive(Default)]
pub struct EntryList {
    // txs sorted by nonce
    pub txs: Vec<L2Transaction>,
    // withdrawals sorted by nonce
    pub withdrawals: Vec<WithdrawalRequest>,
}

impl EntryList {
    pub fn is_empty(&self) -> bool {
        self.txs.is_empty() && self.withdrawals.is_empty()
    }

    // remove and return txs which tx.nonce is lower than nonce
    pub fn remove_lower_nonce_txs(&mut self, nonce: u32) -> Vec<L2Transaction> {
        let mut removed = Vec::default();
        while !self.txs.is_empty() {
            let tx_nonce: u32 = self.txs[0].raw().nonce().unpack();
            if tx_nonce >= nonce {
                break;
            }
            removed.push(self.txs.remove(0));
        }
        removed
    }

    // remove and return withdrawals which withdrawal.nonce is lower than nonce & have not enough balance
    pub fn remove_lower_nonce_withdrawals(
        &mut self,
        nonce: u32,
        capacity: u128,
    ) -> Vec<WithdrawalRequest> {
        let mut removed = Vec::default();

        // remove lower nonce withdrawals
        while !self.withdrawals.is_empty() {
            let withdrawal_nonce: u32 = self.withdrawals[0].raw().nonce().unpack();
            if withdrawal_nonce >= nonce {
                break;
            }
            removed.push(self.withdrawals.remove(0));
        }

        // remove lower balance withdrawals
        if let Some(withdrawal) = self.withdrawals.get(0) {
            let withdrawal_capacity: u64 = withdrawal.raw().capacity().unpack();
            if (withdrawal_capacity as u128) > capacity {
                // TODO instead of remove all withdrawals, put them into future queue
                removed.extend_from_slice(&self.withdrawals);
                self.withdrawals.clear();
            }
        }

        removed
    }
}
