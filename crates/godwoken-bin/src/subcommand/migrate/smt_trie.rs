use anyhow::{ensure, Context, Result};
use gw_db::{
    migrate::{Migration, SMTTrieMigrationPlaceHolder},
    schema::{
        COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_BLOCK_SMT_BRANCH,
        COLUMN_BLOCK_SMT_LEAF, COLUMN_REVERTED_BLOCK_SMT_BRANCH, COLUMN_REVERTED_BLOCK_SMT_LEAF,
    },
    DBIterator, IteratorMode, RocksDB,
};
use gw_store::{
    traits::{chain_store::ChainStore, kv_store::KVStoreWrite},
    Store,
};

pub struct SMTTrieMigration;

impl Migration for SMTTrieMigration {
    fn migrate(&self, db: RocksDB) -> Result<RocksDB> {
        log::info!("SMTTrieMigration running");
        let store = Store::new(db);

        // Get state smt root before migration.
        let old_state_smt_root = {
            let tx = &store.begin_transaction();
            let state_smt = tx.state_smt().context("state_smt")?;
            *state_smt.root()
        };

        log::info!("deleting old SMT branches");
        {
            let mut wb = store.as_inner().new_write_batch();

            wb.delete_range(COLUMN_ACCOUNT_SMT_BRANCH, &[], &[255; 64])
                .context("delete account smt branches")?;
            // So that if we exit in the middle of this migration, the smt branches
            // columns are not empty and SMTTrieMigrationPlaceholder won't just succeed.
            wb.put(COLUMN_ACCOUNT_SMT_BRANCH, b"migrating", b"migrating")
                .context("put migrating")?;
            wb.delete_range(COLUMN_BLOCK_SMT_BRANCH, &[], &[255; 64])
                .context("delete block smt branches")?;
            wb.delete_range(COLUMN_REVERTED_BLOCK_SMT_BRANCH, &[], &[255; 64])
                .context("delete reverted block smt branches")?;

            store.as_inner().write(&wb)?;
        }

        log::info!("migrating state smt");
        {
            let tx = store.begin_transaction();
            let mut state_smt = tx.state_smt().context("state_smt")?;
            // XXX: memory usage of long running transaction.
            for (k, v) in store
                .as_inner()
                .iter(COLUMN_ACCOUNT_SMT_LEAF, IteratorMode::Start)
                .context("iter state smt leaves")?
            {
                state_smt
                    .update(
                        <[u8; 32]>::try_from(&k[..]).unwrap().into(),
                        <[u8; 32]>::try_from(&v[..]).unwrap().into(),
                    )
                    .context("update state_smt")?;
            }
            ensure!(old_state_smt_root == *state_smt.root());
            tx.commit().context("commit state_smt")?;
        }

        log::info!("migrating block smt");
        {
            let tx = &store.begin_transaction();
            let mut block_smt = tx.block_smt().context("block_smt")?;
            for (k, v) in store
                .as_inner()
                .iter(COLUMN_BLOCK_SMT_LEAF, IteratorMode::Start)
                .context("iter block smt leaves")?
            {
                block_smt
                    .update(
                        <[u8; 32]>::try_from(&k[..]).unwrap().into(),
                        <[u8; 32]>::try_from(&v[..]).unwrap().into(),
                    )
                    .context("update block_smt")?;
            }
            ensure!(tx.get_block_smt_root().unwrap() == *block_smt.root());
            tx.commit().context("commit block smt")?;
        }

        log::info!("migrating reverted block smt");
        {
            let tx = &store.begin_transaction();
            let mut reverted_block_smt = tx.reverted_block_smt().context("reverted_block_smt")?;
            for (k, v) in store
                .as_inner()
                .iter(COLUMN_REVERTED_BLOCK_SMT_LEAF, IteratorMode::Start)
                .context("iter reverted_block_smt leaves")?
            {
                reverted_block_smt
                    .update(
                        <[u8; 32]>::try_from(&k[..]).unwrap().into(),
                        <[u8; 32]>::try_from(&v[..]).unwrap().into(),
                    )
                    .context("update reverted_block_smt")?;
            }
            ensure!(tx.get_reverted_block_smt_root().unwrap() == *reverted_block_smt.root());
            tx.commit().context("commit reverted_block_smt")?;
        }

        {
            let tx = &store.begin_transaction();
            tx.delete(COLUMN_ACCOUNT_SMT_BRANCH, b"migrating")?;
            tx.commit()?;
        }

        log::info!("SMTTrieMigration completed");
        Ok(store.into_inner())
    }
    fn version(&self) -> &str {
        SMTTrieMigrationPlaceHolder.version()
    }
}
