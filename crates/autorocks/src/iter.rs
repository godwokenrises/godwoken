use std::{hint::unreachable_unchecked, marker::PhantomData, pin::Pin};

use autocxx::prelude::UniquePtr;
use autorocks_sys::rocksdb::Iterator;

use crate::slice::as_rust_slice1;

pub enum Direction {
    Forward,
    Backward,
}

pub struct DbIterator<T> {
    pub(crate) inner: UniquePtr<Iterator>,
    pub(crate) just_seeked: bool,
    pub(crate) direction: Direction,
    pub(crate) phantom: PhantomData<T>,
}

impl<T> DbIterator<T> {
    /// Safety: inner must NOT be null.
    pub(crate) unsafe fn new(mut inner: UniquePtr<Iterator>, direction: Direction) -> Self {
        let ptr = unwrap_unchecked(inner.as_mut());
        match direction {
            Direction::Forward => ptr.SeekToFirst(),
            Direction::Backward => ptr.SeekToLast(),
        }
        Self {
            inner,
            just_seeked: true,
            direction,
            phantom: PhantomData,
        }
    }

    pub fn as_inner(&self) -> &Iterator {
        unsafe { unwrap_unchecked(self.inner.as_ref()) }
    }

    pub fn as_inner_mut(&mut self) -> Pin<&mut Iterator> {
        unsafe { unwrap_unchecked(self.inner.as_mut()) }
    }

    pub fn seek(&mut self, key: &[u8]) {
        self.as_inner_mut().Seek(&key.into());
        self.just_seeked = true;
    }

    pub fn seek_for_prev(&mut self, key: &[u8]) {
        self.as_inner_mut().SeekForPrev(&key.into());
        self.just_seeked = true;
    }

    pub fn valid(&self) -> bool {
        self.as_inner().Valid()
    }

    pub fn key(&self) -> Option<&[u8]> {
        if self.valid() {
            Some(unsafe { as_rust_slice1(self.as_inner().key()) })
        } else {
            None
        }
    }

    pub fn value(&self) -> Option<&[u8]> {
        if self.valid() {
            Some(unsafe { as_rust_slice1(self.as_inner().value()) })
        } else {
            None
        }
    }
}

impl<T> core::iter::Iterator for DbIterator<T> {
    type Item = (Box<[u8]>, Box<[u8]>);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.just_seeked {
            match self.direction {
                Direction::Backward => self.as_inner_mut().Prev(),
                Direction::Forward => self.as_inner_mut().Next(),
            }
        } else {
            self.just_seeked = false;
        }
        if self.as_inner().Valid() {
            let v = (
                unsafe { unwrap_unchecked(self.key()) }.into(),
                unsafe { unwrap_unchecked(self.value()) }.into(),
            );
            Some(v)
        } else {
            None
        }
    }
}

unsafe fn unwrap_unchecked<T>(x: Option<T>) -> T {
    match x {
        Some(x) => x,
        None => unreachable_unchecked(),
    }
}
