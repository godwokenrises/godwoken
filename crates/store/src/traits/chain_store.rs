#![allow(clippy::mutable_key_type)]

use std::cell::RefCell;

use anyhow::Result;
use gw_types::{
    from_box_should_be_ok,
    h256::H256,
    offchain::{global_state_from_slice, SMTRevertedBlockHashes},
    packed::{
        self, ChallengeTarget, DepositInfoVec, FinalizedCustodianCapacity, NumberHash,
        NumberHashReader, Script, Transaction, TransactionKey, WithdrawalKey,
    },
    prelude::*,
};

use crate::{schema::*, traits::kv_store::KVStoreRead};

/// L2 block status on L1.
pub enum BlockStatus {
    Confirmed,
    Submitted,
    Local,
}

pub trait ChainStore: KVStoreRead {
    fn has_genesis(&self) -> Result<bool> {
        Ok(self.get_block_hash_by_number(0)?.is_some())
    }

    fn get_chain_id(&self) -> Result<H256> {
        let slice = self
            .get(COLUMN_META, META_CHAIN_ID_KEY)
            .expect("must has chain_id");
        debug_assert_eq!(slice.len(), 32);
        let mut chain_id = [0u8; 32];
        chain_id.copy_from_slice(&slice);
        Ok(chain_id)
    }

    fn get_block_smt_root(&self) -> Result<H256> {
        let slice = self
            .get(COLUMN_META, META_BLOCK_SMT_ROOT_KEY)
            .expect("must has root");
        debug_assert_eq!(slice.len(), 32);
        let mut root = [0u8; 32];
        root.copy_from_slice(&slice);
        Ok(root)
    }

    fn get_reverted_block_smt_root(&self) -> Result<H256> {
        let slice = self
            .get(COLUMN_META, META_REVERTED_BLOCK_SMT_ROOT_KEY)
            .expect("must has root");
        debug_assert_eq!(slice.len(), 32);
        let mut root = [0u8; 32];
        root.copy_from_slice(&slice);
        Ok(root)
    }

    fn get_prev_reverted_block_smt_root(&self, root: &H256) -> Result<Option<H256>> {
        match self.get(COLUMN_REVERTED_BLOCK_SMT_ROOT, root.as_slice()) {
            Some(slice) => {
                let block_hashes = packed::Byte32VecReader::from_slice_should_be_ok(slice.as_ref());
                Ok(block_hashes.iter().next().map(|h| h.unpack()))
            }
            None => Ok(None),
        }
    }

    fn get_last_valid_tip_block(&self) -> Result<packed::L2Block> {
        let block_hash = self.get_last_valid_tip_block_hash()?;
        let block = self
            .get_block(&block_hash)?
            .expect("last valid tip block exists");

        Ok(block)
    }

    fn get_last_valid_tip_block_hash(&self) -> Result<H256> {
        let slice = self
            .get(COLUMN_META, META_LAST_VALID_TIP_BLOCK_HASH_KEY)
            .expect("get last valid tip block hash");

        let byte32 = packed::Byte32Reader::from_slice_should_be_ok(slice.as_ref());
        Ok(byte32.unpack())
    }

    fn get_last_confirmed_block_number_hash(&self) -> Option<NumberHash> {
        let data = self.get(COLUMN_META, META_LAST_CONFIRMED_BLOCK_NUMBER_HASH_KEY)?;
        Some(from_box_should_be_ok!(NumberHashReader, data))
    }

    fn get_last_submitted_block_number_hash(&self) -> Option<NumberHash> {
        let data = self.get(COLUMN_META, META_LAST_SUBMITTED_BLOCK_NUMBER_HASH_KEY)?;
        Some(from_box_should_be_ok!(NumberHashReader, data))
    }

    fn get_block_status(&self, block_number: u64) -> BlockStatus {
        if Some(block_number)
            <= self
                .get_last_confirmed_block_number_hash()
                .map(|nh| nh.number().unpack())
        {
            return BlockStatus::Confirmed;
        }
        if Some(block_number)
            <= self
                .get_last_submitted_block_number_hash()
                .map(|nh| nh.number().unpack())
        {
            return BlockStatus::Submitted;
        }
        BlockStatus::Local
    }

    fn get_block_submit_tx(&self, block_number: u64) -> Option<Transaction> {
        let data = self.get(COLUMN_BLOCK_SUBMIT_TX, &block_number.to_be_bytes())?;
        Some(from_box_should_be_ok!(packed::TransactionReader, data))
    }

    fn get_block_submit_tx_hash(&self, block_number: u64) -> Option<H256> {
        let data = self.get(COLUMN_BLOCK_SUBMIT_TX_HASH, &block_number.to_be_bytes())?;
        Some(packed::Byte32Reader::from_slice_should_be_ok(data.as_ref()).unpack())
    }

    fn get_block_deposit_info_vec(&self, block_number: u64) -> Option<DepositInfoVec> {
        let data = self.get(COLUMN_BLOCK_DEPOSIT_INFO_VEC, &block_number.to_be_bytes())?;
        Some(from_box_should_be_ok!(packed::DepositInfoVecReader, data))
    }

    fn get_block_post_finalized_custodian_capacity(
        &self,
        block_number: u64,
    ) -> Option<FinalizedCustodianCapacity> {
        let data = self.get(
            COLUMN_BLOCK_POST_FINALIZED_CUSTODIAN_CAPACITY,
            &block_number.to_be_bytes(),
        )?;
        Some(from_box_should_be_ok!(
            packed::FinalizedCustodianCapacityReader,
            data
        ))
    }

    /// Get tip block hash. It may be a bad block.
    fn get_tip_block_hash(&self) -> Result<H256> {
        let slice = self
            .get(COLUMN_META, META_TIP_BLOCK_HASH_KEY)
            .expect("get tip block hash");
        Ok(packed::Byte32Reader::from_slice_should_be_ok(slice.as_ref()).unpack())
    }

    /// Get tip block. The block is NOT necessarily valid, i.e. it may be a bad
    /// block.
    fn get_tip_block(&self) -> Result<packed::L2Block> {
        let tip_block_hash = self.get_tip_block_hash()?;
        Ok(self
            .get_block(&tip_block_hash)?
            .or_else(|| self.get_bad_block(&tip_block_hash))
            .expect("get tip block"))
    }

    /// Does NOT return block hash for bad blocks.
    fn get_block_hash_by_number(&self, number: u64) -> Result<Option<H256>> {
        let block_number: packed::Uint64 = number.pack();
        match self.get(COLUMN_INDEX, block_number.as_slice()) {
            Some(slice) => Ok(Some(
                packed::Byte32Reader::from_slice_should_be_ok(slice.as_ref()).unpack(),
            )),
            None => Ok(None),
        }
    }

    /// Does NOT return block number for bad blocks.
    fn get_block_number(&self, block_hash: &H256) -> Result<Option<u64>> {
        match self.get(COLUMN_INDEX, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::Uint64Reader::from_slice_should_be_ok(slice.as_ref()).unpack(),
            )),
            None => Ok(None),
        }
    }

    /// Does NOT return bad blocks.
    fn get_block(&self, block_hash: &H256) -> Result<Option<packed::L2Block>> {
        match self.get(COLUMN_BLOCK, block_hash.as_slice()) {
            Some(slice) => Ok(Some(from_box_should_be_ok!(packed::L2BlockReader, slice))),
            None => Ok(None),
        }
    }

    fn get_bad_block(&self, block_hash: &H256) -> Option<packed::L2Block> {
        let slice = self.get(COLUMN_BAD_BLOCK, block_hash.as_slice())?;
        Some(from_box_should_be_ok!(packed::L2BlockReader, slice))
    }

    fn get_transaction(&self, tx_hash: &H256) -> Result<Option<packed::L2Transaction>> {
        match self.get_transaction_info(tx_hash)? {
            Some(tx_info) => self.get_transaction_by_key(&tx_info.key()),
            None => Ok(None),
        }
    }

    fn get_transaction_info(&self, tx_hash: &H256) -> Result<Option<packed::TransactionInfo>> {
        let tx_info_opt = self
            .get(COLUMN_TRANSACTION_INFO, tx_hash.as_slice())
            .map(|slice| from_box_should_be_ok!(packed::TransactionInfoReader, slice));
        Ok(tx_info_opt)
    }

    fn get_transaction_by_key(
        &self,
        tx_key: &TransactionKey,
    ) -> Result<Option<packed::L2Transaction>> {
        Ok(self
            .get(COLUMN_TRANSACTION, tx_key.as_slice())
            .map(|slice| from_box_should_be_ok!(packed::L2TransactionReader, slice)))
    }

    fn get_transaction_receipt(&self, tx_hash: &H256) -> Result<Option<packed::TxReceipt>> {
        if let Some(slice) = self.get(COLUMN_TRANSACTION_INFO, tx_hash.as_slice()) {
            let info = from_box_should_be_ok!(packed::TransactionInfoReader, slice);
            let tx_key = info.key();
            self.get_transaction_receipt_by_key(&tx_key)
        } else {
            Ok(None)
        }
    }

    fn get_transaction_receipt_by_key(
        &self,
        key: &TransactionKey,
    ) -> Result<Option<packed::TxReceipt>> {
        Ok(self
            .get(COLUMN_TRANSACTION_RECEIPT, key.as_slice())
            .map(|slice| from_box_should_be_ok!(packed::TxReceiptReader, slice)))
    }

    fn get_withdrawal(
        &self,
        withdrawal_hash: &H256,
    ) -> Result<Option<packed::WithdrawalRequestExtra>> {
        match self.get_withdrawal_info(withdrawal_hash)? {
            Some(withdrawal_info) => self.get_withdrawal_by_key(&withdrawal_info.key()),
            None => Ok(None),
        }
    }

    fn get_withdrawal_info(
        &self,
        withdrawal_hash: &H256,
    ) -> Result<Option<packed::WithdrawalInfo>> {
        let withdrawal_info_opt = self
            .get(COLUMN_WITHDRAWAL_INFO, withdrawal_hash.as_slice())
            .map(|slice| from_box_should_be_ok!(packed::WithdrawalInfoReader, slice));
        Ok(withdrawal_info_opt)
    }

    fn get_withdrawal_by_key(
        &self,
        withdrawal_key: &WithdrawalKey,
    ) -> Result<Option<packed::WithdrawalRequestExtra>> {
        Ok(self
            .get(COLUMN_WITHDRAWAL, withdrawal_key.as_slice())
            .map(|slice| from_box_should_be_ok!(packed::WithdrawalRequestExtraReader, slice)))
    }

    /// Works for both valid and bad blocks.
    fn get_block_post_global_state(
        &self,
        block_hash: &H256,
    ) -> Result<Option<packed::GlobalState>> {
        match self.get(COLUMN_BLOCK_GLOBAL_STATE, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                global_state_from_slice(slice.as_ref()).expect("global state should be ok"),
            )),
            None => Ok(None),
        }
    }

    fn get_bad_block_challenge_target(&self, block_hash: &H256) -> Result<Option<ChallengeTarget>> {
        match self.get(COLUMN_BAD_BLOCK_CHALLENGE_TARGET, block_hash.as_slice()) {
            Some(slice) => {
                let target = packed::ChallengeTargetReader::from_slice_should_be_ok(slice.as_ref());
                Ok(Some(target.to_entity()))
            }
            None => Ok(None),
        }
    }

    fn get_reverted_block_hashes_by_root(
        &self,
        reverted_block_smt_root: &H256,
    ) -> Result<Option<SMTRevertedBlockHashes>> {
        match self.get(
            COLUMN_REVERTED_BLOCK_SMT_ROOT,
            reverted_block_smt_root.as_slice(),
        ) {
            Some(slice) => {
                let mut block_hashes: Vec<_> =
                    from_box_should_be_ok!(packed::Byte32VecReader, slice).unpack();

                // Remove prev smt root at 0 idx
                let last_hash_idx = block_hashes.len().saturating_sub(1);
                block_hashes.swap(0, last_hash_idx);
                let prev_smt_root = block_hashes.pop().expect("prev root");

                let root_hashes = SMTRevertedBlockHashes {
                    prev_smt_root,
                    block_hashes,
                };

                Ok(Some(root_hashes))
            }
            None => Ok(None),
        }
    }

    fn get_asset_script(&self, script_hash: &H256) -> Result<Option<Script>> {
        match self.get(COLUMN_ASSET_SCRIPT, script_hash.as_slice()) {
            Some(slice) => Ok(Some(from_box_should_be_ok!(packed::ScriptReader, slice))),
            None => Ok(None),
        }
    }

    fn get_mem_pool_transaction(&self, tx_hash: &H256) -> Result<Option<packed::L2Transaction>> {
        Ok(self
            .get(COLUMN_MEM_POOL_TRANSACTION, tx_hash.as_slice())
            .map(|slice| from_box_should_be_ok!(packed::L2TransactionReader, slice)))
    }

    fn get_mem_pool_transaction_receipt(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::TxReceipt>> {
        Ok(self
            .get(COLUMN_MEM_POOL_TRANSACTION_RECEIPT, tx_hash.as_slice())
            .map(|slice| from_box_should_be_ok!(packed::TxReceiptReader, slice)))
    }

    fn get_mem_pool_withdrawal(
        &self,
        withdrawal_hash: &H256,
    ) -> Result<Option<packed::WithdrawalRequestExtra>> {
        let maybe_withdrawal =
            if let Some(slice) = self.get(COLUMN_MEM_POOL_WITHDRAWAL, withdrawal_hash.as_slice()) {
                Some(packed::WithdrawalRequestExtra::from_slice(slice.as_ref())?)
            } else {
                None
            };
        Ok(maybe_withdrawal)
    }
}

impl<T: ChainStore> ChainStore for &T {}
impl<T: ChainStore> ChainStore for &mut T {}
impl<T: ChainStore> ChainStore for &RefCell<T> {}
