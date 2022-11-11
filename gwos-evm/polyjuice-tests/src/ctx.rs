use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub use gw_common::builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID, RESERVED_ACCOUNT_ID};
use gw_common::{
    blake2b::new_blake2b,
    h256_ext::H256Ext,
    registry_address::RegistryAddress,
    state::{build_account_key, build_data_hash_key, State},
    CKB_SUDT_SCRIPT_ARGS, H256,
};
use gw_config::{BackendConfig, BackendForkConfig, BackendType};
use gw_db::schema::{COLUMN_INDEX, COLUMN_META, META_TIP_BLOCK_HASH_KEY};
use gw_generator::{
    account_lock_manage::{secp256k1::Secp256k1Eth, AccountLockManage},
    backend_manage::BackendManage,
    traits::StateExt,
    Generator,
};
use gw_store::{
    chain_view::ChainView,
    state::traits::JournalDB,
    traits::{chain_store::ChainStore, kv_store::KVStoreWrite},
    Store,
};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    core::{AllowedContractType, AllowedEoaType, ScriptHashType},
    offchain::{RollupContext, RunResult},
    packed::{AllowedTypeHash, BlockInfo, RawL2Transaction, RollupConfig, Script, Uint64},
    prelude::*,
    U256,
};

use crate::{
    helper::{
        build_eth_l2_script, build_l2_sudt_script, create_block_producer, load_program,
        PolyjuiceArgsBuilder, CHAIN_ID, CREATOR_ACCOUNT_ID, ETH_ACCOUNT_LOCK_CODE_HASH,
        L2TX_MAX_CYCLES, META_VALIDATOR_SCRIPT_TYPE_HASH, POLYJUICE_PROGRAM_CODE_HASH,
        ROLLUP_SCRIPT_HASH, SECP_LOCK_CODE_HASH, SUDT_VALIDATOR_SCRIPT_TYPE_HASH,
    },
    new_dummy_state, DummyState,
};

// meta contract
pub const META_VALIDATOR_PATH: &str = "build/godwoken-scripts/meta-contract-validator";
pub const META_GENERATOR_PATH: &str = "build/godwoken-scripts/meta-contract-generator";
// simple UDT
pub const SUDT_VALIDATOR_PATH: &str = "build/godwoken-scripts/sudt-validator";
pub const SUDT_GENERATOR_PATH: &str = "build/godwoken-scripts/sudt-generator";
pub const SECP_DATA_PATH: &str = "build/secp256k1_data";
// pub const SECP_DATA: &[u8] = include_bytes!("../../build/secp256k1_data");

// polyjuice
pub const POLYJUICE_GENERATOR_NAME: &str = "build/generator_log.aot";
pub const POLYJUICE_VALIDATOR_NAME: &str = "build/validator";
// ETH Address Registry
pub const ETH_ADDRESS_REGISTRY_GENERATOR_NAME: &str =
    "build/godwoken-scripts/eth-addr-reg-generator";
pub const ETH_ADDRESS_REGISTRY_VALIDATOR_NAME: &str =
    "build/godwoken-scripts/eth-addr-reg-validator";

fn load_code_hash(path: &Path) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(&load_program(path.to_str().unwrap()));
    hasher.finalize(&mut buf);
    buf
}

fn set_mapping(state: &mut DummyState, eth_address: &[u8; 20], script_hash: &[u8; 32]) {
    let address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec());
    state
        .mapping_registry_address_to_script_hash(address, (*script_hash).into())
        .expect("map reg addr to script hash");
}

pub struct MockChain {
    ctx: Context,
    block_producer: RegistryAddress,
    block_number: u64,
    timestamp: SystemTime,
    l2tx_cycle_limit: u64,
}

impl MockChain {
    /**
     * Setup with a base path. The base path is where we can find the **build**
     * directory.
     */
    pub fn setup(base_path: &str) -> anyhow::Result<Self> {
        let mut ctx = Context::setup(base_path)?;
        let block_producer = create_block_producer(&mut ctx.state);
        let timestamp = SystemTime::now();
        Ok(Self {
            ctx,
            block_producer,
            block_number: 0u64,
            timestamp,
            l2tx_cycle_limit: L2TX_MAX_CYCLES,
        })
    }

    pub fn set_max_cycles(&mut self, max_cycles: u64) {
        self.l2tx_cycle_limit = max_cycles;
    }

    fn new_block_info(&self) -> anyhow::Result<BlockInfo> {
        let timestamp = self
            .timestamp
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs();
        let block_info = BlockInfo::new_builder()
            .block_producer(Bytes::from(self.block_producer.to_bytes()).pack())
            .number(self.block_number.pack())
            .timestamp(timestamp.pack())
            .build();
        Ok(block_info)
    }

    pub fn create_eoa_account(
        &mut self,
        eth_address: &[u8; 20],
        mint_ckb: U256,
    ) -> anyhow::Result<u32> {
        let script = build_eth_l2_script(eth_address);
        let script_hash = script.hash();
        let account_id = self.ctx.state.create_account_from_script(script)?;
        set_mapping(&mut self.ctx.state, eth_address, &script_hash);
        let address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec());
        self.ctx
            .state
            .mint_sudt(CKB_SUDT_ACCOUNT_ID, &address, mint_ckb)?;
        Ok(account_id)
    }

    pub fn create_contract_account(
        &mut self,
        eth_address: &[u8; 20],
        mint_ckb: U256,
        code: &[u8],
        storage: HashMap<H256, H256>,
    ) -> anyhow::Result<u32> {
        let mut new_script_args = vec![0u8; 32 + 4 + 20];
        new_script_args[0..32].copy_from_slice(&ROLLUP_SCRIPT_HASH);
        new_script_args[32..36].copy_from_slice(&CREATOR_ACCOUNT_ID.to_le_bytes()[..]);
        new_script_args[36..36 + 20].copy_from_slice(eth_address);

        let script = Script::new_builder()
            .code_hash(POLYJUICE_PROGRAM_CODE_HASH.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(new_script_args.pack())
            .build();
        let script_hash = script.hash();
        let account_id = self.ctx.state.create_account_from_script(script)?;
        set_mapping(&mut self.ctx.state, eth_address, &script_hash);
        let address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec());
        self.ctx
            .state
            .mint_sudt(CKB_SUDT_ACCOUNT_ID, &address, mint_ckb)?;

        self.set_code(account_id, code)?;
        self.set_storage(account_id, storage)?;
        Ok(account_id)
    }

    pub fn root_state(&self) -> anyhow::Result<H256> {
        let root = self.ctx.state.calculate_root()?;
        Ok(root)
    }

    fn set_code(&mut self, account_id: u32, code: &[u8]) -> anyhow::Result<()> {
        //build polyjuice account key
        let key = build_contract_code_key(account_id);

        let mut data_hash = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(code);
        hasher.finalize(&mut data_hash);
        let data_hash = build_account_key(account_id, &data_hash);
        //sys_store key - data hash
        self.ctx.state.update_value(account_id, &key, data_hash)?;
        let data_hash = self.ctx.state.get_value(account_id, &key)?;

        let data_hash_key = build_data_hash_key(data_hash.as_slice());
        self.ctx.state.update_raw(data_hash_key, H256::one())?;
        //sys_store_data data hash - data
        let code = Bytes::copy_from_slice(code);

        self.ctx.state.insert_data(data_hash, code);
        Ok(())
    }

    fn set_storage(&mut self, account_id: u32, values: HashMap<H256, H256>) -> anyhow::Result<()> {
        for (raw_key, val) in values {
            let key = build_account_key(account_id, raw_key.as_slice());
            self.ctx.state.update_raw(key, val)?;
        }

        Ok(())
    }
    pub fn deploy(
        &mut self,
        from_id: u32,
        code: &[u8],
        gas_limit: u64,
        gas_price: u128,
        value: u128,
    ) -> anyhow::Result<RunResult> {
        let args = PolyjuiceArgsBuilder::default()
            .do_create(true)
            .gas_limit(gas_limit)
            .gas_price(gas_price)
            .value(value)
            .input(code)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(CREATOR_ACCOUNT_ID.pack())
            .args(Bytes::from(args).pack())
            .build();
        let run_result = self.call(raw_tx)?;
        self.ctx.state.finalise()?;
        Ok(run_result)
    }

    pub fn execute_raw(&mut self, raw_tx: RawL2Transaction) -> anyhow::Result<RunResult> {
        let run_result = self.call(raw_tx)?;
        self.ctx.state.finalise()?;
        Ok(run_result)
    }

    pub fn execute(
        &mut self,
        from_id: u32,
        to_id: u32,
        code: &[u8],
        gas_limit: u64,
        gas_price: u128,
        value: u128,
    ) -> anyhow::Result<RunResult> {
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(gas_limit)
            .gas_price(gas_price)
            .value(value)
            .input(code)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(to_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let run_result = self.call(raw_tx)?;
        self.ctx.state.finalise()?;
        Ok(run_result)
    }

    pub fn call(&mut self, raw_tx: RawL2Transaction) -> anyhow::Result<RunResult> {
        let db = &self.ctx.store.begin_transaction();
        let tip_block_hash = db.get_tip_block_hash()?;
        let chain = ChainView::new(&db, tip_block_hash);
        let block_info = self.new_block_info()?;

        let run_result = self.ctx.generator.execute_transaction(
            &chain,
            &mut self.ctx.state,
            &block_info,
            &raw_tx,
            self.l2tx_cycle_limit,
            None,
        )?;

        self.block_number += 1;
        self.timestamp = SystemTime::now();
        Ok(run_result)
    }

    pub fn get_script_hash_by_registry_address(
        &self,
        registry_address: &RegistryAddress,
    ) -> anyhow::Result<Option<H256>> {
        let script_hash = self
            .ctx
            .state
            .get_script_hash_by_registry_address(registry_address)?;
        Ok(script_hash)
    }

    pub fn get_account_id_by_script_hash(&self, script_hash: &H256) -> anyhow::Result<Option<u32>> {
        let id = self.ctx.state.get_account_id_by_script_hash(script_hash)?;
        Ok(id)
    }

    pub fn get_account_id_by_eth_address(
        &self,
        eth_address: &[u8; 20],
    ) -> anyhow::Result<Option<u32>> {
        let address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec());
        let script_hash = self
            .ctx
            .state
            .get_script_hash_by_registry_address(&address)?;
        match script_hash {
            Some(script_hash) => {
                let id = self.ctx.state.get_account_id_by_script_hash(&script_hash)?;
                Ok(id)
            }
            None => Ok(None),
        }
    }

    pub fn get_nonce(&self, account_id: u32) -> anyhow::Result<u32> {
        let nonce = self.ctx.state.get_nonce(account_id)?;
        Ok(nonce)
    }

    pub fn to_reg_addr(eth_address: &[u8; 20]) -> RegistryAddress {
        RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec())
    }

    pub fn get_balance(&self, eth_address: &[u8; 20]) -> anyhow::Result<U256> {
        let reg_addr = Self::to_reg_addr(eth_address);
        let balance = self
            .ctx
            .state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &reg_addr)?;
        Ok(balance)
    }
}
pub struct Context {
    state: DummyState,
    store: Store,
    generator: Generator,
}

impl Context {
    pub fn setup(base_path: &str) -> anyhow::Result<Self> {
        let _ = env_logger::try_init();
        let config = Config::new(base_path);

        let store = Store::open_tmp()?;
        let snapshot = store.get_snapshot();
        let mut state = new_dummy_state(snapshot);

        let meta_script = Script::new_builder()
            .code_hash(META_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
            .hash_type(ScriptHashType::Type.into())
            .build();
        let reserved_id = state.create_account_from_script(meta_script)?;
        assert_eq!(
            reserved_id, RESERVED_ACCOUNT_ID,
            "reserved account id must be zero"
        );

        // setup CKB simple UDT contract
        let ckb_sudt_script = build_l2_sudt_script(CKB_SUDT_SCRIPT_ARGS);
        let ckb_sudt_id = state.create_account_from_script(ckb_sudt_script)?;
        assert_eq!(
            ckb_sudt_id, CKB_SUDT_ACCOUNT_ID,
            "ckb simple UDT account id"
        );

        // create `ETH Address Registry` layer2 contract account
        let eth_addr_reg_script = Script::new_builder()
            .code_hash(config.eth_addr_reg_code_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(ROLLUP_SCRIPT_HASH.to_vec().pack())
            .build();
        let eth_addr_reg_account_id = state.create_account_from_script(eth_addr_reg_script)?;
        assert_eq!(eth_addr_reg_account_id, ETH_REGISTRY_ACCOUNT_ID);

        let mut args = [0u8; 36];
        args[0..32].copy_from_slice(&ROLLUP_SCRIPT_HASH);
        args[32..36].copy_from_slice(&ckb_sudt_id.to_le_bytes()[..]);
        let creator_script = Script::new_builder()
            .code_hash(config.polyjuice_validator_code_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(args.to_vec().pack())
            .build();
        let creator_account_id = state
            .create_account_from_script(creator_script)
            .expect("create creator_account");
        assert_eq!(creator_account_id, CREATOR_ACCOUNT_ID);

        state.insert_data(config.secp_data_hash.into(), config.secp_data.clone());
        state
            .update_raw(
                build_data_hash_key(config.secp_data_hash.as_slice()),
                H256::one(),
            )
            .expect("update secp data key");

        let backend_manage =
            BackendManage::from_config(vec![config.backends.clone()]).expect("default backend");
        // NOTICE in this test we won't need SUM validator
        let mut account_lock_manage = AccountLockManage::default();
        account_lock_manage.register_lock_algorithm(
            SECP_LOCK_CODE_HASH.into(),
            Box::new(Secp256k1Eth::default()),
        );
        let rollup_context = RollupContext {
            rollup_script_hash: ROLLUP_SCRIPT_HASH.into(),
            rollup_config: config.rollup,
        };
        let generator = Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context,
            Default::default(),
        );

        let tx = &store.begin_transaction();
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
        Ok(Self {
            store,
            state,
            generator,
        })
    }
}

struct Config {
    backends: BackendForkConfig,
    rollup: RollupConfig,
    polyjuice_validator_code_hash: [u8; 32],
    eth_addr_reg_code_hash: [u8; 32],
    secp_data: Bytes,
    secp_data_hash: [u8; 32],
}

impl Config {
    fn new(base_path: &str) -> Self {
        let path: PathBuf = [base_path, POLYJUICE_VALIDATOR_NAME].iter().collect();
        let polyjuice_validator_code_hash = load_code_hash(&path);

        let path: PathBuf = [base_path, ETH_ADDRESS_REGISTRY_VALIDATOR_NAME]
            .iter()
            .collect();
        let eth_addr_reg_code_hash = load_code_hash(&path);
        let path: PathBuf = [base_path, SECP_DATA_PATH].iter().collect();
        let secp_data = load_program(path.to_str().unwrap());
        let secp_data_hash = load_code_hash(&path);
        let backends = BackendForkConfig {
            switch_height: 0,
            backends: vec![
                BackendConfig {
                    backend_type: BackendType::Meta,
                    validator_path: [base_path, META_VALIDATOR_PATH].iter().collect(),
                    generator_path: [base_path, META_GENERATOR_PATH].iter().collect(),
                    validator_script_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
                },
                BackendConfig {
                    backend_type: BackendType::Sudt,
                    validator_path: [base_path, SUDT_VALIDATOR_PATH].iter().collect(),
                    generator_path: [base_path, SUDT_GENERATOR_PATH].iter().collect(),
                    validator_script_type_hash: SUDT_VALIDATOR_SCRIPT_TYPE_HASH.into(),
                },
                BackendConfig {
                    backend_type: BackendType::Polyjuice,
                    validator_path: [base_path, POLYJUICE_VALIDATOR_NAME].iter().collect(),
                    generator_path: [base_path, POLYJUICE_GENERATOR_NAME].iter().collect(),
                    validator_script_type_hash: polyjuice_validator_code_hash.into(),
                },
                BackendConfig {
                    backend_type: BackendType::EthAddrReg,
                    validator_path: [base_path, ETH_ADDRESS_REGISTRY_VALIDATOR_NAME]
                        .iter()
                        .collect(),
                    generator_path: [base_path, ETH_ADDRESS_REGISTRY_GENERATOR_NAME]
                        .iter()
                        .collect(),
                    validator_script_type_hash: eth_addr_reg_code_hash.into(),
                },
            ],
        };
        let rollup = RollupConfig::new_builder()
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
                        polyjuice_validator_code_hash,
                    ),
                    AllowedTypeHash::new(AllowedContractType::EthAddrReg, eth_addr_reg_code_hash),
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
        Self {
            backends,
            rollup,
            polyjuice_validator_code_hash,
            eth_addr_reg_code_hash,
            secp_data,
            secp_data_hash,
        }
    }
}

// port from poolyjuice.h#polyjuice_build_system_key
pub fn build_contract_code_key(account_id: u32) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[0..4].copy_from_slice(&account_id.to_le_bytes());
    key[4] = 0xFF;
    key[5] = 0x01;
    key
}
