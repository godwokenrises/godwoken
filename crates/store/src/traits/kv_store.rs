use std::cell::RefCell;

use anyhow::Result;

use crate::schema::Col;

pub trait KVStoreRead {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>>;
}

pub trait KVStoreWrite {
    fn insert_raw(&mut self, col: Col, key: &[u8], value: &[u8]) -> Result<()>;
    fn delete(&mut self, col: Col, key: &[u8]) -> Result<()>;
}

pub trait KVStore: KVStoreRead + KVStoreWrite {}

impl<T: KVStoreRead> KVStoreRead for &T {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        <T as KVStoreRead>::get(self, col, key)
    }
}

impl<T: KVStoreRead> KVStoreRead for &mut T {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        <T as KVStoreRead>::get(self, col, key)
    }
}

impl<T: KVStoreRead> KVStoreRead for &RefCell<T> {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        <T as KVStoreRead>::get(&self.borrow(), col, key)
    }
}

impl<T: KVStoreWrite> KVStoreWrite for &mut T {
    fn insert_raw(&mut self, col: Col, key: &[u8], value: &[u8]) -> Result<()> {
        <T as KVStoreWrite>::insert_raw(self, col, key, value)
    }
    fn delete(&mut self, col: Col, key: &[u8]) -> Result<()> {
        <T as KVStoreWrite>::delete(self, col, key)
    }
}

impl<T: KVStoreWrite> KVStoreWrite for &RefCell<T> {
    fn insert_raw(&mut self, col: Col, key: &[u8], value: &[u8]) -> Result<()> {
        <T as KVStoreWrite>::insert_raw(&mut self.borrow_mut(), col, key, value)
    }
    fn delete(&mut self, col: Col, key: &[u8]) -> Result<()> {
        <T as KVStoreWrite>::delete(&mut self.borrow_mut(), col, key)
    }
}

impl<T: KVStore> KVStore for &mut T {}
impl<T: KVStore> KVStore for &RefCell<T> {}
