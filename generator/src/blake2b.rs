pub use blake2b_rs::{Blake2b, Blake2bBuilder};

pub fn new_blake2b() -> Blake2b {
    Blake2bBuilder::new(32)
        .personal(b"ckb-default-hash")
        .build()
}
