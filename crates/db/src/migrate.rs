// This mod is used to init a db version when the db version is absent at the first time.
// And check present db version is still compatible. Godwoken must run on a valid db.
// If godwoken with an advanced verion runs on an old db, this is the time we can run migrations.
use crate::{
    error::Error,
    read_only_db::{self, ReadOnlyDB},
    schema::{COLUMN_META, META_TIP_BLOCK_HASH_KEY},
    Result,
};
use std::{cmp::Ordering, collections::BTreeMap};

use gw_config::StoreConfig;

use crate::{
    schema::{COLUMNS, MIGRATION_VERSION_KEY},
    RocksDB,
};

pub fn open_or_create_db(config: &StoreConfig) -> Result<RocksDB> {
    let read_only_db =
        read_only_db::ReadOnlyDB::open_cf(&config.path, vec![COLUMN_META.to_string()])?;
    let factory = init_migration_factory();
    if let Some(db) = read_only_db {
        match check_readonly_db_version(&db, factory.last_db_version())? {
            Ordering::Greater => {
                eprintln!(
                    "The database is created by a higher version executable binary, \n\
                     so that the current executable binary couldn't open this database.\n\
                     Please download the latest executable binary."
                );
                Err(Error {
                    message: "The database is created by a higher version executable binary"
                        .to_string(),
                })
            }
            Ordering::Equal => Ok(RocksDB::open(config, COLUMNS)),
            Ordering::Less => {
                log::info!("process fast migrations ...");
                // TODO: Currently, there is only one migration: Add db version.
                // So we can add db version by running default migration here.
                // We should stop the process if we have more migrations and use **migration** command to run migration instead.
                let db = RocksDB::open(config, COLUMNS);
                let _ = factory.migrate(db)?;

                Ok(RocksDB::open(config, COLUMNS))
            }
        }
    } else {
        let db = RocksDB::open(config, COLUMNS);
        init_db_version(&db, factory.last_db_version())?;
        Ok(db)
    }
}

//TODO: Replace with migration db version when we have our first migration impl.
pub(crate) fn init_db_version(db: &RocksDB, db_ver: Option<&str>) -> Result<()> {
    if let Some(db_ver) = db_ver {
        log::info!("Init db version: {}", db_ver);
        db.put_default(MIGRATION_VERSION_KEY, db_ver)?
    }
    Ok(())
}

fn check_readonly_db_version(db: &ReadOnlyDB, db_ver: Option<&str>) -> Result<Ordering> {
    let version = match db.get_pinned_default(MIGRATION_VERSION_KEY)? {
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

fn is_non_empty_rdb(db: &ReadOnlyDB) -> bool {
    if let Ok(v) = db.get_pinned(COLUMN_META, META_TIP_BLOCK_HASH_KEY) {
        if v.is_some() {
            return true;
        }
    }
    false
}

trait Migration {
    fn migrate(&self, db: RocksDB) -> Result<RocksDB>;
    // Version can be genereated with: date '+%Y%m%d%H%M%S'
    fn version(&self) -> &str;
    fn expensive(&self) -> bool;
}

struct DefaultMigration;
impl Migration for DefaultMigration {
    fn migrate(&self, db: RocksDB) -> Result<RocksDB> {
        Ok(db)
    }
    #[allow(clippy::needless_return)]
    fn version(&self) -> &str {
        return "20211229181750";
    }
    fn expensive(&self) -> bool {
        false
    }
}

struct MigrationFactory {
    migration_map: BTreeMap<String, Box<dyn Migration>>,
}

fn init_migration_factory() -> MigrationFactory {
    let mut factory = MigrationFactory::create();
    let migration = DefaultMigration;
    factory.insert(Box::new(migration));
    factory
}

impl MigrationFactory {
    fn create() -> Self {
        let migration_map = BTreeMap::new();
        Self { migration_map }
    }

    fn insert(&mut self, migration: Box<dyn Migration>) {
        self.migration_map
            .insert(migration.version().to_string(), migration);
    }

    fn migrate(&self, db: RocksDB) -> Result<RocksDB> {
        let db_version = db
            .get_pinned_default(MIGRATION_VERSION_KEY)?
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
            db.put_default(MIGRATION_VERSION_KEY, v)?;
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
    use crate::Result;
    use std::collections::HashMap;

    use gw_config::StoreConfig;

    use crate::{
        schema::{COLUMNS, MIGRATION_VERSION_KEY},
        RocksDB,
    };

    use super::{init_migration_factory, open_or_create_db, MigrationFactory};
    #[test]
    fn test_migration() -> Result<()> {
        let dir = tempfile::tempdir().expect("create temp dir");

        let config = StoreConfig {
            path: dir.path().to_owned(),
            options: HashMap::new(),
            options_file: None,
            cache_size: None,
        };
        let old_db = RocksDB::open(&config, COLUMNS);
        let factory = init_migration_factory();
        assert!(factory.last_db_version().is_some());

        let db = factory.migrate(old_db);

        assert!(db.is_ok());
        let db = db.unwrap();
        let v = db
            .get_pinned_default(MIGRATION_VERSION_KEY)?
            .map(|v| String::from_utf8(v.to_vec()));

        assert_eq!(v, Some(Ok(factory.last_db_version().unwrap().to_string())));
        Ok(())
    }

    #[test]
    fn test_migration_with_fresh_new() -> Result<()> {
        let dir = tempfile::tempdir().expect("create temp dir");

        let config = StoreConfig {
            path: dir.path().to_owned(),
            options: HashMap::new(),
            options_file: None,
            cache_size: None,
        };
        let db = open_or_create_db(&config)?;
        let v = db.get_pinned_default(MIGRATION_VERSION_KEY)?;
        assert!(v.is_some());
        let factory = init_migration_factory();

        let v = db
            .get_pinned_default(MIGRATION_VERSION_KEY)?
            .map(|v| String::from_utf8(v.to_vec()));

        assert_eq!(v, Some(Ok(factory.last_db_version().unwrap().to_string())));
        Ok(())
    }
}
