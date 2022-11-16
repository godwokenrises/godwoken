use gw_common::blake2b::new_blake2b;
use gw_common::builtins::CKB_SUDT_ACCOUNT_ID;
use gw_common::registry_address::RegistryAddress;
use gw_common::state::State;
use gw_common::H256;
use gw_generator::error::TransactionError;
use gw_generator::{account_lock_manage::AccountLockManage, Generator};
use gw_store::state::traits::JournalDB;
use gw_traits::{ChainView, CodeStore};
use gw_types::U256;
use gw_types::{
    bytes::Bytes,
    offchain::RunResult,
    packed::{BlockInfo, LogItem, RawL2Transaction, RollupConfig},
    prelude::*,
};
use gw_utils::RollupContext;
use lazy_static::lazy_static;
use std::convert::TryInto;
use std::{fs, io::Read, path::PathBuf};

use crate::testing_tool::chain::build_backend_manage;

mod examples;
mod meta_contract;
mod sudt;

const EXAMPLES_DIR: &str = "../../gwos/c/build/examples";
const SUM_BIN_NAME: &str = "sum-generator";
const ACCOUNT_OP_BIN_NAME: &str = "account-operation-generator";
const RECOVER_BIN_NAME: &str = "recover-account-generator";
const SUDT_TOTAL_SUPPLY_BIN_NAME: &str = "sudt-total-supply-generator";

lazy_static! {
    static ref SUM_PROGRAM_PATH: PathBuf = {
        let mut path = PathBuf::new();
        path.push(&EXAMPLES_DIR);
        path.push(&SUM_BIN_NAME);
        path
    };
    static ref SUM_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut f = fs::File::open(&*SUM_PROGRAM_PATH).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    static ref SUM_PROGRAM_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&SUM_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    static ref ACCOUNT_OP_PROGRAM_PATH: PathBuf = {
        let mut path = PathBuf::new();
        path.push(&EXAMPLES_DIR);
        path.push(&ACCOUNT_OP_BIN_NAME);
        path
    };
    static ref ACCOUNT_OP_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut f = fs::File::open(&*ACCOUNT_OP_PROGRAM_PATH).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    static ref ACCOUNT_OP_PROGRAM_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ACCOUNT_OP_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    static ref RECOVER_PROGRAM_PATH: PathBuf = {
        let mut path = PathBuf::new();
        path.push(&EXAMPLES_DIR);
        path.push(&RECOVER_BIN_NAME);
        path
    };
    static ref RECOVER_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut f = fs::File::open(&*RECOVER_PROGRAM_PATH).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    static ref RECOVER_PROGRAM_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&RECOVER_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    static ref SUDT_TOTAL_SUPPLY_PROGRAM_PATH: PathBuf = {
        let mut path = PathBuf::new();
        path.push(&EXAMPLES_DIR);
        path.push(&SUDT_TOTAL_SUPPLY_BIN_NAME);
        path
    };
    static ref SUDT_TOTAL_SUPPLY_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut f = fs::File::open(&*SUDT_TOTAL_SUPPLY_PROGRAM_PATH).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    static ref SUDT_TOTAL_SUPPLY_PROGRAM_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&SUDT_TOTAL_SUPPLY_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
}

pub fn new_block_info(block_producer: &RegistryAddress, number: u64, timestamp: u64) -> BlockInfo {
    BlockInfo::new_builder()
        .block_producer(Bytes::from(block_producer.to_bytes()).pack())
        .number(number.pack())
        .timestamp(timestamp.pack())
        .build()
}

struct DummyChainStore;
impl ChainView for DummyChainStore {
    fn get_block_hash_by_number(&self, _number: u64) -> Result<Option<H256>, gw_db::error::Error> {
        Err("dummy chain store".to_string().into())
    }
}

pub const GW_LOG_SUDT_TRANSFER: u8 = 0x0;
pub const GW_LOG_SUDT_PAY_FEE: u8 = 0x1;
#[allow(dead_code)]
pub const GW_LOG_POLYJUICE_SYSTEM: u8 = 0x2;
#[allow(dead_code)]
pub const GW_LOG_POLYJUICE_USER: u8 = 0x3;

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum SudtLogType {
    Transfer,
    PayFee,
}

impl SudtLogType {
    fn from_u8(service_flag: u8) -> Result<SudtLogType, String> {
        match service_flag {
            GW_LOG_SUDT_TRANSFER => Ok(Self::Transfer),
            GW_LOG_SUDT_PAY_FEE => Ok(Self::PayFee),
            _ => Err(format!(
                "Not a sudt transfer/payfee prefix: {}",
                service_flag
            )),
        }
    }
}

#[derive(Debug)]
pub struct SudtLog {
    sudt_id: u32,
    from_addr: RegistryAddress,
    to_addr: RegistryAddress,
    amount: U256,
    log_type: SudtLogType,
}

impl SudtLog {
    fn from_log_item(item: &LogItem) -> Result<SudtLog, String> {
        let sudt_id: u32 = item.account_id().unpack();
        let service_flag: u8 = item.service_flag().into();
        let raw_data = item.data().raw_data();
        let data: &[u8] = raw_data.as_ref();
        let log_type = SudtLogType::from_u8(service_flag)?;
        if data.len() > (1 + 32 + 32 + 32) {
            return Err(format!("Invalid data length: {}", data.len()));
        }
        let from_addr = {
            let registry_id: u32 = u32::from_le_bytes(data[..4].try_into().unwrap());
            let addr_len: u32 = u32::from_le_bytes(data[4..8].try_into().unwrap());
            RegistryAddress::new(registry_id, data[8..(8 + addr_len as usize)].to_vec())
        };
        let to_addr = {
            let offset = from_addr.len();
            let registry_id: u32 = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
            let addr_len: u32 =
                u32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap());
            RegistryAddress::new(
                registry_id,
                data[(offset + 8)..(offset + 8 + addr_len as usize)].to_vec(),
            )
        };

        let amount: U256 = {
            let mut u256_bytes = [0u8; 32];
            u256_bytes.copy_from_slice(&data[data.len() - 32..]);
            U256::from_little_endian(&u256_bytes)
        };

        Ok(SudtLog {
            sudt_id,
            from_addr,
            to_addr,
            amount,
            log_type,
        })
    }
}

pub fn check_transfer_logs(
    logs: &[LogItem],
    sudt_id: u32,
    block_producer_addr: &RegistryAddress,
    fee: u128,
    from_addr: &RegistryAddress,
    to_addr: &RegistryAddress,
    amount: U256,
) {
    // pay fee log
    let sudt_fee_log = SudtLog::from_log_item(&logs[0]).unwrap();
    assert_eq!(sudt_fee_log.sudt_id, CKB_SUDT_ACCOUNT_ID);
    assert_eq!(&sudt_fee_log.from_addr, from_addr,);
    assert_eq!(&sudt_fee_log.to_addr, block_producer_addr);
    assert_eq!(sudt_fee_log.amount, fee.into());
    assert_eq!(sudt_fee_log.log_type, SudtLogType::PayFee);
    // transfer to `to_id`
    let sudt_transfer_log = SudtLog::from_log_item(&logs[1]).unwrap();
    assert_eq!(sudt_transfer_log.sudt_id, sudt_id);
    assert_eq!(&sudt_transfer_log.from_addr, from_addr);
    assert_eq!(&sudt_transfer_log.to_addr, to_addr);
    assert_eq!(sudt_transfer_log.amount, amount);
    assert_eq!(sudt_transfer_log.log_type, SudtLogType::Transfer);
}

pub fn run_contract_get_result<S: State + CodeStore + JournalDB>(
    rollup_config: &RollupConfig,
    tree: &mut S,
    from_id: u32,
    to_id: u32,
    args: Bytes,
    block_info: &BlockInfo,
) -> Result<RunResult, TransactionError> {
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(to_id.pack())
        .args(args.pack())
        .build();
    let backend_manage = build_backend_manage(rollup_config);
    let account_lock_manage = AccountLockManage::default();
    let rollup_ctx = RollupContext {
        rollup_config: rollup_config.clone(),
        rollup_script_hash: [42u8; 32].into(),
        fork_config: Default::default(),
    };
    let generator = Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_ctx,
        Default::default(),
    );
    let chain_view = DummyChainStore;
    let run_result = generator
        .execute_transaction(&chain_view, tree, block_info, &raw_tx, None, None)
        .map_err(|err| err.downcast::<TransactionError>().unwrap())?;
    Ok(run_result)
}

pub fn run_contract<S: State + CodeStore + JournalDB>(
    rollup_config: &RollupConfig,
    tree: &mut S,
    from_id: u32,
    to_id: u32,
    args: Bytes,
    block_info: &BlockInfo,
) -> Result<Vec<u8>, TransactionError> {
    let run_result =
        run_contract_get_result(rollup_config, tree, from_id, to_id, args, block_info)?;
    Ok(run_result.return_data.to_vec())
}
