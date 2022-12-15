use anyhow::{ensure, Context, Result};
use gw_store::{
    autorocks::{Direction, TransactionDb},
    migrate::{Migration, SMTTrieMigrationPlaceHolder},
    schema::{
        COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_BLOCK_SMT_BRANCH,
        COLUMN_BLOCK_SMT_LEAF, COLUMN_REVERTED_BLOCK_SMT_BRANCH, COLUMN_REVERTED_BLOCK_SMT_LEAF,
    },
};
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{h256::H256, prelude::Unpack};
use indicatif::ProgressIterator;

pub struct SMTTrieMigration;

impl Migration for SMTTrieMigration {
    fn migrate(&self, db: TransactionDb) -> Result<TransactionDb> {
        log::info!("SMTTrieMigration running");
        let mut store = Store::new(db);

        log::info!("deleting old SMT branches");
        let db = store.as_inner_mut();
        db.clear_cf(COLUMN_ACCOUNT_SMT_BRANCH)
            .context("clear COLUMN_ACCOUNT_SMT_BRANCH")?;

        // So that if we exit in the middle of this migration, the smt branches
        // columns are not empty and SMTTrieMigrationPlaceholder won't just
        // succeed.
        db.put(COLUMN_ACCOUNT_SMT_BRANCH, b"migrating", b"migrating")
            .context("put migrating")?;
        db.clear_cf(COLUMN_BLOCK_SMT_BRANCH)
            .context("clear COLUMN_BLOCK_SMT_BRANCH")?;
        db.clear_cf(COLUMN_REVERTED_BLOCK_SMT_BRANCH)
            .context("clear COLUMN_REVERTED_BLOCK_SMT_BRANCH")?;

        log::info!("migrating state smt");
        {
            let len = store
                .as_inner()
                .get_int_property(COLUMN_ACCOUNT_SMT_LEAF, "rocksdb.estimate-num-keys")
                .context("get estimate-num-keys of account smt leaves")?;
            let mut tx = store.begin_transaction_skip_concurrency_control();
            let mut state_smt = tx.state_smt().context("state_smt")?;
            for (i, (k, v)) in store
                .as_inner()
                .iter(COLUMN_ACCOUNT_SMT_LEAF, Direction::Forward)
                .enumerate()
                .progress_count(len)
            {
                state_smt
                    .update(
                        <[u8; 32]>::try_from(&k[..]).unwrap().into(),
                        <[u8; 32]>::try_from(&v[..]).unwrap().into(),
                    )
                    .context("update state_smt")?;
                // Commit periodically so that we don't use too much memory.
                if i % 128 == 0 {
                    let tx = state_smt.store_mut().inner_store_mut();
                    tx.commit()?;
                    **tx = store.begin_transaction_skip_concurrency_control();
                }
            }
            let root = *state_smt.root();
            let expected_root: H256 = tx
                .get_last_valid_tip_block()
                .context("get last valid tip block")?
                .raw()
                .post_account()
                .merkle_root()
                .unpack();
            ensure!(expected_root == H256::from(root));
            tx.commit().context("commit state_smt")?;
        }

        log::info!("migrating block smt");
        {
            let len = store
                .as_inner()
                .get_int_property(COLUMN_BLOCK_SMT_LEAF, "rocksdb.estimate-num-keys")
                .context("get estimate-num-keys of block smt leaves")?;
            let mut tx = store.begin_transaction_skip_concurrency_control();
            let mut block_smt = tx.block_smt().context("block_smt")?;
            for (i, (k, v)) in store
                .as_inner()
                .iter(COLUMN_BLOCK_SMT_LEAF, Direction::Forward)
                .enumerate()
                .progress_count(len)
            {
                block_smt
                    .update(
                        <[u8; 32]>::try_from(&k[..]).unwrap().into(),
                        <[u8; 32]>::try_from(&v[..]).unwrap().into(),
                    )
                    .context("update block_smt")?;
                // Commit periodically so that we don't use too much memory.
                if i % 128 == 0 {
                    let tx = block_smt.store_mut().inner_store_mut();
                    tx.commit()?;
                    **tx = store.begin_transaction_skip_concurrency_control();
                }
            }
            let root = *block_smt.root();
            ensure!(tx.get_block_smt_root().unwrap() == H256::from(root));
            tx.commit().context("commit block smt")?;
        }

        log::info!("migrating reverted block smt");
        {
            let mut tx = store.begin_transaction_skip_concurrency_control();
            let mut reverted_block_smt = tx.reverted_block_smt().context("reverted_block_smt")?;
            for (k, v) in store
                .as_inner()
                .iter(COLUMN_REVERTED_BLOCK_SMT_LEAF, Direction::Forward)
            {
                reverted_block_smt
                    .update(
                        <[u8; 32]>::try_from(&k[..]).unwrap().into(),
                        <[u8; 32]>::try_from(&v[..]).unwrap().into(),
                    )
                    .context("update reverted_block_smt")?;
            }
            let root = *reverted_block_smt.root();
            ensure!(tx.get_reverted_block_smt_root().unwrap() == H256::from(root));
            tx.commit().context("commit reverted_block_smt")?;
        }

        store
            .as_inner()
            .delete(COLUMN_ACCOUNT_SMT_BRANCH, b"migrating")?;

        log::info!("SMTTrieMigration completed");
        Ok(store.into_inner())
    }
    fn version(&self) -> &str {
        SMTTrieMigrationPlaceHolder.version()
    }
}
