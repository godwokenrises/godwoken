use std::path::PathBuf;

use autorocks::*;
use autorocks_sys::rocksdb::{CompressionType, PinnableSlice, Status_Code};
use moveit::moveit;
use tempfile::{tempdir, TempDir};

fn open_temp(columns: usize) -> (TransactionDb, TempDir) {
    let dir = tempdir().unwrap();
    (
        DbOptions::new(dir.path(), columns)
            .create_if_missing(true)
            .create_missing_column_families(true)
            .open()
            .unwrap(),
        dir,
    )
}

#[test]
fn test_db_open_put_get_delete_drop_cf_int_property() {
    let (mut db, _dir) = open_temp(1);
    db.put(0, b"key", b"value").unwrap();
    assert_eq!(db.default_col(), 1);
    db.put(db.default_col(), b"default", b"default").unwrap();
    moveit! {
        let mut slice = PinnableSlice::new();
    }
    let v = db.get(0, b"key", slice.as_mut()).unwrap();
    assert_eq!(v.unwrap(), b"value");
    let v = db
        .get(db.default_col(), b"default", slice.as_mut())
        .unwrap();
    assert_eq!(v.unwrap(), b"default");
    db.delete(0, b"key").unwrap();
    let v = db.get(0, b"key", slice.as_mut()).unwrap();
    assert!(v.is_none());

    db.drop_cf(0).unwrap();

    let size = db
        .get_int_property(db.default_col(), "rocksdb.size-all-mem-tables")
        .unwrap();
    assert!(size > 0);
}

#[test]
fn test_db_set_options() {
    let (db, _dir) = open_temp(1);
    db.set_db_options([("max_subcompactions", "2")]).unwrap();
    db.set_options(0, [("ttl", "36000")]).unwrap();
}

#[test]
fn test_load_options() {
    let dir = tempdir().unwrap();
    let mut opts = DbOptions::new(dir.path(), 21);
    let mut options_file = PathBuf::from(file!());
    options_file.set_file_name("db.toml");
    opts.load_options_from_file(&options_file, 1024 * 1024)
        .unwrap();
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);
    let db = opts.open().unwrap();
    assert_eq!(db.default_col(), 21);
}

#[test]
fn test_read_only_db() {
    let (db, dir) = open_temp(5);
    db.put(0, b"key", b"value").unwrap();
    drop(db);

    let rdb = DbOptions::new(dir.path(), 1).open_read_only().unwrap();
    moveit! {
        let mut slice = PinnableSlice::new();
    }
    let v = rdb.get(0, b"key", slice.as_mut()).unwrap();
    assert_eq!(v.unwrap(), b"value");
}

#[cfg(feature = "snappy")]
#[test]
fn test_db_open_snappy() {
    let dir = tempdir().unwrap();
    let db = DbOptions::new(dir.path(), 1)
        .create_if_missing(true)
        .create_missing_column_families(true)
        .compression(CompressionType::kSnappyCompression)
        .open()
        .unwrap();
    db.put(0, b"key", b"value").unwrap();
}

#[test]
fn test_snapshot() {
    let (db, _dir) = open_temp(1);
    db.put(0, b"key", b"value").unwrap();
    let snap = db.snapshot();
    db.put(0, b"key", b"value1").unwrap();
    let snap1 = db.snapshot();
    moveit! {
        let mut slice = PinnableSlice::new();
    }
    let v = snap.get(0, b"key", slice.as_mut()).unwrap();
    assert_eq!(v.unwrap(), b"value");
    let v = snap1.get(0, b"key", slice.as_mut()).unwrap();
    assert_eq!(v.unwrap(), b"value1");
    let v = snap1.get(0, b"key1", slice.as_mut()).unwrap();
    assert!(v.is_none());
}

#[test]
fn test_tx_and_tx_snapshot() {
    let (db, _dir) = open_temp(1);
    db.put(0, b"key", b"value").unwrap();
    moveit! {
        let mut slice = PinnableSlice::new();
    }
    let mut tx = db.begin_transaction();

    db.put(0, b"key", b"value1").unwrap();

    let snap = tx.snapshot();
    let snap1 = tx.timestamped_snapshot();
    let v = snap.get(0, b"key", slice.as_mut()).unwrap().unwrap();
    assert_eq!(v, b"value");
    let v = tx.get(0, b"key", slice.as_mut()).unwrap().unwrap();
    assert_eq!(v, b"value1");

    tx.put(0, b"key1", b"value1").unwrap();
    let err = tx.put(0, b"key", b"value2").unwrap_err();
    assert!(err.code == Status_Code::kBusy);
    tx.delete(0, b"key1").unwrap();
    let v = tx.get(0, b"key1", slice.as_mut()).unwrap();
    assert!(v.is_none());

    tx.commit().unwrap();
    drop(tx);

    let v = snap1.get(0, b"key", slice.as_mut()).unwrap().unwrap();
    assert_eq!(v, b"value");
}

#[test]
fn test_iter() {
    let (db, _dir) = open_temp(1);
    db.put(0, b"key", b"value").unwrap();
    let tx = db.begin_transaction();
    let snap1 = tx.snapshot();
    let snap = db.snapshot();
    db.put(0, b"key1", b"value1").unwrap();
    db.put(0, b"key3", b"value").unwrap();
    db.put(0, b"key4", b"value").unwrap();
    db.put(0, b"key5", b"value").unwrap();
    assert_eq!(snap.iter(0, Direction::Forward).count(), 1);
    assert_eq!(snap1.iter(0, Direction::Backward).count(), 1);
    assert_eq!(db.iter(0, Direction::Backward).count(), 5);
    assert_eq!(tx.iter(0, Direction::Forward).count(), 5);

    let mut iter = db.iter(0, Direction::Forward);
    iter.seek(b"key2");
    assert_eq!(iter.count(), 3);
}

#[test]
fn test_write_batch() {
    let (db, _dir) = open_temp(1);
    db.put(0, b"key", b"value").unwrap();
    let mut wb = db.new_write_batch();
    wb.put(0, b"key1", b"value1").unwrap();
    wb.delete(0, b"key").unwrap();
    db.write(&mut wb).unwrap();
    moveit! {
        let mut buf = PinnableSlice::new();
    }
    assert!(db.get(0, b"key", buf.as_mut()).unwrap().is_none());
    assert!(db.get(0, b"key1", buf.as_mut()).unwrap().is_some());
}

#[test]
fn test_clear_cf() {
    let (mut db, _dir) = open_temp(1);
    db.put(0, b"key", b"value").unwrap();
    db.put(0, b"key1", b"value1").unwrap();
    db.clear_cf(0).unwrap();
    assert_eq!(db.iter(0, Direction::Forward).count(), 0);
    db.put(0, b"key", b"value").unwrap();
    assert_eq!(db.iter(0, Direction::Forward).count(), 1);
}
