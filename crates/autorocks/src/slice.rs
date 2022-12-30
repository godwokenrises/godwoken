use std::{ops::Deref, pin::Pin};

use autorocks_sys::rocksdb::{PinnableSlice, Slice};
use moveit::MoveRef;

pub struct PinnedSlice<'a> {
    slice: Pin<MoveRef<'a, PinnableSlice>>,
}

impl<'a> PinnedSlice<'a> {
    pub(crate) fn new(slice: Pin<MoveRef<'a, PinnableSlice>>) -> Self {
        Self { slice }
    }
}

impl<'a> Deref for PinnedSlice<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        as_rust_slice(&self.slice)
    }
}

impl<'a> AsRef<[u8]> for PinnedSlice<'a> {
    fn as_ref(&self) -> &[u8] {
        as_rust_slice(&self.slice)
    }
}

pub(crate) unsafe fn as_rust_slice1<'a>(s: Slice) -> &'a [u8] {
    core::slice::from_raw_parts(s.data_ as *const _, s.size_)
}

pub(crate) fn as_rust_slice(s: &PinnableSlice) -> &[u8] {
    let s = s.as_ref();
    unsafe { core::slice::from_raw_parts(s.data_ as *const _, s.size_) }
}
