use gw_common::registry_address::RegistryAddress;
pub use gw_common::{
    blake2b::new_blake2b,
    state::{build_data_hash_key, State},
    CKB_SUDT_SCRIPT_ARGS,
};
use gw_types::h256::*;

use gw_config::{BackendConfig, BackendForkConfig, BackendType, ForkConfig, Resource};
pub use gw_generator::{
    account_lock_manage::{secp256k1::Secp256k1Eth, AccountLockManage},
    backend_manage::{Backend, BackendManage},
    traits::StateExt,
    Generator,
};
pub use gw_store::{chain_view::ChainView, Store};
use gw_store::{schema::*, traits::kv_store::KVStoreWrite};
use gw_store::{state::traits::JournalDB, traits::chain_store::ChainStore};
use gw_traits::CodeStore;
use gw_types::packed::{ETHAddrRegArgs, ETHAddrRegArgsUnion};
use gw_types::{
    bytes::Bytes,
    core::{AllowedContractType, AllowedEoaType, ScriptHashType},
    offchain::{CycleMeter, RunResult},
    packed::{
        AllowedTypeHash, BatchSetMapping, BlockInfo, Fee, LogItem, RawL2Transaction, RollupConfig,
        Script, SetMapping, Uint64,
    },
    prelude::*,
    U256,
};
use gw_utils::{checksum::file_checksum, RollupContext};
use rlp::RlpStream;
use std::{convert::TryInto, fs, io::Read, path::PathBuf, sync::Arc};

pub use gw_common::builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID, RESERVED_ACCOUNT_ID};

use crate::{new_dummy_state, DummyState};
pub const CREATOR_ACCOUNT_ID: u32 = 3;
pub const CHAIN_ID: u64 = 202204;

pub const L2TX_MAX_CYCLES: Option<u64> = Some(7000_0000);

// meta contract
pub const META_VALIDATOR_PATH: &str = "../build/godwoken-scripts/meta-contract-validator";
pub const META_GENERATOR_PATH: &str = "../build/godwoken-scripts/meta-contract-generator";
pub const META_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [0xa1u8; 32];
// simple UDT
pub const SUDT_VALIDATOR_PATH: &str = "../build/godwoken-scripts/sudt-validator";
pub const SUDT_GENERATOR_PATH: &str = "../build/godwoken-scripts/sudt-generator";
pub const SUDT_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [0xa2u8; 32];
pub const SECP_DATA: &[u8] = include_bytes!("../../build/secp256k1_data");

// polyjuice
pub const POLYJUICE_GENERATOR_NAME: &str = "../build/generator";
pub const POLYJUICE_VALIDATOR_NAME: &str = "../build/validator";
// ETH Address Registry
pub const ETH_ADDRESS_REGISTRY_GENERATOR_NAME: &str =
    "../build/godwoken-scripts/eth-addr-reg-generator";
pub const ETH_ADDRESS_REGISTRY_VALIDATOR_NAME: &str =
    "../build/godwoken-scripts/eth-addr-reg-validator";

pub const ROLLUP_SCRIPT_HASH: [u8; 32] = [0xa9u8; 32];
pub const ETH_ACCOUNT_LOCK_CODE_HASH: [u8; 32] = [0xaau8; 32];
pub const SECP_LOCK_CODE_HASH: [u8; 32] = [0xbbu8; 32];

pub const GW_LOG_SUDT_TRANSFER: u8 = 0x0;
pub const GW_LOG_SUDT_PAY_FEE: u8 = 0x1;
pub const GW_LOG_POLYJUICE_SYSTEM: u8 = 0x2;
pub const GW_LOG_POLYJUICE_USER: u8 = 0x3;

// pub const FATAL_POLYJUICE: i8 = -50;
pub const ERROR_REVERT: i8 = 2;
pub const FATAL_PRECOMPILED_CONTRACTS: i8 = -51;

pub(crate) const SUDT_ERC20_PROXY_USER_DEFINED_DECIMALS_CODE: &str =
    include_str!("../../solidity/erc20/SudtERC20Proxy_UserDefinedDecimals.bin");

pub fn load_program(program_name: &str) -> Bytes {
    let mut buf = Vec::new();
    let mut path = PathBuf::new();
    path.push(program_name);
    let mut f = fs::File::open(&path).unwrap_or_else(|_| panic!("load program {}", program_name));
    f.read_to_end(&mut buf).expect("read program");
    Bytes::from(buf.to_vec())
}

lazy_static::lazy_static! {
    pub static ref SECP_DATA_HASH: H256 = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(SECP_DATA);
        hasher.finalize(&mut buf);
        buf.into()
    };

    pub static ref POLYJUICE_GENERATOR_PROGRAM: Bytes
        = load_program(POLYJUICE_GENERATOR_NAME);
    pub static ref POLYJUICE_VALIDATOR_PROGRAM: Bytes
        = load_program(POLYJUICE_VALIDATOR_NAME);
    pub static ref POLYJUICE_PROGRAM_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&POLYJUICE_VALIDATOR_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };

    pub static ref ETH_ADDRESS_REGISTRY_GENERATOR_PROGRAM: Bytes
        = load_program(ETH_ADDRESS_REGISTRY_GENERATOR_NAME);
    pub static ref ETH_ADDRESS_REGISTRY_VALIDATOR_PROGRAM: Bytes
        = load_program(ETH_ADDRESS_REGISTRY_VALIDATOR_NAME);
    pub static ref ETH_ADDRESS_REGISTRY_PROGRAM_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ETH_ADDRESS_REGISTRY_VALIDATOR_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
}

#[derive(Debug, Clone)]
pub enum Log {
    SudtTransfer {
        sudt_id: u32,
        from_addr: RegistryAddress,
        to_addr: RegistryAddress,
        amount: U256,
    },
    SudtPayFee {
        sudt_id: u32,
        from_addr: RegistryAddress,
        block_producer_addr: RegistryAddress,
        amount: U256,
    },
    PolyjuiceSystem {
        gas_used: u64,
        cumulative_gas_used: u64,
        created_address: [u8; 20],
        status_code: u32,
    },
    PolyjuiceUser {
        address: [u8; 20],
        data: Vec<u8>,
        topics: Vec<H256>,
    },
}

fn parse_sudt_log_data(data: &[u8]) -> (RegistryAddress, RegistryAddress, U256) {
    let from_addr = RegistryAddress::from_slice(&data[0..28]).expect("parse from_addr");
    let to_addr = RegistryAddress::from_slice(&data[28..56]).expect("parse to_addr");

    let mut u256_bytes = [0u8; 32];
    u256_bytes.copy_from_slice(&data[56..56 + 32]);
    let amount = U256::from_little_endian(&u256_bytes);
    (from_addr, to_addr, amount)
}

pub fn parse_log(item: &LogItem) -> Log {
    let service_flag: u8 = item.service_flag().into();
    let raw_data = item.data().raw_data();
    let data = raw_data.as_ref();
    match service_flag {
        GW_LOG_SUDT_TRANSFER => {
            let sudt_id: u32 = item.account_id().unpack();
            if data.len() != (28 + 28 + 32) {
                panic!("Invalid data length: {}", data.len());
            }
            let (from_addr, to_addr, amount) = parse_sudt_log_data(data);
            Log::SudtTransfer {
                sudt_id,
                from_addr,
                to_addr,
                amount,
            }
        }
        GW_LOG_SUDT_PAY_FEE => {
            let sudt_id: u32 = item.account_id().unpack();
            if data.len() != (28 + 28 + 32) {
                panic!("Invalid data length: {}", data.len());
            }
            let (from_addr, block_producer_addr, amount) = parse_sudt_log_data(data);
            Log::SudtPayFee {
                sudt_id,
                from_addr,
                block_producer_addr,
                amount,
            }
        }
        GW_LOG_POLYJUICE_SYSTEM => {
            if data.len() != (8 + 8 + 20 + 4) {
                panic!("invalid system log raw data length: {}", data.len());
            }

            let mut u64_bytes = [0u8; 8];
            u64_bytes.copy_from_slice(&data[0..8]);
            let gas_used = u64::from_le_bytes(u64_bytes);
            u64_bytes.copy_from_slice(&data[8..16]);
            let cumulative_gas_used = u64::from_le_bytes(u64_bytes);

            let mut created_address = [0u8; 20];
            created_address.copy_from_slice(&data[16..36]);
            let mut u32_bytes = [0u8; 4];
            u32_bytes.copy_from_slice(&data[36..40]);
            let status_code = u32::from_le_bytes(u32_bytes);
            Log::PolyjuiceSystem {
                gas_used,
                cumulative_gas_used,
                created_address,
                status_code,
            }
        }
        GW_LOG_POLYJUICE_USER => {
            let mut offset: usize = 0;
            let mut address = [0u8; 20];
            address.copy_from_slice(&data[offset..offset + 20]);
            offset += 20;
            let mut data_size_bytes = [0u8; 4];
            data_size_bytes.copy_from_slice(&data[offset..offset + 4]);
            offset += 4;
            let data_size: u32 = u32::from_le_bytes(data_size_bytes);
            let mut log_data = vec![0u8; data_size as usize];
            log_data.copy_from_slice(&data[offset..offset + (data_size as usize)]);
            offset += data_size as usize;
            println!("data_size: {}", data_size);

            let mut topics_count_bytes = [0u8; 4];
            topics_count_bytes.copy_from_slice(&data[offset..offset + 4]);
            offset += 4;
            let topics_count: u32 = u32::from_le_bytes(topics_count_bytes);
            let mut topics = Vec::new();
            println!("topics_count: {}", topics_count);
            for _ in 0..topics_count {
                let mut topic = [0u8; 32];
                topic.copy_from_slice(&data[offset..offset + 32]);
                offset += 32;
                topics.push(topic);
            }
            if offset != data.len() {
                panic!(
                    "Too many bytes for polyjuice user log data: offset={}, data.len()={}",
                    offset,
                    data.len()
                );
            }
            Log::PolyjuiceUser {
                address,
                data: log_data,
                topics,
            }
        }
        _ => {
            panic!("invalid log service flag: {}", service_flag);
        }
    }
}

pub fn new_block_info(block_producer: RegistryAddress, number: u64, timestamp: u64) -> BlockInfo {
    BlockInfo::new_builder()
        .block_producer(Bytes::from(block_producer.to_bytes()).pack())
        .number(number.pack())
        .timestamp(timestamp.pack())
        .build()
}

// pub(crate) fn contract_id_to_short_script_hash(
//     state: &DummyState,
//     id: u32,
//     ethabi: bool,
// ) -> Vec<u8> {
//     let offset = if ethabi { 12 } else { 0 };
//     let mut data = vec![0u8; offset + 20];
//     let account_script_hash = state.get_script_hash(id).unwrap();
//     data[offset..offset + 20].copy_from_slice(&account_script_hash.as_slice()[0..20]);
//     data
// }

pub(crate) fn eth_addr_to_ethabi_addr(eth_addr: &[u8; 20]) -> [u8; 32] {
    let mut ethabi_addr = [0; 32];
    ethabi_addr[12..32].copy_from_slice(eth_addr);
    ethabi_addr
}

pub fn new_contract_account_script_with_nonce(from_addr: &[u8; 20], from_nonce: u32) -> Script {
    let mut stream = RlpStream::new_list(2);
    stream.append(&from_addr.to_vec());
    stream.append(&from_nonce);
    // println!(
    //     "rlp data of (eoa_address + nonce): {}",
    //     hex::encode(stream.as_raw())
    // );
    let data_hash = tiny_keccak::keccak256(stream.as_raw());

    let mut new_script_args = vec![0u8; 32 + 4 + 20];
    new_script_args[0..32].copy_from_slice(&ROLLUP_SCRIPT_HASH);
    new_script_args[32..36].copy_from_slice(&CREATOR_ACCOUNT_ID.to_le_bytes()[..]);
    new_script_args[36..36 + 20].copy_from_slice(&data_hash[12..]);
    // println!("eth_address: {:?}", &data_hash[12..32]);

    Script::new_builder()
        .code_hash(POLYJUICE_PROGRAM_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(new_script_args.pack())
        .build()
}

pub fn new_contract_account_script(
    state: &DummyState,
    from_id: u32,
    from_eth_address: &[u8; 20],
    current_nonce: bool,
) -> Script {
    let mut from_nonce = state.get_nonce(from_id).unwrap();
    if !current_nonce {
        from_nonce -= 1;
    }
    new_contract_account_script_with_nonce(from_eth_address, from_nonce)
}

pub(crate) fn contract_script_to_eth_addr(script: &Script, ethabi: bool) -> Vec<u8> {
    let offset = if ethabi { 12 } else { 0 };
    let mut eth_addr = vec![0u8; offset + 20];
    eth_addr[offset..].copy_from_slice(&script.args().raw_data().as_ref()[36..56]);
    eth_addr
}

#[derive(Default, Debug)]
pub struct PolyjuiceArgsBuilder {
    is_create: bool,
    gas_limit: u64,
    gas_price: u128,
    value: u128,
    input: Vec<u8>,
    to_address: Option<[u8; 20]>,
}

impl PolyjuiceArgsBuilder {
    pub fn do_create(mut self, value: bool) -> Self {
        self.is_create = value;
        self
    }
    pub fn gas_limit(mut self, value: u64) -> Self {
        self.gas_limit = value;
        self
    }
    pub fn gas_price(mut self, value: u128) -> Self {
        self.gas_price = value;
        self
    }
    pub fn value(mut self, new_value: u128) -> Self {
        self.value = new_value;
        self
    }
    pub fn input(mut self, value: &[u8]) -> Self {
        self.input = value.to_vec();
        self
    }

    pub fn to_address(mut self, to_address: [u8; 20]) -> Self {
        self.to_address = Some(to_address);
        self
    }
    pub fn build(self) -> Vec<u8> {
        let mut output: Vec<u8> = vec![0u8; 52];
        let call_kind: u8 = if self.is_create { 3 } else { 0 };
        output[0..8].copy_from_slice(&[0xff, 0xff, 0xff, b'P', b'O', b'L', b'Y', call_kind][..]);
        output[8..16].copy_from_slice(&self.gas_limit.to_le_bytes()[..]);
        output[16..32].copy_from_slice(&self.gas_price.to_le_bytes()[..]);
        output[32..48].copy_from_slice(&self.value.to_le_bytes()[..]);
        output[48..52].copy_from_slice(&(self.input.len() as u32).to_le_bytes()[..]);
        output.extend(self.input);
        if let Some(to_address) = self.to_address {
            output.extend_from_slice(&to_address);
        }
        output
    }
}

pub fn setup() -> (Store, DummyState, Generator) {
    // If you want to watch the [contract debug] logs in Polyjuice,
    // please change the log level from `info` to `debug`.
    // then run `cargo test -- [test_filter] --nocapture`,
    // or run `RUST_LOG=gw=debug cargo test -- [test_filter] --nocapture` directly
    let _ = env_logger::try_init_from_env(env_logger::Env::default().default_filter_or("info"));

    let store = Store::open_tmp().unwrap();
    let snapshot = store.get_snapshot();
    let mut state = new_dummy_state(snapshot);

    let meta_script = Script::new_builder()
        .code_hash(META_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let reserved_id = state
        .create_account_from_script(meta_script)
        .expect("create meta_account");
    assert_eq!(
        reserved_id, RESERVED_ACCOUNT_ID,
        "reserved account id must be zero"
    );

    // setup CKB simple UDT contract
    let ckb_sudt_script = build_l2_sudt_script(CKB_SUDT_SCRIPT_ARGS);
    let ckb_sudt_id = state
        .create_account_from_script(ckb_sudt_script)
        .expect("create CKB simple UDT contract account");
    assert_eq!(
        ckb_sudt_id, CKB_SUDT_ACCOUNT_ID,
        "ckb simple UDT account id"
    );

    // create `ETH Address Registry` layer2 contract account
    let eth_addr_reg_script = Script::new_builder()
        .code_hash(ETH_ADDRESS_REGISTRY_PROGRAM_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(ROLLUP_SCRIPT_HASH.to_vec().pack())
        .build();
    let eth_addr_reg_account_id = state
        .create_account_from_script(eth_addr_reg_script)
        .expect("create `ETH Address Registry` layer2 contract");
    assert_eq!(eth_addr_reg_account_id, ETH_REGISTRY_ACCOUNT_ID);

    let mut args = [0u8; 36];
    args[0..32].copy_from_slice(&ROLLUP_SCRIPT_HASH);
    args[32..36].copy_from_slice(&ckb_sudt_id.to_le_bytes()[..]);
    let creator_script = Script::new_builder()
        .code_hash(POLYJUICE_PROGRAM_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(args.to_vec().pack())
        .build();
    let creator_account_id = state
        .create_account_from_script(creator_script)
        .expect("create creator_account");
    assert_eq!(creator_account_id, CREATOR_ACCOUNT_ID);

    state.insert_data(*SECP_DATA_HASH, Bytes::from(SECP_DATA));
    state
        .update_raw(build_data_hash_key(&*SECP_DATA_HASH), H256::one())
        .expect("update secp data key");

    // ==== Build generator
    let fork_configs = vec![BackendForkConfig {
        fork_height: 0,
        sudt_proxy: None,
        backends: vec![
            BackendConfig {
                backend_type: BackendType::Meta,
                generator: Resource::file_system(META_GENERATOR_PATH.into()),
                generator_checksum: file_checksum(META_GENERATOR_PATH).unwrap().into(),
                validator_script_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
            },
            BackendConfig {
                backend_type: BackendType::Sudt,
                generator: Resource::file_system(SUDT_GENERATOR_PATH.into()),
                generator_checksum: file_checksum(SUDT_GENERATOR_PATH).unwrap().into(),
                validator_script_type_hash: SUDT_VALIDATOR_SCRIPT_TYPE_HASH.into(),
            },
            BackendConfig {
                backend_type: BackendType::Polyjuice,
                generator: Resource::file_system(POLYJUICE_GENERATOR_NAME.into()),
                generator_checksum: file_checksum(POLYJUICE_GENERATOR_NAME).unwrap().into(),
                validator_script_type_hash: (*POLYJUICE_PROGRAM_CODE_HASH).into(),
            },
            BackendConfig {
                backend_type: BackendType::EthAddrReg,
                generator: Resource::file_system(ETH_ADDRESS_REGISTRY_GENERATOR_NAME.into()),
                generator_checksum: file_checksum(ETH_ADDRESS_REGISTRY_GENERATOR_NAME)
                    .unwrap()
                    .into(),
                validator_script_type_hash: (*ETH_ADDRESS_REGISTRY_PROGRAM_CODE_HASH).into(),
            },
        ],
    }];
    let backend_manage = BackendManage::from_config(fork_configs.clone()).expect("default backend");
    // NOTICE in this test we won't need SUM validator
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(
        SECP_LOCK_CODE_HASH.into(),
        Arc::new(Secp256k1Eth::default()),
    );
    let rollup_config = RollupConfig::new_builder()
        .chain_id(CHAIN_ID.pack())
        .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .allowed_contract_type_hashes(
            vec![
                AllowedTypeHash::new(AllowedContractType::Meta, META_VALIDATOR_SCRIPT_TYPE_HASH),
                AllowedTypeHash::new(AllowedContractType::Sudt, SUDT_VALIDATOR_SCRIPT_TYPE_HASH),
                AllowedTypeHash::new(AllowedContractType::Polyjuice, *POLYJUICE_PROGRAM_CODE_HASH),
                AllowedTypeHash::new(
                    AllowedContractType::EthAddrReg,
                    *ETH_ADDRESS_REGISTRY_PROGRAM_CODE_HASH,
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
    let fork_config = ForkConfig {
        increase_max_l2_tx_cycles_to_500m: None,
        upgrade_global_state_version_to_v2: None,
        backend_forks: fork_configs,
        ..Default::default()
    };
    let rollup_context = RollupContext {
        rollup_script_hash: ROLLUP_SCRIPT_HASH.into(),
        rollup_config,
        fork_config,
    };
    let generator = Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_context,
        Default::default(),
    );

    let mut tx = store.begin_transaction();
    let tip_block_number: Uint64 = 8.pack();
    let tip_block_hash = [8u8; 32];
    tx.insert_raw(COLUMN_META, META_TIP_BLOCK_HASH_KEY, &tip_block_hash[..])
        .unwrap();
    tx.insert_raw(
        COLUMN_INDEX,
        tip_block_number.as_slice(),
        &tip_block_hash[..],
    )
    .unwrap();
    tx.insert_raw(
        COLUMN_INDEX,
        &tip_block_hash[..],
        tip_block_number.as_slice(),
    )
    .unwrap();
    tx.commit().unwrap();

    (store, state, generator)
}

pub fn deploy(
    generator: &Generator,
    store: &Store,
    state: &mut DummyState,
    creator_account_id: u32,
    from_id: u32,
    init_code: &str,
    gas_limit: u64,
    value: u128,
    block_producer: RegistryAddress,
    block_number: u64,
) -> RunResult {
    let block_info = new_block_info(block_producer, block_number, block_number);
    let input = hex::decode(init_code).unwrap();
    let args = PolyjuiceArgsBuilder::default()
        .do_create(true)
        .gas_limit(gas_limit)
        .gas_price(1)
        .value(value)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(creator_account_id.pack())
        .args(Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = db.get_tip_block_hash().unwrap();
    let run_result = generator
        .execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("deploy Polyjuice contract");
    state.finalise().expect("update state");
    // println!("[deploy contract] used cycles: {}", run_result.used_cycles);
    run_result
}

/// https://eips.ethereum.org/EIPS/eip-1014#specification
pub fn compute_create2_script(
    sender_contract_addr: &[u8],
    create2_salt: &[u8],
    init_code: &[u8],
) -> Script {
    assert_eq!(create2_salt.len(), 32);

    let init_code_hash = tiny_keccak::keccak256(init_code);
    let mut data = [0u8; 1 + 20 + 32 + 32];
    data[0] = 0xff;
    data[1..1 + 20].copy_from_slice(sender_contract_addr);
    data[1 + 20..1 + 20 + 32].copy_from_slice(create2_salt);
    data[1 + 20 + 32..1 + 20 + 32 + 32].copy_from_slice(&init_code_hash[..]);
    let data_hash = tiny_keccak::keccak256(&data);

    let mut script_args = vec![0u8; 32 + 4 + 20];
    script_args[0..32].copy_from_slice(&ROLLUP_SCRIPT_HASH[..]);
    script_args[32..32 + 4].copy_from_slice(&CREATOR_ACCOUNT_ID.to_le_bytes()[..]);
    script_args[32 + 4..32 + 4 + 20].copy_from_slice(&data_hash[12..]);

    println!(
        "[compute_create2_script] init_code: {}",
        hex::encode(init_code)
    );
    println!("create2_script_args: {}", hex::encode(&script_args[..]));
    Script::new_builder()
        .code_hash(POLYJUICE_PROGRAM_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(script_args.pack())
        .build()
}

#[derive(Debug, Clone)]
pub struct MockContractInfo {
    pub eth_addr: Vec<u8>,
    pub eth_abi_addr: Vec<u8>,
    pub script_hash: H256,
    pub reg_addr: RegistryAddress,
}

impl MockContractInfo {
    pub fn create(eth_addr: &[u8; 20], nonce: u32) -> Self {
        let contract_script = new_contract_account_script_with_nonce(eth_addr, nonce);
        let contract_eth_addr = contract_script_to_eth_addr(&contract_script, false);
        let contract_eth_abi_addr = contract_script_to_eth_addr(&contract_script, true);
        let reg_addr = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, contract_eth_addr.clone());
        Self {
            eth_addr: contract_eth_addr,
            eth_abi_addr: contract_eth_abi_addr,
            script_hash: contract_script.hash(),
            reg_addr,
        }
    }
}

pub fn simple_storage_get(
    store: &Store,
    state: &mut DummyState,
    generator: &Generator,
    block_number: u64,
    from_id: u32,
    ss_account_id: u32,
) -> RunResult {
    let eth_addr = [0x99u8; 20];
    let addr = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_addr.to_vec());
    let block_info = new_block_info(addr, block_number, block_number);
    let input = hex::decode("6d4ce63c").unwrap();
    let args = PolyjuiceArgsBuilder::default()
        .gas_limit(30000)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(ss_account_id.pack())
        .args(Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = db.get_tip_block_hash().unwrap();
    let run_result = generator
        .execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("execute_transaction");

    run_result
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

pub(crate) fn create_block_producer(state: &mut DummyState) -> RegistryAddress {
    // This eth_address is hardcoded in src/test_cases/evm-contracts/BlockInfo.sol
    let eth_address: [u8; 20] = hex::decode("a1ad227Ad369f593B5f3d0Cc934A681a50811CB2")
        .expect("decode hex eth_address")
        .try_into()
        .unwrap();
    let block_producer_script = build_eth_l2_script(&eth_address);
    let block_producer_script_hash = block_producer_script.hash();
    let _block_producer_id = state
        .create_account_from_script(block_producer_script)
        .expect("create_block_producer");
    register_eoa_account(state, &eth_address, &block_producer_script_hash);
    RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec())
}

pub(crate) fn create_eth_eoa_account(
    state: &mut DummyState,
    eth_address: &[u8; 20],
    mint_ckb: U256,
) -> (u32, [u8; 32]) {
    let script = build_eth_l2_script(eth_address);
    let script_hash = script.hash();
    let account_id = state.create_account_from_script(script).unwrap();
    register_eoa_account(state, eth_address, &script_hash);
    let address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec());
    state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &address, mint_ckb)
        .unwrap();
    (account_id, script_hash)
}

pub(crate) fn check_cycles(l2_tx_label: &str, cycles: CycleMeter, warning_cycles: u64) {
    if POLYJUICE_GENERATOR_NAME.contains("_log") {
        return; // disable cycles check
    }

    let all_cycles = cycles.execution + cycles.r#virtual;

    if all_cycles > warning_cycles {
        let overflow_cycles = all_cycles - warning_cycles;
        println!(
            "[{}] overflow_cycles: {}({}%)",
            l2_tx_label,
            overflow_cycles,
            overflow_cycles * 100 / warning_cycles
        );
    }

    println!(
        "[check_cycles] {l2_tx_label}'s execution_cycles({}) + virtual_cycles({}) = {}",
        cycles.execution, cycles.r#virtual, all_cycles
    );
    assert!(
        all_cycles < warning_cycles,
        "[Warning(cycles: {}): {} used too many cycles({})]",
        warning_cycles,
        l2_tx_label,
        all_cycles
    );
}

/// update eth_address_registry by state.update_raw(...)
pub(crate) fn register_eoa_account(
    state: &mut DummyState,
    eth_address: &[u8; 20],
    script_hash: &[u8; 32],
) {
    let address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec());
    state
        .mapping_registry_address_to_script_hash(address, (*script_hash).into())
        .expect("map reg addr to script hash");
}

pub enum SetMappingArgs {
    One(H256),
    Batch(Vec<H256>),
}

/// Set two-ways mappings between `eth_address` and `gw_script_hash`
/// by `ETH Address Registry` layer2 contract
pub(crate) fn eth_address_regiser(
    store: &Store,
    state: &mut DummyState,
    generator: &Generator,
    from_id: u32,
    block_info: BlockInfo,
    set_mapping_args: SetMappingArgs,
) -> anyhow::Result<RunResult> {
    let args = match set_mapping_args {
        SetMappingArgs::One(gw_script_hash) => {
            let fee = Fee::new_builder()
                .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
                .amount(1000u128.pack())
                .build();
            let set_mapping = SetMapping::new_builder()
                .fee(fee)
                .gw_script_hash(gw_script_hash.pack())
                .build();
            let args = ETHAddrRegArgs::new_builder()
                .set(ETHAddrRegArgsUnion::SetMapping(set_mapping))
                .build();
            args.as_bytes().pack()
        }
        SetMappingArgs::Batch(gw_script_hashes) => {
            let fee = Fee::new_builder()
                .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
                .amount(1000u128.pack())
                .build();
            let batch_set_mapping = BatchSetMapping::new_builder()
                .fee(fee)
                .gw_script_hashes(gw_script_hashes.pack())
                .build();
            let args = ETHAddrRegArgs::new_builder()
                .set(ETHAddrRegArgsUnion::BatchSetMapping(batch_set_mapping))
                .build();
            args.as_bytes().pack()
        }
    };

    let raw_l2tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(ETH_REGISTRY_ACCOUNT_ID.pack())
        .args(args)
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = store.get_tip_block_hash().unwrap();
    generator.execute_transaction(
        &ChainView::new(&db, tip_block_hash),
        state,
        &block_info,
        &raw_l2tx,
        L2TX_MAX_CYCLES,
        None,
    )
}

pub(crate) fn print_gas_used(operation: &str, logs: &Vec<LogItem>) {
    let mut gas_used: Option<u64> = None;
    for log in logs {
        gas_used = match parse_log(log) {
            crate::helper::Log::PolyjuiceSystem {
                gas_used,
                cumulative_gas_used: _,
                created_address: _,
                status_code: _,
            } => Some(gas_used),
            _ => None,
        };
        if gas_used.is_some() {
            break;
        }
    }
    println!("{}: {} gas used", operation, gas_used.unwrap());
}
