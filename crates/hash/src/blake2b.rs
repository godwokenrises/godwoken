pub use blake2b_ref::{Blake2b, Blake2bBuilder};

pub const BLAKE2B_KEY: &[u8] = &[];
pub const BLAKE2B_LEN: usize = 32;
pub const CKB_PERSONALIZATION: &[u8] = b"ckb-default-hash";

pub fn new_blake2b() -> Blake2b {
    Blake2bBuilder::new(32)
        .personal(CKB_PERSONALIZATION)
        .build()
}
