use gw_common::blake2b::new_blake2b;
use gw_types::bytes::Bytes;
use lazy_static::lazy_static;
use std::{fs, io::Read, path::PathBuf};

mod stake_lock;

const SCRIPT_DIR: &'static str = "../../build/debug";
const STAKE_LOCK: &'static str = "stake-lock";

lazy_static! {
    pub static ref STAKE_LOCK_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&STAKE_LOCK);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref STAKE_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&STAKE_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
}
