use autorocks_sys::rocksdb::{PinnableSlice, Slice};

pub(crate) unsafe fn as_rust_slice1<'a>(s: Slice) -> &'a [u8] {
    core::slice::from_raw_parts(s.data_ as *const _, s.size_)
}

pub(crate) fn as_rust_slice(s: &PinnableSlice) -> &[u8] {
    let s = s.as_ref();
    unsafe { core::slice::from_raw_parts(s.data_ as *const _, s.size_) }
}
