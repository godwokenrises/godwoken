#[allow(dead_code)]
pub mod constants;
use anyhow::{anyhow, Result};
use constants::{
    BLOCK_HASH, BLOCK_PRODUCER_ETH_ADDRESSS, CHAIN_ID, CREATOR_ACCOUNT_ID,
    ETH_ACCOUNT_LOCK_CODE_HASH, ETH_ADDRESS_REGISTRY_PROGRAM_CODE_HASH, GW_ERROR_ACCOUNT_NOT_FOUND,
    GW_ERROR_DUPLICATED_SCRIPT_HASH, GW_ERROR_INVALID_ACCOUNT_SCRIPT, GW_ERROR_NOT_FOUND,
    GW_ITEM_MISSING, META_VALIDATOR_SCRIPT_TYPE_HASH, POLYJUICE_PROGRAM_CODE_HASH,
    ROLLUP_SCRIPT_HASH, SUCCESS, SUDT_VALIDATOR_SCRIPT_TYPE_HASH,
};
use gw_common::blake2b::new_blake2b;
use gw_common::builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID};
use gw_common::registry_address::RegistryAddress;
use gw_common::state::{
    build_account_field_key, build_account_key, build_data_hash_key,
    build_script_hash_to_account_id_key, State, GW_ACCOUNT_NONCE_TYPE, GW_ACCOUNT_SCRIPT_HASH_TYPE,
};
use gw_common::CKB_SUDT_SCRIPT_ARGS;
use gw_generator::syscalls::bn;
use gw_generator::traits::StateExt;
use gw_store::Store;
use gw_traits::CodeStore;
use gw_types::U256;
use gw_types::{
    bytes::Bytes,
    core::{AllowedContractType, AllowedEoaType, ScriptHashType},
    offchain::RunResult,
    packed::*,
    prelude::*,
};
use gwstore::state::traits::JournalDB;
use once_cell::sync::Lazy;
use std::os::raw::{c_char, c_int, c_void};
use std::u128;
use std::{ffi::CStr, sync::Mutex};

use crate::constants::ERROR;
use gw_smt::smt::SMT;
pub use gw_store as gwstore;
pub use gw_types;
use gw_types::h256::*;
use gwstore::{
    smt::smt_store::SMTStateStore,
    snapshot::StoreSnapshot,
    state::{
        overlay::{mem_state::MemStateTree, mem_store::MemStore},
        MemStateDB,
    },
};

type DummyState = MemStateDB;
pub fn new_dummy_state(store: StoreSnapshot) -> MemStateDB {
    let smt = SMT::new(
        H256::zero().into(),
        SMTStateStore::new(MemStore::new(store)),
    );
    let inner = MemStateTree::new(smt, 0);
    MemStateDB::new(inner)
}

struct GodwokenHost {
    rollup_config: RollupConfig,
    block_info: BlockInfo,
    tx: Option<Vec<u8>>,
    run_result: RunResult,
    state: DummyState,
}

impl GodwokenHost {
    fn new() -> Self {
        let rollup_config = RollupConfig::new_builder()
            .chain_id(CHAIN_ID.pack())
            .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
            .allowed_contract_type_hashes(
                vec![
                    AllowedTypeHash::new(
                        AllowedContractType::Meta,
                        META_VALIDATOR_SCRIPT_TYPE_HASH,
                    ),
                    AllowedTypeHash::new(
                        AllowedContractType::Sudt,
                        SUDT_VALIDATOR_SCRIPT_TYPE_HASH,
                    ),
                    AllowedTypeHash::new(
                        AllowedContractType::Polyjuice,
                        POLYJUICE_PROGRAM_CODE_HASH,
                    ),
                    AllowedTypeHash::new(
                        AllowedContractType::EthAddrReg,
                        ETH_ADDRESS_REGISTRY_PROGRAM_CODE_HASH,
                    ),
                ]
                .pack(),
            )
            .allowed_eoa_type_hashes(
                vec![AllowedTypeHash::new(
                    AllowedEoaType::Eth,
                    ETH_ACCOUNT_LOCK_CODE_HASH,
                )]
                .pack(),
            )
            .build();
        let store = Store::open_tmp().expect("open store");
        let snapshot = store.get_snapshot();
        let state = new_dummy_state(snapshot); // will be reset
        let eth_address: [u8; 20] = hex::decode(BLOCK_PRODUCER_ETH_ADDRESSS)
            .expect("decode hex eth_address")
            .try_into()
            .unwrap();
        let address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec());
        let block_info = BlockInfo::new_builder()
            .number(1.pack())
            .timestamp(1.pack())
            .block_producer(Bytes::from(address.to_bytes()).pack())
            .build();
        GodwokenHost {
            rollup_config,
            block_info,
            tx: None,
            run_result: RunResult::default(),
            state,
        }
    }
    fn get_raw(&mut self, key: &H256) -> Result<H256> {
        let tree_value = self
            .state
            .get_raw(key)
            .map_err(|err| anyhow!(err.to_string()))?;
        self.run_result.read_data_hashes.insert(*key);
        Ok(tree_value)
    }
    fn get_account_id_by_script_hash(&mut self, script_hash: &H256) -> Result<Option<u32>> {
        let value = self
            .get_raw(&build_script_hash_to_account_id_key(script_hash.as_slice()))
            .map_err(|err| anyhow!("syscall error: get account id by script hash : {:?}", err))?;
        if value.is_zero() {
            return Ok(None);
        }
        let id = value.to_u32();
        Ok(Some(id))
    }

    fn get_account_count(&self) -> Result<u32> {
        let count = self
            .state
            .get_account_count()
            .map_err(|err| anyhow!("syscall error: get account count : {:?}", err))?;
        Ok(count)
    }

    fn get_script_hash(&mut self, id: u32) -> Result<H256> {
        let value = self
            .get_raw(&build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE))
            .map_err(|err| anyhow!("syscall error: get script hash by account id : {:?}", err))?;
        Ok(value)
    }

    fn get_script(&mut self, script_hash: &H256) -> Option<Script> {
        self.state.get_script(script_hash)
    }
    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.state.get_data(data_hash)
    }
}

static HOST: Lazy<Mutex<GodwokenHost>> = Lazy::new(|| Mutex::new(GodwokenHost::new()));

#[no_mangle]
pub extern "C" fn ckb_exit(code: i8) -> i32 {
    std::process::exit(code.into());
}

#[no_mangle]
pub extern "C" fn ckb_debug(s: *const c_char) {
    let message = unsafe { CStr::from_ptr(s) }.to_str().expect("UTF8 error!");
    println!("Debug message: {}", message);
}

#[no_mangle]
pub extern "C" fn gw_load_rollup_config(addr: *mut c_void, len: *mut u64) -> c_int {
    let data = HOST.lock().unwrap().rollup_config.as_slice().to_vec();
    store_data(addr, len, 0, &data);
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_store(key_addr: *const u8, value_addr: *const u8) -> c_int {
    let key = load_data_h256(key_addr);
    let value = load_data_h256(value_addr);
    HOST.lock()
        .unwrap()
        .state
        .update_raw(key, value)
        .expect("gw_store");
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_load(key_addr: *const u8, value_addr: *mut u8) -> c_int {
    let key = load_data_h256(key_addr);
    let val = HOST.lock().unwrap().state.get_raw(&key).expect("gw_load");
    store_h256(value_addr, &val);
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_set_return_data(addr: *const u8, len: u64) -> c_int {
    let buf = load_bytes(addr, len);
    HOST.lock().unwrap().run_result.return_data = buf;
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_create(
    script_addr: *const u8,
    script_len: u64,
    account_id_addr: *mut u32,
) -> c_int {
    let script_data = load_bytes(script_addr, script_len);
    let script = match Script::from_slice(&script_data[..]) {
        Ok(script) => script,
        Err(_err) => return GW_ERROR_INVALID_ACCOUNT_SCRIPT.into(),
    };
    let script_hash = script.hash();
    let mut host = HOST.lock().unwrap();
    // check exists
    match host.get_account_id_by_script_hash(&script_hash.into()) {
        Ok(Some(_)) => return GW_ERROR_DUPLICATED_SCRIPT_HASH.into(),
        Ok(None) => {}
        Err(_err) => {
            return ERROR;
        }
    }
    // TODO: valide script
    let id = match host.get_account_count() {
        Ok(id) => id,
        Err(_err) => {
            return ERROR;
        }
    };

    let account_nonce_key = build_account_field_key(id, GW_ACCOUNT_NONCE_TYPE);
    host.state
        .update_raw(account_nonce_key, H256::zero())
        .expect("account nonce key");
    host.run_result.write_data_hashes.insert(account_nonce_key);
    let account_script_hash_key = build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE);
    host.state
        .update_raw(account_script_hash_key, script_hash)
        .expect("account script hash key");
    host.run_result
        .write_data_hashes
        .insert(account_script_hash_key);
    // script hash to id
    let script_hash_to_id_value: H256 = {
        let mut buf: [u8; 32] = H256::from_u32(id).into();
        // the first 4 bytes is id, set exists flag(fifth byte) to 1
        buf[4] = 1;
        buf.into()
    };
    let script_hash_to_account_id_key = build_script_hash_to_account_id_key(&script_hash[..]);
    host.state
        .update_raw(script_hash_to_account_id_key, script_hash_to_id_value)
        .expect("write script hash to account id key");
    host.run_result
        .write_data_hashes
        .insert(script_hash_to_account_id_key);
    // insert script
    host.state.insert_script(script_hash, script);
    host.state
        .set_account_count(id + 1)
        .expect("set account count");

    let size_ptr = unsafe { account_id_addr.as_mut().expect("casting pointer") };
    *size_ptr = id;

    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_load_tx(addr: *mut c_void, len: *mut u64) -> c_int {
    if let Some(tx) = &HOST.lock().unwrap().tx {
        store_data(addr, len, 0, tx);
        SUCCESS
    } else {
        GW_ITEM_MISSING
    }
}

#[no_mangle]
pub extern "C" fn gw_load_block_info(addr: *mut c_void, len: *mut u64) -> c_int {
    let data = HOST.lock().unwrap().block_info.as_slice().to_vec();
    store_data(addr, len, 0, &data);
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_store_data(data_addr: *const u8, len: u64) -> c_int {
    let data = load_bytes(data_addr, len);
    let mut data_hash = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(data.as_ref());
    hasher.finalize(&mut data_hash);
    // insert data hash into SMT
    let data_hash_key = build_data_hash_key(&data_hash);
    let mut host = HOST.lock().unwrap();
    host.state
        .update_raw(data_hash_key, H256::one())
        .expect("gw store data");
    host.state.insert_data(data_hash.into(), data);

    host.run_result.write_data_hashes.insert(data_hash_key);
    host.run_result.write_data_hashes.insert(data_hash.into());
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_load_data(
    data_addr: &mut c_void,
    len: *mut u64,
    offset: u64,
    data_hash_addr: *const u8,
) -> c_int {
    let data_hash = load_data_h256(data_hash_addr);
    let host = &mut HOST.lock().unwrap();
    let data = match host.get_data(&data_hash) {
        Some(data) => data,
        None => return GW_ERROR_NOT_FOUND.into(),
    };
    store_data(data_addr, len, offset, data.as_ref());
    host.run_result.read_data_hashes.insert(data_hash);
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_load_account_script(
    script_addr: *mut c_void,
    len: *mut u64,
    offset: u64,
    account_id: u32,
) -> c_int {
    let host = &mut HOST.lock().unwrap();
    let script_hash = match host.get_script_hash(account_id) {
        Ok(id) => id,
        Err(_err) => {
            return ERROR;
        }
    };
    if script_hash.is_zero() {
        return GW_ERROR_ACCOUNT_NOT_FOUND.into();
    }
    let script = match host.get_script(&script_hash) {
        Some(script) => script,
        None => {
            return ERROR;
        }
    };
    store_data(script_addr, len, offset, script.as_slice());
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_get_block_hash(block_hash_addr: *mut u8, _number: u64) -> c_int {
    store_h256(block_hash_addr, &H256::from(BLOCK_HASH));
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_pay_fee(
    reg_addr_buf: *const u8,
    len: u64,
    _sudt_id: u32,
    amount_addr: *const u8,
) -> c_int {
    let payer_addr_bytes = load_bytes(reg_addr_buf, len);
    let _payer_addr = match RegistryAddress::from_slice(&payer_addr_bytes) {
        Some(addr) => addr,
        None => {
            return ERROR;
        }
    };
    let _amount = load_data_h256(amount_addr);
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_log(account_id: u32, service_flag: u8, len: u64, data: *const u8) -> c_int {
    let data = load_bytes(data, len);
    let log_item = LogItem::new_builder()
        .account_id(account_id.pack())
        .service_flag(service_flag.into())
        .data(Bytes::from(data).pack())
        .build();
    HOST.lock().unwrap().run_result.logs.push(log_item);
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_bn_add(
    output: &mut c_void,
    output_len: *mut u64,
    offset: u64,
    input: *const u8,
    input_size: u64,
) -> c_int {
    let buf = load_bytes(input, input_size);
    match bn::add(&buf) {
        Ok(data) => {
            store_data(output, output_len, offset, &data);
            SUCCESS
        }
        Err(err) => {
            let err_msg = format!("syscall SYS_BN_ADD error: {:?}", err.0);
            println!("{}", err_msg);
            return ERROR;
        }
    }
}

#[no_mangle]
pub extern "C" fn gw_bn_mul(
    output: &mut c_void,
    output_len: *mut u64,
    offset: u64,
    input: *const u8,
    input_size: u64,
) -> c_int {
    let buf = load_bytes(input, input_size);
    match bn::mul(&buf) {
        Ok(data) => {
            store_data(output, output_len, offset, &data);
            SUCCESS
        }
        Err(err) => {
            let err_msg = format!("syscall SYS_BN_ADD error: {:?}", err.0);
            println!("{}", err_msg);
            return ERROR;
        }
    }
}

#[no_mangle]
pub extern "C" fn gw_bn_pairing(
    output: &mut c_void,
    output_len: *mut u64,
    offset: u64,
    input: *const u8,
    input_size: u64,
) -> c_int {
    let buf = load_bytes(input, input_size);
    match bn::pairing(&buf) {
        Ok(data) => {
            store_data(output, output_len, offset, &data);
            SUCCESS
        }
        Err(err) => {
            let err_msg = format!("syscall SYS_BN_ADD error: {:?}", err.0);
            println!("{}", err_msg);
            return ERROR;
        }
    }
}

#[no_mangle]
pub extern "C" fn gw_snapshot(snapshot_id: *mut u32) -> c_int {
    let host = &mut HOST.lock().unwrap();
    let id = host.state.snapshot() as u32;

    let snapshot_id_ptr = unsafe { snapshot_id.as_mut().expect("casting pointer") };
    *snapshot_id_ptr = id;
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_revert(snapshot_id: u32) -> c_int {
    let host = &mut HOST.lock().unwrap();
    host.state
        .revert(snapshot_id as usize)
        .expect("revert failed");
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_check_sudt_addr_permission(_addr: *const u8) -> c_int {
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_reset() -> c_int {
    let host = &mut HOST.lock().unwrap();

    let store = Store::open_tmp().expect("open store");
    let snapshot = store.get_snapshot();
    let mut state = new_dummy_state(snapshot);
    // setup CKB simple UDT contract
    let ckb_sudt_script = build_l2_sudt_script(CKB_SUDT_SCRIPT_ARGS);
    let ckb_sudt_id = state.create_account_from_script(ckb_sudt_script).expect("");
    // creator id
    let mut args = [0u8; 36];
    args[0..32].copy_from_slice(&ROLLUP_SCRIPT_HASH);
    args[32..36].copy_from_slice(&ckb_sudt_id.to_le_bytes()[..]);
    let creator_script = Script::new_builder()
        .code_hash(POLYJUICE_PROGRAM_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(args.to_vec().pack())
        .build();
    let _creator_account_id = state
        .create_account_from_script(creator_script)
        .expect("create creator_account");
    let eth_address: [u8; 20] = hex::decode(BLOCK_PRODUCER_ETH_ADDRESSS)
        .expect("decode hex eth_address")
        .try_into()
        .unwrap();
    let mut script_args = Vec::with_capacity(32 + 20);
    script_args.extend(&ROLLUP_SCRIPT_HASH);
    script_args.extend(&eth_address[..]);
    let block_producer_script = Script::new_builder()
        .args(Bytes::from(script_args).pack())
        .code_hash(ETH_ACCOUNT_LOCK_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let block_producer_script_hash = block_producer_script.hash().into();
    let _block_producer_id = state
        .create_account_from_script(block_producer_script)
        .expect("create_block_producer");

    let address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec());
    state
        .mapping_registry_address_to_script_hash(address, block_producer_script_hash)
        .expect("set mapping");
    host.state = state;
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_set_tx(addr: *const u8, len: u64) -> c_int {
    let slice = unsafe { std::slice::from_raw_parts(addr, len as usize) };
    let mut buf = Vec::with_capacity(len as usize);
    buf.extend_from_slice(slice);
    HOST.lock().unwrap().tx = Some(buf);
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_create_eoa_account(
    eth_address: *const u8,
    mint_ckb: *const u8,
    account_id_addr: *mut u32,
) -> c_int {
    let slice = unsafe { std::slice::from_raw_parts(eth_address, 20) };
    let mut eth_address = [0u8; 20];
    eth_address.copy_from_slice(slice);
    let script = build_eth_l2_script(&eth_address);
    let script_hash = script.hash();
    let state = &mut HOST.lock().unwrap().state;
    let account_id = match state.create_account_from_script(script) {
        Ok(account_id) => account_id,
        Err(_err) => {
            return ERROR;
        }
    };
    let address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec());
    if let Err(_err) =
        state.mapping_registry_address_to_script_hash(address.clone(), script_hash.into())
    {
        return ERROR;
    }

    let slice = unsafe { std::slice::from_raw_parts(mint_ckb, 16) };
    let mut mint_array = [0u8; 16];
    mint_array.copy_from_slice(slice);
    let mint_ckb = U256::from(u128::from_le_bytes(mint_array));

    if let Err(_err) = state.mint_sudt(CKB_SUDT_ACCOUNT_ID, &address, mint_ckb) {
        return ERROR;
    }

    let size_ptr = unsafe { account_id_addr.as_mut().expect("casting pointer") };
    *size_ptr = account_id;
    SUCCESS
}

#[no_mangle]
pub extern "C" fn gw_create_contract_account(
    eth_address: *const u8,
    mint: *const u8,
    code: *const u8,
    code_size: u64,
    account_id_addr: *mut u32,
) -> c_int {
    let mut new_script_args = vec![0u8; 32 + 4 + 20];
    new_script_args[0..32].copy_from_slice(&ROLLUP_SCRIPT_HASH);
    new_script_args[32..36].copy_from_slice(&CREATOR_ACCOUNT_ID.to_le_bytes()[..]);
    let slice = unsafe { std::slice::from_raw_parts(eth_address, 20) };
    let mut eth_address = [0u8; 20];
    eth_address.copy_from_slice(slice);
    new_script_args[36..36 + 20].copy_from_slice(&eth_address);

    let script = Script::new_builder()
        .code_hash(POLYJUICE_PROGRAM_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(new_script_args.pack())
        .build();
    let script_hash = script.hash();
    let state = &mut HOST.lock().unwrap().state;
    let account_id = match state.create_account_from_script(script) {
        Ok(account_id) => account_id,
        Err(_err) => {
            return ERROR;
        }
    };
    let address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec());
    if let Err(_err) =
        state.mapping_registry_address_to_script_hash(address.clone(), script_hash.into())
    {
        return ERROR;
    }

    let slice = unsafe { std::slice::from_raw_parts(mint, 16) };
    let mut mint_array = [0u8; 16];
    mint_array.copy_from_slice(slice);
    let mint_ckb = U256::from(u128::from_le_bytes(mint_array));
    if let Err(_err) = state.mint_sudt(CKB_SUDT_ACCOUNT_ID, &address, mint_ckb) {
        return ERROR;
    }

    //set code
    //build polyjuice account key
    let mut key = [0u8; 32];
    key[0..4].copy_from_slice(&account_id.to_le_bytes());
    key[4] = 0xFF;
    key[5] = 0x01;

    let code_slice = unsafe { std::slice::from_raw_parts(code, code_size as usize) };
    let mut data_hash = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(code_slice);
    hasher.finalize(&mut data_hash);
    let data_hash = build_account_key(account_id, &data_hash);
    //sys_store key - data hash
    state
        .update_value(account_id, &key, data_hash)
        .expect("update value");
    let data_hash = state.get_value(account_id, &key).expect("get value");

    let data_hash_key = build_data_hash_key(data_hash.as_slice());
    state
        .update_raw(data_hash_key, H256::one())
        .expect("update raw");
    //sys_store_data data hash - data
    let code = Bytes::copy_from_slice(code_slice);
    state.insert_data(data_hash, code);

    let size_ptr = unsafe { account_id_addr.as_mut().expect("casting pointer") };
    *size_ptr = account_id;
    SUCCESS
}

fn store_data(ptr: *mut c_void, len: *mut u64, offset: u64, data: &[u8]) {
    let size_ptr = unsafe { len.as_mut().expect("casting pointer") };
    let size = *size_ptr;
    let buffer = unsafe { std::slice::from_raw_parts_mut(ptr as *mut u8, size as usize) };
    let data_len = data.len() as u64;
    let offset = std::cmp::min(data_len, offset);
    let full_size = data_len - offset;
    let real_size = std::cmp::min(size, full_size);
    *size_ptr = full_size;
    buffer[..real_size as usize]
        .copy_from_slice(&data[offset as usize..(offset + real_size) as usize]);
}

fn store_h256(ptr: *mut u8, h256: &H256) {
    let buffer = unsafe { std::slice::from_raw_parts_mut(ptr, 32) };
    buffer.copy_from_slice(h256.as_slice());
}

fn load_data_h256(addr: *const u8) -> H256 {
    let mut arr = [0u8; 32];
    let slice = unsafe { std::slice::from_raw_parts(addr, 32) };
    arr.copy_from_slice(slice);
    arr.into()
}

fn load_bytes(addr: *const u8, len: u64) -> Bytes {
    let slice = unsafe { std::slice::from_raw_parts(addr, len as usize) };
    slice.into()
}

pub fn build_eth_l2_script(args: &[u8; 20]) -> Script {
    let mut script_args = Vec::with_capacity(32 + 20);
    script_args.extend(&ROLLUP_SCRIPT_HASH);
    script_args.extend(&args[..]);
    Script::new_builder()
        .args(Bytes::from(script_args).pack())
        .code_hash(ETH_ACCOUNT_LOCK_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .build()
}

pub fn build_l2_sudt_script(args: [u8; 32]) -> Script {
    let mut script_args = Vec::with_capacity(64);
    script_args.extend(&ROLLUP_SCRIPT_HASH);
    script_args.extend(&args[..]);
    Script::new_builder()
        .args(Bytes::from(script_args).pack())
        .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .build()
}
