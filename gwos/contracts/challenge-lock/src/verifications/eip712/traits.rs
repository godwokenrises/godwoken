use alloc::vec::Vec;
use sha3::{Digest, Keccak256};

pub trait EIP712Encode {
    fn type_name() -> &'static str;
    fn encode_type(&self, buf: &mut Vec<u8>);
    fn encode_data(&self, buf: &mut Vec<u8>);

    fn hash_struct(&self) -> [u8; 32] {
        let type_hash: [u8; 32] = {
            let mut buf = Vec::default();
            self.encode_type(&mut buf);
            let mut hasher = Keccak256::new();
            hasher.update(&mut buf);
            hasher.finalize().into()
        };
        let encoded_data = {
            let mut buf = Vec::default();
            self.encode_data(&mut buf);
            buf
        };
        let mut hasher = Keccak256::new();
        hasher.update(&type_hash);
        hasher.update(&encoded_data);
        hasher.finalize().into()
    }

    fn eip712_message(&self, domain_seperator: [u8; 32]) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update(b"\x19\x01");
        hasher.update(&domain_seperator);
        hasher.update(&self.hash_struct());
        hasher.finalize().into()
    }
}
