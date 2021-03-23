pub struct Wallet;

impl Wallet {
    pub fn lock_hash(&self) -> [u8; 32] {
        unimplemented!()
    }

    // sign message
    pub fn sign(&self, msg: [u8; 32]) -> [u8; 65] {
        unimplemented!()
    }
}
