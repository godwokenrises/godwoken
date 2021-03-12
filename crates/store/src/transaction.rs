use crate::{db_utils::build_transaction_key, smt_store_impl::SMTStore, traits::KVStore};
use gw_common::{smt::SMT, CKB_SUDT_SCRIPT_ARGS, H256};
use gw_db::schema::{
    Col, COLUMN_BLOCK, COLUMN_BLOCK_DEPOSITION_REQUESTS, COLUMN_BLOCK_GLOBAL_STATE,
    COLUMN_BLOCK_SMT_BRANCH, COLUMN_BLOCK_SMT_LEAF, COLUMN_CUSTODIAN_ASSETS, COLUMN_INDEX,
    COLUMN_META, COLUMN_SYNC_BLOCK_HEADER_INFO, COLUMN_TRANSACTION, COLUMN_TRANSACTION_INFO,
    COLUMN_TRANSACTION_RECEIPT, META_ACCOUNT_SMT_COUNT_KEY, META_ACCOUNT_SMT_ROOT_KEY,
    META_BLOCK_SMT_ROOT_KEY, META_CHAIN_ID_KEY, META_TIP_BLOCK_HASH_KEY,
};
use gw_db::{error::Error, iter::DBIter, DBIterator, IteratorMode, RocksDBTransaction};
use gw_types::{packed, prelude::*};
use std::{borrow::BorrowMut, collections::HashMap, rc::Rc};

#[derive(Clone)]
pub struct StoreTransaction {
    pub(crate) inner: Rc<RocksDBTransaction>,
}

impl KVStore for StoreTransaction {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        self.inner.get(col, key)
        .expect("db operation should be ok")
        .map(|v| { Box::<[u8]>::from(v.as_ref()) })
    }

    fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
        self.inner
            .iter(col, mode)
            .expect("db operation should be ok")
    }

    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error> {
        self.inner.put(col, key, value)
    }

    fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error> {
        self.inner.delete(col, key)
    }
}

impl StoreTransaction {
    pub fn commit(&self) -> Result<(), Error> {
        self.inner.commit()
    }

    pub fn setup_chain_id(&self, chain_id: H256) -> Result<(), Error> {
        self.insert_raw(COLUMN_META, META_CHAIN_ID_KEY, chain_id.as_slice())?;
        Ok(())
    }

    pub fn get_block_smt_root(&self) -> Result<H256, Error> {
        let slice = self
            .get(COLUMN_META, META_BLOCK_SMT_ROOT_KEY)
            .expect("must has root");
        debug_assert_eq!(slice.len(), 32);
        let mut root = [0u8; 32];
        root.copy_from_slice(&slice);
        Ok(root.into())
    }

    pub fn set_block_smt_root(&self, root: H256) -> Result<(), Error> {
        self.insert_raw(COLUMN_META, META_BLOCK_SMT_ROOT_KEY, root.as_slice())?;
        Ok(())
    }

    pub fn block_smt<'a>(&'a self) -> Result<SMT<SMTStore<'a, Self>>, Error> {
        let root = self.get_block_smt_root()?;
        let smt_store = SMTStore::new(COLUMN_BLOCK_SMT_LEAF, COLUMN_BLOCK_SMT_BRANCH, self);
        Ok(SMT::new(root, smt_store))
    }

    pub fn get_account_smt_root(&self) -> Result<H256, Error> {
        let slice = self
            .get(COLUMN_META, META_ACCOUNT_SMT_ROOT_KEY)
            .expect("must has root");
        debug_assert_eq!(slice.len(), 32);
        let mut root = [0u8; 32];
        root.copy_from_slice(&slice);
        Ok(root.into())
    }

    pub fn set_account_smt_root(&self, root: H256) -> Result<(), Error> {
        self.insert_raw(COLUMN_META, META_ACCOUNT_SMT_ROOT_KEY, root.as_slice())?;
        Ok(())
    }

    pub fn set_account_count(&self, count: u32) -> Result<(), Error> {
        let count: packed::Uint32 = count.pack();
        self.insert_raw(COLUMN_META, META_ACCOUNT_SMT_COUNT_KEY, count.as_slice())
            .expect("insert");
        Ok(())
    }

    pub fn get_account_count(&self) -> Result<u32, Error> {
        let slice = self
            .get(COLUMN_META, META_ACCOUNT_SMT_COUNT_KEY)
            .expect("account count");
        let count = packed::Uint32Reader::from_slice_should_be_ok(&slice.as_ref()).to_entity();
        Ok(count.unpack())
    }

    pub fn get_tip_block_hash(&self) -> Result<H256, Error> {
        let slice = self
            .get(COLUMN_META, META_TIP_BLOCK_HASH_KEY)
            .expect("get tip block hash");
        Ok(
            packed::Byte32Reader::from_slice_should_be_ok(&slice.as_ref())
                .to_entity()
                .unpack(),
        )
    }

    pub fn get_tip_block(&self) -> Result<packed::L2Block, Error> {
        let tip_block_hash = self.get_tip_block_hash()?;
        Ok(self.get_block(&tip_block_hash)?.expect("get tip block"))
    }

    pub fn get_block_hash_by_number(&self, number: u64) -> Result<Option<H256>, Error> {
        let block_number: packed::Uint64 = number.pack();
        match self.get(COLUMN_INDEX, block_number.as_slice()) {
            Some(slice) => Ok(Some(
                packed::Byte32Reader::from_slice_should_be_ok(&slice.as_ref())
                    .to_entity()
                    .unpack(),
            )),
            None => Ok(None),
        }
    }

    pub fn get_block_number(&self, block_hash: &H256) -> Result<Option<u64>, Error> {
        match self.get(COLUMN_INDEX, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::Uint64Reader::from_slice_should_be_ok(&slice.as_ref())
                    .to_entity()
                    .unpack(),
            )),
            None => Ok(None),
        }
    }

    pub fn get_block(&self, block_hash: &H256) -> Result<Option<packed::L2Block>, Error> {
        match self.get(COLUMN_BLOCK, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::L2BlockReader::from_slice_should_be_ok(&slice.as_ref()).to_entity(),
            )),
            None => Ok(None),
        }
    }

    pub fn get_transaction(&self, tx_hash: &H256) -> Result<Option<packed::L2Transaction>, Error> {
        if let Some(slice) = self.get(COLUMN_TRANSACTION_INFO, tx_hash.as_slice()) {
            let info =
                packed::TransactionInfoReader::from_slice_should_be_ok(&slice.as_ref()).to_entity();
            let tx_key = info.key();
            let mut block_hash_bytes = [0u8; 32];
            let mut index_bytes = [0u8; 4];
            block_hash_bytes.copy_from_slice(&tx_key.as_slice()[..32]);
            index_bytes.copy_from_slice(&tx_key.as_slice()[32..36]);
            let block_hash = H256::from(block_hash_bytes);
            let index = u32::from_le_bytes(index_bytes);
            if let Some(block) = self.get_block(&block_hash)? {
                Ok(block.transactions().get(index as usize))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    pub fn get_block_synced_header_info(
        &self,
        block_hash: &H256,
    ) -> Result<Option<packed::HeaderInfo>, Error> {
        match self.get(COLUMN_SYNC_BLOCK_HEADER_INFO, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::HeaderInfoReader::from_slice_should_be_ok(&slice.as_ref()).to_entity(),
            )),
            None => Ok(None),
        }
    }

    pub fn get_block_deposition_requests(
        &self,
        block_hash: &H256,
    ) -> Result<Option<Vec<packed::DepositionRequest>>, Error> {
        match self.get(COLUMN_BLOCK_DEPOSITION_REQUESTS, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::DepositionRequestVecReader::from_slice_should_be_ok(&slice.as_ref())
                    .to_entity()
                    .into_iter()
                    .collect(),
            )),
            None => Ok(None),
        }
    }

    pub fn get_block_post_global_state(
        &self,
        block_hash: &H256,
    ) -> Result<Option<packed::GlobalState>, Error> {
        match self.get(COLUMN_BLOCK_GLOBAL_STATE, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::GlobalStateReader::from_slice_should_be_ok(&slice.as_ref()).to_entity(),
            )),
            None => Ok(None),
        }
    }

    /// key: sudt_script_hash
    fn set_custodian_asset(&self, key: H256, value: u128) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_CUSTODIAN_ASSETS,
            key.as_slice(),
            &value.to_le_bytes(),
        )
    }

    /// key: sudt_script_hash
    pub fn get_custodian_asset(&self, key: H256) -> Result<u128, Error> {
        match self.get(COLUMN_CUSTODIAN_ASSETS, key.as_slice()) {
            Some(slice) => {
                let mut buf = [0u8; 16];
                buf.copy_from_slice(&slice);
                Ok(u128::from_le_bytes(buf))
            }
            None => Ok(0),
        }
    }

    pub fn insert_block(
        &self,
        block: packed::L2Block,
        header_info: packed::HeaderInfo,
        global_state: packed::GlobalState,
        tx_receipts: Vec<packed::TxReceipt>,
        deposition_requests: Vec<packed::DepositionRequest>,
    ) -> Result<(), Error> {
        debug_assert_eq!(block.transactions().len(), tx_receipts.len());
        let block_hash = block.hash();
        self.insert_raw(COLUMN_BLOCK, &block_hash, block.as_slice())?;
        self.insert_raw(
            COLUMN_SYNC_BLOCK_HEADER_INFO,
            &block_hash,
            header_info.as_slice(),
        )?;
        self.insert_raw(
            COLUMN_BLOCK_GLOBAL_STATE,
            &block_hash,
            global_state.as_slice(),
        )?;
        let deposition_requests_vec: packed::DepositionRequestVec = deposition_requests.pack();
        self.insert_raw(
            COLUMN_BLOCK_DEPOSITION_REQUESTS,
            &block_hash,
            deposition_requests_vec.as_slice(),
        )?;

        for (index, (tx, tx_receipt)) in block
            .transactions()
            .into_iter()
            .zip(tx_receipts)
            .enumerate()
        {
            let key = build_transaction_key(tx.hash().pack(), index as u32);
            self.insert_raw(COLUMN_TRANSACTION, &key, tx.as_slice())?;
            self.insert_raw(COLUMN_TRANSACTION_RECEIPT, &key, tx_receipt.as_slice())?;
        }
        Ok(())
    }

    /// Update custodian assets
    fn update_custodian_assets<
        AddIter: Iterator<Item = CustodianChange>,
        RemIter: Iterator<Item = CustodianChange>,
    >(
        &self,
        addition: AddIter,
        removed: RemIter,
    ) -> Result<(), Error> {
        let mut touched_custodian_assets: HashMap<H256, u128> = Default::default();
        for request in addition {
            let CustodianChange {
                sudt_script_hash,
                amount,
                capacity,
            } = request;

            // update ckb balance
            let ckb_balance = touched_custodian_assets
                .entry(CKB_SUDT_SCRIPT_ARGS.into())
                .or_insert_with(|| {
                    self.get_custodian_asset(CKB_SUDT_SCRIPT_ARGS.into())
                        .expect("get custodian asset")
                })
                .borrow_mut();
            *ckb_balance = ckb_balance
                .checked_add(capacity as u128)
                .expect("deposit overflow");

            // update sUDT balance
            let balance = touched_custodian_assets
                .entry(sudt_script_hash)
                .or_insert_with(|| {
                    self.get_custodian_asset(sudt_script_hash)
                        .expect("get custodian asset")
                })
                .borrow_mut();
            *balance = balance.checked_add(amount).expect("deposit overflow");
        }
        for request in removed {
            let CustodianChange {
                sudt_script_hash,
                amount,
                capacity,
            } = request;

            // update ckb balance
            let ckb_balance = touched_custodian_assets
                .entry(CKB_SUDT_SCRIPT_ARGS.into())
                .or_insert_with(|| {
                    self.get_custodian_asset(CKB_SUDT_SCRIPT_ARGS.into())
                        .expect("get custodian asset")
                })
                .borrow_mut();

            *ckb_balance = ckb_balance
                .checked_sub(capacity as u128)
                .expect("withdrawal overflow");

            // update sUDT balance
            let balance = touched_custodian_assets
                .entry(sudt_script_hash)
                .or_insert_with(|| {
                    self.get_custodian_asset(sudt_script_hash)
                        .expect("get custodian asset")
                })
                .borrow_mut();
            *balance = balance.checked_sub(amount).expect("withdrawal overflow");
        }
        // write touched assets to storage
        for (key, balance) in touched_custodian_assets {
            self.set_custodian_asset(key, balance)?;
        }
        Ok(())
    }

    /// Attach block to the rollup main chain
    pub fn attach_block(&self, block: packed::L2Block) -> Result<(), Error> {
        let raw = block.raw();
        let raw_number = raw.number();
        let block_hash = raw.hash();

        // build tx info
        for (index, tx) in block.transactions().into_iter().enumerate() {
            let key = build_transaction_key(block_hash.pack(), index as u32);
            let info = packed::TransactionInfo::new_builder()
                .key(key.pack())
                .block_number(raw_number.clone())
                .build();
            let tx_hash = tx.hash();
            self.insert_raw(COLUMN_TRANSACTION_INFO, &tx_hash, info.as_slice())?;
        }

        // update custodian assets
        let deposit_assets = self
            .get_block_deposition_requests(&block_hash.into())?
            .expect("deposits")
            .into_iter()
            .map(|deposit| CustodianChange {
                sudt_script_hash: deposit.sudt_script_hash().unpack(),
                amount: deposit.amount().unpack(),
                capacity: deposit.capacity().unpack(),
            });
        let withdrawal_assets = block.withdrawals().into_iter().map(|withdrawal| {
            let raw = withdrawal.raw();
            CustodianChange {
                sudt_script_hash: raw.sudt_script_hash().unpack(),
                amount: raw.amount().unpack(),
                capacity: raw.capacity().unpack(),
            }
        });
        self.update_custodian_assets(deposit_assets, withdrawal_assets)?;

        // build main chain index
        self.insert_raw(COLUMN_INDEX, raw_number.as_slice(), &block_hash)?;
        self.insert_raw(COLUMN_INDEX, &block_hash, raw_number.as_slice())?;

        // update block tree
        let mut block_smt = self.block_smt()?;
        block_smt
            .update(raw.smt_key().into(), raw.hash().into())
            .map_err(|err| Error::from(format!("SMT error {}", err)))?;
        let root = block_smt.root();
        self.set_block_smt_root(*root)?;
        // update tip
        self.insert_raw(COLUMN_META, &META_TIP_BLOCK_HASH_KEY, &block_hash)?;
        Ok(())
    }

    pub fn detach_block(&self, block: &packed::L2Block) -> Result<(), Error> {
        // remove transaction info
        for tx in block.transactions().into_iter() {
            let tx_hash = tx.hash();
            self.delete(COLUMN_TRANSACTION_INFO, &tx_hash)?;
        }

        // update custodian assets
        let deposit_assets = self
            .get_block_deposition_requests(&block.hash().into())?
            .expect("deposits")
            .into_iter()
            .map(|deposit| CustodianChange {
                sudt_script_hash: deposit.sudt_script_hash().unpack(),
                amount: deposit.amount().unpack(),
                capacity: deposit.capacity().unpack(),
            });
        let withdrawal_assets = block.withdrawals().into_iter().map(|withdrawal| {
            let raw = withdrawal.raw();
            CustodianChange {
                sudt_script_hash: raw.sudt_script_hash().unpack(),
                amount: raw.amount().unpack(),
                capacity: raw.capacity().unpack(),
            }
        });
        self.update_custodian_assets(withdrawal_assets, deposit_assets)?;

        let block_number = block.raw().number();
        self.delete(COLUMN_INDEX, block_number.as_slice())?;
        self.delete(COLUMN_INDEX, &block.hash())?;

        // update block tree
        let mut block_smt = self.block_smt()?;
        block_smt
            .update(block.smt_key().into(), H256::zero())
            .map_err(|err| Error::from(format!("SMT error {}", err)))?;
        let root = block_smt.root();
        self.set_block_smt_root(*root)?;

        // update tip
        let block_number: u64 = block_number.unpack();
        let parent_number = block_number.saturating_sub(1);
        let parent_block_hash = self
            .get_block_hash_by_number(parent_number)?
            .expect("parent block hash");
        self.insert_raw(
            COLUMN_META,
            &META_TIP_BLOCK_HASH_KEY,
            parent_block_hash.as_slice(),
        )?;
        Ok(())
    }
}

struct CustodianChange {
    capacity: u64,
    sudt_script_hash: H256,
    amount: u128,
}
