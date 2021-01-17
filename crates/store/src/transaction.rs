use crate::db_utils::build_transaction_key;
use crate::CodeStore;
use gw_common::{
    error::Error as StateError,
    smt::SMT,
    sparse_merkle_tree::{
        error::Error as SMTError,
        traits::Store as SMTStore,
        tree::{BranchNode, LeafNode},
    },
    state::State,
    H256,
};
use gw_db::schema::{
    Col, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_BLOCK,
    COLUMN_BLOCK_DEPOSITION_REQUESTS, COLUMN_BLOCK_GLOBAL_STATE, COLUMN_BLOCK_SMT_BRANCH,
    COLUMN_BLOCK_SMT_LEAF, COLUMN_DATA, COLUMN_INDEX, COLUMN_META, COLUMN_SCRIPT,
    COLUMN_SYNC_BLOCK_HEADER_INFO, COLUMN_TRANSACTION, COLUMN_TRANSACTION_INFO,
    COLUMN_TRANSACTION_RECEIPT, META_ACCOUNT_SMT_COUNT_KEY, META_ACCOUNT_SMT_ROOT_KEY,
    META_BLOCK_SMT_ROOT_KEY, META_CHAIN_ID_KEY, META_TIP_BLOCK_HASH_KEY,
};
use gw_db::{
    error::Error, iter::DBIter, DBIterator, DBVector, IteratorMode, RocksDBTransaction,
    RocksDBTransactionSnapshot,
};
use gw_types::{bytes::Bytes, packed, prelude::*};

pub struct SMTStoreTransaction<'a> {
    leaf_col: Col,
    branch_col: Col,
    store: &'a StoreTransaction,
}

impl<'a> SMTStoreTransaction<'a> {
    pub fn new(leaf_col: Col, branch_col: Col, store: &'a StoreTransaction) -> Self {
        SMTStoreTransaction {
            leaf_col,
            branch_col,
            store,
        }
    }
}

impl<'a> SMTStore<H256> for SMTStoreTransaction<'a> {
    fn get_branch(&self, node: &H256) -> Result<Option<BranchNode>, SMTError> {
        match self.store.get(self.branch_col, node.as_slice()) {
            Some(slice) => {
                let branch = packed::SMTBranchNodeReader::from_slice_should_be_ok(&slice.as_ref())
                    .to_entity();
                Ok(Some(branch.unpack()))
            }
            None => Ok(None),
        }
    }
    fn get_leaf(&self, leaf_hash: &H256) -> Result<Option<LeafNode<H256>>, SMTError> {
        match self.store.get(self.leaf_col, leaf_hash.as_slice()) {
            Some(slice) => {
                let leaf =
                    packed::SMTLeafNodeReader::from_slice_should_be_ok(&slice.as_ref()).to_entity();
                Ok(Some(leaf.unpack()))
            }
            None => Ok(None),
        }
    }
    fn insert_branch(&mut self, node: H256, branch: BranchNode) -> Result<(), SMTError> {
        let branch: packed::SMTBranchNode = branch.pack();
        self.store
            .insert_raw(self.branch_col, node.as_slice(), branch.as_slice())
            .map_err(|err| SMTError::Store(format!("Insert error {}", err)))?;
        Ok(())
    }
    fn insert_leaf(&mut self, leaf_hash: H256, leaf: LeafNode<H256>) -> Result<(), SMTError> {
        let leaf: packed::SMTLeafNode = leaf.pack();
        self.store
            .insert_raw(self.leaf_col, leaf_hash.as_slice(), leaf.as_slice())
            .map_err(|err| SMTError::Store(format!("Insert error {}", err)))?;
        Ok(())
    }
    fn remove_branch(&mut self, node: &H256) -> Result<(), SMTError> {
        self.store
            .delete(self.branch_col, node.as_slice())
            .map_err(|err| SMTError::Store(format!("Delete error {}", err)))?;
        Ok(())
    }
    fn remove_leaf(&mut self, leaf_hash: &H256) -> Result<(), SMTError> {
        self.store
            .delete(self.leaf_col, leaf_hash.as_slice())
            .map_err(|err| SMTError::Store(format!("Delete error {}", err)))?;
        Ok(())
    }
}

pub struct StoreTransactionSnapshot<'a> {
    pub(crate) inner: RocksDBTransactionSnapshot<'a>,
}

pub struct StoreTransaction {
    pub(crate) inner: RocksDBTransaction,
}

impl StoreTransaction {
    fn get(&self, col: Col, key: &[u8]) -> Option<DBVector> {
        self.inner.get(col, key).expect("db operation should be ok")
    }

    fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
        self.inner
            .iter(col, mode)
            .expect("db operation should be ok")
    }

    pub fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error> {
        self.inner.put(col, key, value)
    }

    pub fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error> {
        self.inner.delete(col, key)
    }

    pub fn commit(&self) -> Result<(), Error> {
        self.inner.commit()
    }

    pub fn get_snapshot(&self) -> StoreTransactionSnapshot<'_> {
        StoreTransactionSnapshot {
            inner: self.inner.get_snapshot(),
        }
    }

    pub fn get_update_for_tip_hash(
        &self,
        snapshot: &StoreTransactionSnapshot<'_>,
    ) -> Option<packed::Byte32> {
        self.inner
            .get_for_update(COLUMN_META, META_TIP_BLOCK_HASH_KEY, &snapshot.inner)
            .expect("db operation should be ok")
            .map(|slice| packed::Byte32Reader::from_slice_should_be_ok(&slice.as_ref()).to_entity())
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

    pub fn block_smt<'a>(&'a self) -> Result<SMT<SMTStoreTransaction<'a>>, Error> {
        let root = self.get_block_smt_root()?;
        let smt_store =
            SMTStoreTransaction::new(COLUMN_BLOCK_SMT_LEAF, COLUMN_BLOCK_SMT_BRANCH, self);
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

    fn set_account_count(&self, count: u32) -> Result<(), Error> {
        let count: packed::Uint32 = count.pack();
        self.insert_raw(COLUMN_META, META_ACCOUNT_SMT_COUNT_KEY, count.as_slice())
            .expect("insert");
        Ok(())
    }

    pub fn account_smt_store<'a>(&'a self) -> Result<SMTStoreTransaction<'a>, Error> {
        let smt_store =
            SMTStoreTransaction::new(COLUMN_ACCOUNT_SMT_LEAF, COLUMN_ACCOUNT_SMT_BRANCH, self);
        Ok(smt_store)
    }

    pub fn account_smt<'a>(&'a self) -> Result<SMT<SMTStoreTransaction<'a>>, Error> {
        let root = self.get_account_smt_root()?;
        let smt_store = self.account_smt_store()?;
        Ok(SMT::new(root, smt_store))
    }

    pub fn account_state_tree<'a>(&'a self) -> Result<StateTree<'a>, Error> {
        Ok(StateTree {
            tree: self.account_smt()?,
            db: self,
        })
    }

    /// clear account state tree, delete leaves and branches from DB
    pub fn clear_account_state_tree(&self) -> Result<(), Error> {
        self.set_account_smt_root(H256::zero())?;
        self.set_account_count(0)?;
        for col in &[COLUMN_ACCOUNT_SMT_LEAF, COLUMN_ACCOUNT_SMT_BRANCH] {
            for (k, _v) in self.get_iter(col, IteratorMode::Start) {
                self.delete(col, k.as_ref())?;
            }
        }
        Ok(())
    }

    pub fn get_block(&self, block_hash: &H256) -> Result<Option<packed::L2Block>, Error> {
        match self.get(COLUMN_BLOCK, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::L2BlockReader::from_slice_should_be_ok(&slice.as_ref()).to_entity(),
            )),
            None => Ok(None),
        }
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

    pub fn detach_block(&self, block: &packed::L2Block) -> Result<(), Error> {
        for tx in block.transactions().into_iter() {
            let tx_hash = tx.hash();
            self.delete(COLUMN_TRANSACTION_INFO, &tx_hash)?;
        }
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

pub struct StateTree<'a> {
    tree: SMT<SMTStoreTransaction<'a>>,
    db: &'a StoreTransaction,
}
impl<'a> State for StateTree<'a> {
    fn get_raw(&self, key: &H256) -> Result<H256, StateError> {
        let v = self.tree.get(&(*key).into())?;
        Ok(v.into())
    }
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), StateError> {
        self.tree.update(key.into(), value.into())?;
        self.db
            .set_account_smt_root(*self.tree.root())
            .expect("set smt root");
        Ok(())
    }
    fn get_account_count(&self) -> Result<u32, StateError> {
        let slice = self
            .db
            .get(COLUMN_META, META_ACCOUNT_SMT_COUNT_KEY)
            .expect("account count");
        let count = packed::Uint32Reader::from_slice_should_be_ok(&slice.as_ref()).to_entity();
        Ok(count.unpack())
    }
    fn set_account_count(&mut self, count: u32) -> Result<(), StateError> {
        Ok(self.db.set_account_count(count).expect("set account count"))
    }
    fn calculate_root(&self) -> Result<H256, StateError> {
        let root = self.tree.root();
        Ok(*root)
    }
}

impl<'a> CodeStore for StateTree<'a> {
    fn insert_script(&mut self, script_hash: H256, script: packed::Script) {
        self.db
            .insert_raw(COLUMN_SCRIPT, script_hash.as_slice(), script.as_slice())
            .expect("insert script");
    }
    fn get_script(&self, script_hash: &H256) -> Option<packed::Script> {
        match self.db.get(COLUMN_SCRIPT, script_hash.as_slice()) {
            Some(slice) => {
                Some(packed::ScriptReader::from_slice_should_be_ok(&slice.as_ref()).to_entity())
            }
            None => None,
        }
    }
    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.db
            .insert_raw(COLUMN_DATA, data_hash.as_slice(), &code)
            .expect("insert data");
    }
    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        match self.db.get(COLUMN_DATA, data_hash.as_slice()) {
            Some(slice) => Some(Bytes::from(slice.to_vec())),
            None => None,
        }
    }
}
