use ckb_std::dynamic_loading::{CKBDLContext, Symbol};
use crate::code_hashes::CODE_HASH_SECP256K1;

/// function signature of validate_secp256k1_blake2b_sighash_all
type ValidateBlake2bSighashAll = unsafe extern "C" fn(pub_key_hash: *mut u8) -> i32;

/// Symbol name
const VALIDATE_BLAKE2B_SIGHASH_ALL: &[u8; 38] = b"validate_secp256k1_blake2b_sighash_all";

pub struct Secp256k1Lib {
    validate_blake2b_sighash_all: Symbol<ValidateBlake2bSighashAll>,
}

impl Secp256k1Lib {
    pub fn load<T>(context: &mut CKBDLContext<T>) -> Self {
        // load library
        let lib = context.load(&CODE_HASH_SECP256K1).expect("load secp256k1");

        // find symbols
        let validate_blake2b_sighash_all: Symbol<ValidateBlake2bSighashAll> = unsafe {
            lib
            .get(VALIDATE_BLAKE2B_SIGHASH_ALL)
            .expect("load function")
        };
        Secp256k1Lib {
            validate_blake2b_sighash_all,
        }
    }

    pub fn validate_blake2b_sighash_all(&self, expected_pubkey_hash: &[u8]) -> Result<bool, i32> {
        let mut pubkey_hash = [0u8; 20];
        let f = &self.validate_blake2b_sighash_all;
        let error_code = unsafe {f(pubkey_hash.as_mut_ptr())};
        if error_code == 0 {
            Ok(expected_pubkey_hash == pubkey_hash)
        } else {
            Err(error_code)
        }
    }
}
