use crate::packed::{
    AccountMerkleState, Byte32, CompactMemBlock, DeprecatedWithdrawRequestExtra, GlobalState,
    GlobalStateV0, MemBlock, RawWithdrawalRequest, Script, TransactionKey, TxReceipt,
    WithdrawalKey, WithdrawalRequest, WithdrawalRequestExtra,
};
use crate::prelude::*;
use ckb_types::error::VerificationError;
use sparse_merkle_tree::H256;

use super::RunResult;

impl TransactionKey {
    pub fn build_transaction_key(block_hash: Byte32, index: u32) -> Self {
        let mut key = [0u8; 36];
        key[..32].copy_from_slice(block_hash.as_slice());
        // use BE, so we have a sorted bytes representation
        key[32..].copy_from_slice(&index.to_be_bytes());
        key.pack()
    }
}

impl WithdrawalKey {
    pub fn build_withdrawal_key(block_hash: Byte32, index: u32) -> Self {
        let mut key = [0u8; 36];
        key[..32].copy_from_slice(block_hash.as_slice());
        // use BE, so we have a sorted bytes representation
        key[32..].copy_from_slice(&index.to_be_bytes());
        key.pack()
    }
}

impl TxReceipt {
    pub fn build_receipt(
        tx_witness_hash: H256,
        run_result: RunResult,
        post_state: AccountMerkleState,
    ) -> Self {
        TxReceipt::new_builder()
            .tx_witness_hash(tx_witness_hash.pack())
            .post_state(post_state)
            .read_data_hashes(
                run_result
                    .read_data
                    .into_iter()
                    .map(|(hash, _)| hash.pack())
                    .collect::<Vec<_>>()
                    .pack(),
            )
            .logs(run_result.logs.pack())
            .build()
    }
}

pub fn global_state_from_slice(slice: &[u8]) -> Result<GlobalState, VerificationError> {
    match GlobalState::from_slice(slice) {
        Ok(state) => Ok(state),
        Err(_) => GlobalStateV0::from_slice(slice).map(Into::into),
    }
}

impl From<MemBlock> for CompactMemBlock {
    fn from(block: MemBlock) -> Self {
        CompactMemBlock::new_builder()
            .txs(block.txs())
            .withdrawals(block.withdrawals())
            .deposits(block.deposits())
            .build()
    }
}

impl CompactMemBlock {
    pub fn from_full_compatible_slice(slice: &[u8]) -> Result<CompactMemBlock, VerificationError> {
        match CompactMemBlock::from_slice(slice) {
            Ok(block) => Ok(block),
            Err(_) => MemBlock::from_slice(slice).map(Into::into),
        }
    }
}

impl WithdrawalRequestExtra {
    pub fn hash(&self) -> [u8; 32] {
        self.request().hash()
    }

    pub fn witness_hash(&self) -> [u8; 32] {
        self.request().witness_hash()
    }

    pub fn raw(&self) -> RawWithdrawalRequest {
        self.request().raw()
    }

    pub fn opt_owner_lock(&self) -> Option<Script> {
        self.owner_lock().to_opt()
    }

    pub fn from_request_compitable_slice(
        slice: &[u8],
    ) -> Result<WithdrawalRequestExtra, VerificationError> {
        if let Ok(extra) = Self::from_len_header_slice(slice) {
            return Ok(extra);
        }

        if let Ok(deprecated_extra) = DeprecatedWithdrawRequestExtra::from_slice(slice) {
            let extra = WithdrawalRequestExtra::new_builder()
                .request(deprecated_extra.request())
                .owner_lock(deprecated_extra.owner_lock())
                .withdraw_to_v1(0u8.into())
                .build();
            return Ok(extra);
        }

        match WithdrawalRequestExtra::from_slice(slice) {
            Ok(withdrawal) => Ok(withdrawal),
            Err(_) => WithdrawalRequest::from_slice(slice).map(Into::into),
        }
    }

    // Support slice without schema updated
    fn from_len_header_slice(slice: &[u8]) -> Result<WithdrawalRequestExtra, VerificationError> {
        if slice.len() < 8 {
            return Err(VerificationError::FieldCountNotMatch(
                "WithdrawalRequestExtra header flag".to_string(),
                1,
                0,
            ));
        }

        let (len_slice, withdrawal_lock_slice) = slice.split_at(8);
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&len_slice[0..4]);
        let withdrawal_len = u32::from_be_bytes(buf) as usize;
        buf.copy_from_slice(&len_slice[4..8]);
        let owner_lock_len = u32::from_be_bytes(buf) as usize;
        if withdrawal_len + owner_lock_len != withdrawal_lock_slice.len() {
            return Err(VerificationError::FieldCountNotMatch(
                "WithdrawalRequestExtra withdrawal owner lock".to_string(),
                2,
                0,
            ));
        }

        let (withdrawal_slice, owner_lock_slice) = withdrawal_lock_slice.split_at(withdrawal_len);
        let withdrawal = WithdrawalRequest::from_slice(withdrawal_slice)?;
        let owner_lock = Script::from_slice(owner_lock_slice)?;

        Ok(WithdrawalRequestExtra::new_builder()
            .request(withdrawal)
            .owner_lock(Some(owner_lock).pack())
            .build())
    }
}

impl From<WithdrawalRequest> for WithdrawalRequestExtra {
    fn from(req: WithdrawalRequest) -> Self {
        WithdrawalRequestExtra::new_builder()
            .request(req)
            .withdraw_to_v1(0u8.into())
            .build()
    }
}

impl PartialEq for WithdrawalRequestExtra {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for WithdrawalRequestExtra {}

#[cfg(test)]
mod test {
    use ckb_types::prelude::Entity;

    use crate::packed::{
        DeprecatedWithdrawRequestExtra, Script, WithdrawalRequest, WithdrawalRequestExtra,
    };

    #[test]
    fn test_withdrawal_request_extra_from_len_header_slice() {
        let withdrawal = WithdrawalRequest::default();
        let owner_lock = Script::default();
        let withdrawal_len_bytes = (withdrawal.as_slice().len() as u32).to_be_bytes();
        let lock_len_bytes = (owner_lock.as_slice().len() as u32).to_be_bytes();

        let mut raw = withdrawal_len_bytes.to_vec();
        raw.extend_from_slice(&lock_len_bytes);
        raw.extend_from_slice(withdrawal.as_slice());
        raw.extend_from_slice(owner_lock.as_slice());
        WithdrawalRequestExtra::from_len_header_slice(&raw).expect("valid");

        let raw = withdrawal_len_bytes.to_vec();
        let err = WithdrawalRequestExtra::from_len_header_slice(&raw).unwrap_err();
        assert!(err.to_string().contains("header flag"));

        let mut raw = withdrawal_len_bytes.to_vec();
        raw.extend_from_slice(withdrawal.as_slice());
        let err = WithdrawalRequestExtra::from_len_header_slice(&raw).unwrap_err();
        assert!(err.to_string().contains("owner lock"));

        let mut raw = lock_len_bytes.to_vec();
        raw.extend_from_slice(&lock_len_bytes);
        raw.extend_from_slice(owner_lock.as_slice());
        raw.extend_from_slice(owner_lock.as_slice());
        let err = WithdrawalRequestExtra::from_len_header_slice(&raw).unwrap_err();
        assert!(err.to_string().contains("field count doesn't match"));

        let mut raw = withdrawal_len_bytes.to_vec();
        raw.extend_from_slice(&withdrawal_len_bytes);
        raw.extend_from_slice(withdrawal.as_slice());
        raw.extend_from_slice(withdrawal.as_slice());
        let err = WithdrawalRequestExtra::from_len_header_slice(&raw).unwrap_err();
        assert!(err.to_string().contains("field count doesn't match"));
    }

    #[test]
    fn test_withdrawal_request_extra_from_deprecated_withdrawal_request_extra() {
        let deprecated = DeprecatedWithdrawRequestExtra::default();
        let withdraw = WithdrawalRequestExtra::from_request_compitable_slice(deprecated.as_slice());
        assert_eq!(withdraw.expect("valid").withdraw_to_v1(), 0u8.into());
    }
}
