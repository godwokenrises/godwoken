use gw_common::blake2b::new_blake2b;
use gw_types::bytes::Bytes;
use lazy_static::lazy_static;
use std::{fs, io::Read, path::PathBuf};

mod state_validator;

const SCRIPT_DIR: &'static str = "../../build/debug";
const STATE_VALIDATOR: &'static str = "dummy-state-validator";

lazy_static! {
    pub static ref STATE_VALIDATOR_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&STATE_VALIDATOR);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref STATE_VALIDATOR_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&STATE_VALIDATOR_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
}
