#![allow(clippy::mutable_key_type)]

use crate::traits::kv_store::KVStoreRead;
use anyhow::Result;
use gw_common::H256;
use gw_db::error::Error;
use gw_db::schema::{
    COLUMN_ASSET_SCRIPT, COLUMN_BAD_BLOCK_CHALLENGE_TARGET, COLUMN_BLOCK,
    COLUMN_BLOCK_DEPOSIT_REQUESTS, COLUMN_BLOCK_GLOBAL_STATE, COLUMN_INDEX,
    COLUMN_L2BLOCK_COMMITTED_INFO, COLUMN_MEM_POOL_TRANSACTION,
    COLUMN_MEM_POOL_TRANSACTION_RECEIPT, COLUMN_MEM_POOL_WITHDRAWAL, COLUMN_META,
    COLUMN_REVERTED_BLOCK_SMT_ROOT, COLUMN_TRANSACTION, COLUMN_TRANSACTION_INFO,
    COLUMN_TRANSACTION_RECEIPT, COLUMN_WITHDRAWAL, COLUMN_WITHDRAWAL_INFO, META_BLOCK_SMT_ROOT_KEY,
    META_CHAIN_ID_KEY, META_LAST_VALID_TIP_BLOCK_HASH_KEY, META_REVERTED_BLOCK_SMT_ROOT_KEY,
    META_TIP_BLOCK_HASH_KEY,
};
use gw_types::offchain::global_state_from_slice;
use gw_types::packed::{Script, WithdrawalKey};
use gw_types::{
    packed::{self, ChallengeTarget, TransactionKey},
    prelude::*,
};

pub trait ChainStore: KVStoreRead {
    fn has_genesis(&self) -> Result<bool> {
        Ok(self.get_block_hash_by_number(0)?.is_some())
    }

    fn get_chain_id(&self) -> Result<H256, Error> {
        let slice = self
            .get(COLUMN_META, META_CHAIN_ID_KEY)
            .expect("must has chain_id");
        debug_assert_eq!(slice.len(), 32);
        let mut chain_id = [0u8; 32];
        chain_id.copy_from_slice(&slice);
        Ok(chain_id.into())
    }

    fn get_block_smt_root(&self) -> Result<H256, Error> {
        let slice = self
            .get(COLUMN_META, META_BLOCK_SMT_ROOT_KEY)
            .expect("must has root");
        debug_assert_eq!(slice.len(), 32);
        let mut root = [0u8; 32];
        root.copy_from_slice(&slice);
        Ok(root.into())
    }

    fn get_reverted_block_smt_root(&self) -> Result<H256, Error> {
        let slice = self
            .get(COLUMN_META, META_REVERTED_BLOCK_SMT_ROOT_KEY)
            .expect("must has root");
        debug_assert_eq!(slice.len(), 32);
        let mut root = [0u8; 32];
        root.copy_from_slice(&slice);
        Ok(root.into())
    }

    fn get_last_valid_tip_block(&self) -> Result<packed::L2Block, Error> {
        let block_hash = self.get_last_valid_tip_block_hash()?;
        let block = self
            .get_block(&block_hash)?
            .expect("last valid tip block exists");

        Ok(block)
    }

    fn get_last_valid_tip_block_hash(&self) -> Result<H256, Error> {
        let slice = self
            .get(COLUMN_META, META_LAST_VALID_TIP_BLOCK_HASH_KEY)
            .expect("get last valid tip block hash");

        let byte32 = packed::Byte32Reader::from_slice_should_be_ok(slice.as_ref()).to_entity();
        Ok(byte32.unpack())
    }

    fn get_tip_block_hash(&self) -> Result<H256, Error> {
        let slice = self
            .get(COLUMN_META, META_TIP_BLOCK_HASH_KEY)
            .expect("get tip block hash");
        Ok(
            packed::Byte32Reader::from_slice_should_be_ok(slice.as_ref())
                .to_entity()
                .unpack(),
        )
    }

    fn get_tip_block(&self) -> Result<packed::L2Block, Error> {
        let tip_block_hash = self.get_tip_block_hash()?;
        Ok(self.get_block(&tip_block_hash)?.expect("get tip block"))
    }

    fn get_block_hash_by_number(&self, number: u64) -> Result<Option<H256>, Error> {
        let block_number: packed::Uint64 = number.pack();
        match self.get(COLUMN_INDEX, block_number.as_slice()) {
            Some(slice) => Ok(Some(
                packed::Byte32Reader::from_slice_should_be_ok(slice.as_ref())
                    .to_entity()
                    .unpack(),
            )),
            None => Ok(None),
        }
    }

    fn get_block_number(&self, block_hash: &H256) -> Result<Option<u64>, Error> {
        match self.get(COLUMN_INDEX, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::Uint64Reader::from_slice_should_be_ok(slice.as_ref())
                    .to_entity()
                    .unpack(),
            )),
            None => Ok(None),
        }
    }

    fn get_block(&self, block_hash: &H256) -> Result<Option<packed::L2Block>, Error> {
        match self.get(COLUMN_BLOCK, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::L2BlockReader::from_slice_should_be_ok(slice.as_ref()).to_entity(),
            )),
            None => Ok(None),
        }
    }

    fn get_transaction(&self, tx_hash: &H256) -> Result<Option<packed::L2Transaction>, Error> {
        match self.get_transaction_info(tx_hash)? {
            Some(tx_info) => self.get_transaction_by_key(&tx_info.key()),
            None => Ok(None),
        }
    }

    fn get_transaction_info(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::TransactionInfo>, Error> {
        let tx_info_opt = self
            .get(COLUMN_TRANSACTION_INFO, tx_hash.as_slice())
            .map(|slice| {
                packed::TransactionInfoReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            });
        Ok(tx_info_opt)
    }

    fn get_transaction_by_key(
        &self,
        tx_key: &TransactionKey,
    ) -> Result<Option<packed::L2Transaction>, Error> {
        Ok(self
            .get(COLUMN_TRANSACTION, tx_key.as_slice())
            .map(|slice| {
                packed::L2TransactionReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    fn get_transaction_receipt(&self, tx_hash: &H256) -> Result<Option<packed::TxReceipt>, Error> {
        if let Some(slice) = self.get(COLUMN_TRANSACTION_INFO, tx_hash.as_slice()) {
            let info =
                packed::TransactionInfoReader::from_slice_should_be_ok(slice.as_ref()).to_entity();
            let tx_key = info.key();
            self.get_transaction_receipt_by_key(&tx_key)
        } else {
            Ok(None)
        }
    }

    fn get_transaction_receipt_by_key(
        &self,
        key: &TransactionKey,
    ) -> Result<Option<packed::TxReceipt>, Error> {
        Ok(self
            .get(COLUMN_TRANSACTION_RECEIPT, key.as_slice())
            .map(|slice| {
                packed::TxReceiptReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    fn get_withdrawal(
        &self,
        withdrawal_hash: &H256,
    ) -> Result<Option<packed::WithdrawalRequest>, Error> {
        match self.get_withdrawal_info(withdrawal_hash)? {
            Some(withdrawal_info) => self.get_withdrawal_by_key(&withdrawal_info.key()),
            None => Ok(None),
        }
    }

    fn get_withdrawal_info(
        &self,
        withdrawal_hash: &H256,
    ) -> Result<Option<packed::WithdrawalInfo>, Error> {
        let withdrawal_info_opt = self
            .get(COLUMN_WITHDRAWAL_INFO, withdrawal_hash.as_slice())
            .map(|slice| {
                packed::WithdrawalInfoReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            });
        Ok(withdrawal_info_opt)
    }

    fn get_withdrawal_by_key(
        &self,
        withdrawal_key: &WithdrawalKey,
    ) -> Result<Option<packed::WithdrawalRequest>, Error> {
        Ok(self
            .get(COLUMN_WITHDRAWAL, withdrawal_key.as_slice())
            .map(|slice| {
                packed::WithdrawalRequestReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    fn get_l2block_committed_info(
        &self,
        block_hash: &H256,
    ) -> Result<Option<packed::L2BlockCommittedInfo>, Error> {
        match self.get(COLUMN_L2BLOCK_COMMITTED_INFO, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::L2BlockCommittedInfoReader::from_slice_should_be_ok(slice.as_ref())
                    .to_entity(),
            )),
            None => Ok(None),
        }
    }

    fn get_block_deposit_requests(
        &self,
        block_hash: &H256,
    ) -> Result<Option<Vec<packed::DepositRequest>>, Error> {
        match self.get(COLUMN_BLOCK_DEPOSIT_REQUESTS, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::DepositRequestVecReader::from_slice_should_be_ok(slice.as_ref())
                    .to_entity()
                    .into_iter()
                    .collect(),
            )),
            None => Ok(None),
        }
    }

    fn get_block_post_global_state(
        &self,
        block_hash: &H256,
    ) -> Result<Option<packed::GlobalState>, Error> {
        match self.get(COLUMN_BLOCK_GLOBAL_STATE, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                global_state_from_slice(slice.as_ref()).expect("global state should be ok"),
            )),
            None => Ok(None),
        }
    }

    fn get_bad_block_challenge_target(
        &self,
        block_hash: &H256,
    ) -> Result<Option<ChallengeTarget>, Error> {
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
    ) -> Result<Option<Vec<H256>>, Error> {
        match self.get(
            COLUMN_REVERTED_BLOCK_SMT_ROOT,
            reverted_block_smt_root.as_slice(),
        ) {
            Some(slice) => {
                let block_hash = packed::Byte32VecReader::from_slice_should_be_ok(slice.as_ref());
                Ok(Some(block_hash.to_entity().unpack()))
            }
            None => Ok(None),
        }
    }

    fn get_asset_script(&self, script_hash: &H256) -> Result<Option<Script>, Error> {
        match self.get(COLUMN_ASSET_SCRIPT, script_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::ScriptReader::from_slice_should_be_ok(slice.as_ref()).to_entity(),
            )),
            None => Ok(None),
        }
    }

    fn get_mem_pool_transaction(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::L2Transaction>, Error> {
        Ok(self
            .get(COLUMN_MEM_POOL_TRANSACTION, tx_hash.as_slice())
            .map(|slice| {
                packed::L2TransactionReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    fn get_mem_pool_transaction_receipt(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::TxReceipt>, Error> {
        Ok(self
            .get(COLUMN_MEM_POOL_TRANSACTION_RECEIPT, tx_hash.as_slice())
            .map(|slice| {
                packed::TxReceiptReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    fn get_mem_pool_withdrawal(
        &self,
        withdrawal_hash: &H256,
    ) -> Result<Option<packed::WithdrawalRequest>, Error> {
        Ok(self
            .get(COLUMN_MEM_POOL_WITHDRAWAL, withdrawal_hash.as_slice())
            .map(|slice| {
                packed::WithdrawalRequestReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }
}
