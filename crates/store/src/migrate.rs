// This mod is used to init a db version when the db version is absent at the first time.
// And check present db version is still compatible. Godwoken must run on a valid db.
// If godwoken with an advanced verion runs on an old db, this is the time we can run migrations.

use std::{cmp::Ordering, collections::BTreeMap};

use anyhow::{bail, Result};
use autorocks::{
    autorocks_sys::rocksdb::Status_SubCode, moveit::slot, DbOptions, Direction, ReadOnlyDb,
    TransactionDb,
};
use gw_config::StoreConfig;

use crate::{
    schema::{
        COLUMNS, COLUMN_BAD_BLOCK, COLUMN_BLOCK, COLUMN_META, META_LAST_VALID_TIP_BLOCK_HASH_KEY,
        META_TIP_BLOCK_HASH_KEY, MIGRATION_VERSION_KEY, REMOVED_COLUMN_BLOCK_DEPOSIT_REQUESTS,
        REMOVED_COLUMN_L2BLOCK_COMMITTED_INFO,
    },
    Store,
};

pub fn open_or_create_db(config: &StoreConfig, factory: MigrationFactory) -> Result<TransactionDb> {
    let read_only_db = match DbOptions::new(&config.path, 1).open_read_only() {
        Ok(db) => Some(db),
        Err(e) if e.sub_code == Status_SubCode::kPathNotFound => None,
        Err(e) => bail!(e),
    };

    if let Some(db) = read_only_db {
        match check_readonly_db_version(&db, factory.last_db_version())? {
            Ordering::Greater => {
                eprintln!(
                    "The database is created by a higher version executable binary, \n\
                     so that the current executable binary couldn't open this database.\n\
                     Please download the latest executable binary."
                );
                bail!("The database is created by a higher version executable binary");
            }
            Ordering::Equal => Ok(Store::open(config, COLUMNS)?.into_inner()),
            Ordering::Less => {
                log::info!("process migrations ...");

                let db = Store::open(config, COLUMNS)?.into_inner();

                let _ = factory.migrate(db)?;

                Ok(Store::open(config, COLUMNS)?.into_inner())
            }
        }
    } else {
        let db = Store::open(config, COLUMNS)?.into_inner();
        init_db_version(&db, factory.last_db_version())?;
        Ok(db)
    }
}

//TODO: Replace with migration db version when we have our first migration impl.
pub(crate) fn init_db_version(db: &TransactionDb, db_ver: Option<&str>) -> Result<()> {
    if let Some(db_ver) = db_ver {
        log::info!("Init db version: {}", db_ver);
        db.put(db.default_col(), MIGRATION_VERSION_KEY, db_ver.as_bytes())?;
    }
    Ok(())
}

fn check_readonly_db_version(db: &ReadOnlyDb, db_ver: Option<&str>) -> Result<Ordering> {
    slot!(slice);
    let version = match db.get(db.default_col(), MIGRATION_VERSION_KEY, slice)? {
        Some(version_bytes) => {
            String::from_utf8(version_bytes.to_vec()).expect("version bytes to utf8")
        }
        None => {
            let ordering = if is_non_empty_rdb(db) {
                Ordering::Less
            } else {
                Ordering::Equal
            };
            return Ok(ordering);
        }
    };
    log::debug!("current database version [{}]", version);
    Ok(version.as_str().cmp(db_ver.expect("Db version is absent!")))
}

fn is_non_empty_rdb(db: &ReadOnlyDb) -> bool {
    slot!(slice);
    if let Ok(v) = db.get(COLUMN_META, META_TIP_BLOCK_HASH_KEY, slice) {
        if v.is_some() {
            return true;
        }
    }
    false
}

pub trait Migration {
    fn migrate(&self, db: TransactionDb) -> Result<TransactionDb>;
    // Version can be genereated with: date '+%Y%m%d%H%M%S'
    fn version(&self) -> &str;
}

struct DefaultMigration;
impl Migration for DefaultMigration {
    fn migrate(&self, db: TransactionDb) -> Result<TransactionDb> {
        Ok(db)
    }
    #[allow(clippy::needless_return)]
    fn version(&self) -> &str {
        return "20211229181750";
    }
}

struct DecoupleBlockProducingSubmissionAndConfirmationMigration;

impl Migration for DecoupleBlockProducingSubmissionAndConfirmationMigration {
    fn migrate(&self, mut db: TransactionDb) -> Result<TransactionDb> {
        if db.iter(COLUMN_BLOCK, Direction::Forward).next().is_some() {
            bail!("Cannot migrate a database with existing data to version 20220517. You have to deploy a new node");
        }

        db.drop_cf(REMOVED_COLUMN_L2BLOCK_COMMITTED_INFO)?;
        db.drop_cf(REMOVED_COLUMN_BLOCK_DEPOSIT_REQUESTS)?;
        Ok(db)
    }
    fn version(&self) -> &str {
        "20220517"
    }
}

struct BadBlockColumnMigration;

impl Migration for BadBlockColumnMigration {
    fn migrate(&self, mut db: TransactionDb) -> Result<TransactionDb> {
        // Check that there are no bad blocks.
        slot!(slice1, slice2);
        let tip = db.get(COLUMN_META, META_TIP_BLOCK_HASH_KEY, slice1)?;
        let valid_tip = db.get(COLUMN_META, META_LAST_VALID_TIP_BLOCK_HASH_KEY, slice2)?;
        if tip.as_deref() != valid_tip.as_deref() {
            bail!("Cannot migrate to version 20221024 when there are bad blocks. You have to rewind or revert first");
        }
        drop(tip);
        drop(valid_tip);

        // Clear this reused column.
        db.drop_cf(COLUMN_BAD_BLOCK)?;
        Ok(db)
    }
    fn version(&self) -> &str {
        "20221024"
    }
}

#[cfg(feature = "smt-trie")]
pub struct SMTTrieMigrationPlaceHolder;

#[cfg(feature = "smt-trie")]
impl Migration for SMTTrieMigrationPlaceHolder {
    fn migrate(&self, db: TransactionDb) -> Result<TransactionDb> {
        use crate::schema::{
            COLUMN_ACCOUNT_SMT_LEAF, COLUMN_BLOCK_SMT_LEAF, COLUMN_REVERTED_BLOCK_SMT_LEAF,
        };

        // Nothing to do if SMT leaves are empty.
        let smts_all_empty = [
            COLUMN_BLOCK_SMT_LEAF,
            COLUMN_ACCOUNT_SMT_LEAF,
            COLUMN_REVERTED_BLOCK_SMT_LEAF,
        ]
        .iter()
        .all(|col| db.iter(*col, Direction::Forward).next().is_none());
        if smts_all_empty {
            return Ok(db);
        }

        bail!(
            "Cannot automatically migrate to version {}. Use “godwoken migrate” command instead",
            Self.version(),
        );
    }
    fn version(&self) -> &str {
        // Use a very large version so that enabling smt-trie feature always needs migration.
        "9999-20221125-smt-trie"
    }
}

pub struct MigrationFactory {
    migration_map: BTreeMap<String, Box<dyn Migration>>,
}

pub fn init_migration_factory() -> MigrationFactory {
    let mut factory = MigrationFactory::create();
    let migration = DefaultMigration;
    factory.insert(Box::new(migration));
    factory.insert(Box::new(
        DecoupleBlockProducingSubmissionAndConfirmationMigration,
    ));
    #[cfg(feature = "smt-trie")]
    factory.insert(Box::new(SMTTrieMigrationPlaceHolder));
    factory
}

impl MigrationFactory {
    fn create() -> Self {
        let migration_map = BTreeMap::new();
        Self { migration_map }
    }

    /// Insert a new migration.
    ///
    /// Returns whether the migration replaces a previously inserted one.
    pub fn insert(&mut self, migration: Box<dyn Migration>) -> bool {
        self.migration_map
            .insert(migration.version().to_string(), migration)
            .is_some()
    }

    fn migrate(&self, db: TransactionDb) -> Result<TransactionDb> {
        slot!(slice);
        let db_version = db
            .get(db.default_col(), MIGRATION_VERSION_KEY, slice)?
            .map(|v| String::from_utf8(v.to_vec()).expect("version bytes to utf8"))
            .unwrap_or_else(|| "".to_string());
        let mut db = db;
        let v = db_version.as_str();
        let mut last_version = None;
        for (mv, migration) in &self.migration_map {
            let mv = mv.as_str();
            if mv > v {
                db = migration.migrate(db)?;
                last_version = Some(mv);
            }
        }
        if let Some(v) = last_version {
            db.put(db.default_col(), MIGRATION_VERSION_KEY, v.as_bytes())?;
            log::info!("Current db version is: {}", v);
        }
        Ok(db)
    }

    fn last_db_version(&self) -> Option<&str> {
        self.migration_map.values().last().map(|m| m.version())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration() -> Result<()> {
        let dir = tempfile::tempdir().expect("create temp dir");

        let config = StoreConfig {
            path: dir.path().to_owned(),
            options_file: None,
            cache_size: None,
        };
        let old_db = Store::open(&config, COLUMNS)?.into_inner();
        let factory = init_migration_factory();
        assert!(factory.last_db_version().is_some());

        let db = factory.migrate(old_db);

        assert!(db.is_ok());
        let db = db.unwrap();
        slot!(slice);
        let v = db
            .get(db.default_col(), MIGRATION_VERSION_KEY, slice)?
            .map(|v| String::from_utf8(v.to_vec()));

        assert_eq!(v, Some(Ok(factory.last_db_version().unwrap().to_string())));
        Ok(())
    }

    #[test]
    fn test_migration_with_fresh_new() -> Result<()> {
        let dir = tempfile::tempdir().expect("create temp dir");

        let config = StoreConfig {
            path: dir.path().to_owned(),
            options_file: None,
            cache_size: None,
        };
        let db = open_or_create_db(&config, init_migration_factory())?;
        {
            slot!(slice);
            let v = db.get(db.default_col(), MIGRATION_VERSION_KEY, slice)?;
            assert!(v.is_some());
        }
        let factory = init_migration_factory();

        slot!(slice);
        let v = db
            .get(db.default_col(), MIGRATION_VERSION_KEY, slice)?
            .map(|v| String::from_utf8(v.to_vec()));

        assert_eq!(v, Some(Ok(factory.last_db_version().unwrap().to_string())));
        Ok(())
    }
}
