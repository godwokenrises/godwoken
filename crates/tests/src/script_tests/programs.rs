use blake2b::new_blake2b;
use ckb_types::bytes::Bytes;
use gw_common::blake2b;
use lazy_static::lazy_static;
use std::{fs, io::Read, path::PathBuf};

const SCRIPT_DIR: &str = "../../gwos/build/debug";
const CHALLENGE_LOCK_PATH: &str = "challenge-lock";
const WITHDRAWAL_LOCK_PATH: &str = "withdrawal-lock";
const CUSTODIAN_LOCK_PATH: &str = "custodian-lock";
const STAKE_LOCK_PATH: &str = "stake-lock";
const STATE_VALIDATOR: &str = "state-validator";
const SECP256K1_DATA_PATH: &str = "../../gwos/c/deps/ckb-production-scripts/build/secp256k1_data";
const ANYONE_CAN_PAY_LOCK_PATH: &str =
    "../../gwos/c/deps/ckb-production-scripts/build/anyone_can_pay";
const C_SCRIPTS_DIR: &str = "../../gwos/c/build";
const META_CONTRACT_BIN_NAME: &str = "meta-contract-validator";
const ETH_ADDR_REG_BIN_NAME: &str = "eth-addr-reg-generator";
// account locks
const ETH_LOCK_PATH: &str = "eth-account-lock";
const TRON_LOCK_PATH: &str = "tron-account-lock";

lazy_static! {
    pub static ref CHALLENGE_LOCK_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&CHALLENGE_LOCK_PATH);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref CHALLENGE_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&CHALLENGE_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
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
    pub static ref CUSTODIAN_LOCK_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&CUSTODIAN_LOCK_PATH);
        let mut f = fs::File::open(&path).expect("load custodian lock program");
        f.read_to_end(&mut buf)
            .expect("read custodian lock program");
        Bytes::from(buf.to_vec())
    };
    pub static ref CUSTODIAN_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&CUSTODIAN_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref STAKE_LOCK_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&STAKE_LOCK_PATH);
        let mut f = fs::File::open(&path).expect("load stake lock program");
        f.read_to_end(&mut buf).expect("read stake lock program");
        Bytes::from(buf.to_vec())
    };
    pub static ref STAKE_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&STAKE_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref ETH_ACCOUNT_LOCK_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&ETH_LOCK_PATH);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref ETH_ACCOUNT_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ETH_ACCOUNT_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref TRON_ACCOUNT_LOCK_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&TRON_LOCK_PATH);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref TRON_ACCOUNT_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&TRON_ACCOUNT_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref SECP256K1_DATA: Bytes = {
        let mut buf = Vec::new();
        let mut f = fs::File::open(&SECP256K1_DATA_PATH).expect("load secp256k1 data");
        f.read_to_end(&mut buf).expect("read secp256k1 data");
        Bytes::from(buf.to_vec())
    };
    pub static ref SECP256K1_DATA_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&SECP256K1_DATA);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref ANYONE_CAN_PAY_LOCK_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut f = fs::File::open(&ANYONE_CAN_PAY_LOCK_PATH).expect("load acp lock program");
        f.read_to_end(&mut buf).expect("read acp program");
        Bytes::from(buf.to_vec())
    };
    pub static ref ANYONE_CAN_PAY_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ANYONE_CAN_PAY_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref META_CONTRACT_VALIDATOR_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&C_SCRIPTS_DIR);
        path.push(&META_CONTRACT_BIN_NAME);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref META_CONTRACT_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&META_CONTRACT_VALIDATOR_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref ETH_ADDR_REG_VALIDATOR_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&C_SCRIPTS_DIR);
        path.push(&ETH_ADDR_REG_BIN_NAME);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref ETH_ADDR_REG_CONTRACT_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ETH_ADDR_REG_VALIDATOR_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref WITHDRAWAL_LOCK_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&WITHDRAWAL_LOCK_PATH);
        let mut f = fs::File::open(&path).expect("load withdrawal lock program");
        f.read_to_end(&mut buf)
            .expect("read withdrawal lock program");
        Bytes::from(buf.to_vec())
    };
    pub static ref WITHDRAWAL_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&WITHDRAWAL_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
}
