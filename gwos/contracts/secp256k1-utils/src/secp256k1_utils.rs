#[link(name = "ckb-secp256k1", kind = "static")]
extern "C" {
    fn recover_secp256k1_uncompressed_key(
        message: *const u8,
        signature: *const u8,
        output_uncompressed_pubkey: *mut u8,
    ) -> i32;
}

pub fn recover_uncompressed_key(message: [u8; 32], signature: [u8; 65]) -> Result<[u8; 65], i32> {
    let mut pubkey = [0u8; 65];
    let ret = unsafe {
        recover_secp256k1_uncompressed_key(
            message.as_ptr(),
            signature.as_ptr(),
            pubkey.as_mut_ptr(),
        )
    };
    if ret == 0 {
        Ok(pubkey)
    } else {
        Err(ret)
    }
}
