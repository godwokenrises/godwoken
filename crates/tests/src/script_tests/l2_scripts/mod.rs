use gw_common::blake2b::new_blake2b;
use gw_common::state::State;
use gw_common::H256;
use gw_generator::{account_lock_manage::AccountLockManage, Generator};
use gw_generator::{error::TransactionError, traits::StateExt, types::RollupContext};
use gw_traits::{ChainStore, CodeStore};
use gw_types::packed::{RawL2Transaction, RollupConfig};
use gw_types::{bytes::Bytes, packed::BlockInfo, prelude::*};
use lazy_static::lazy_static;
use std::{fs, io::Read, path::PathBuf};

use crate::testing_tool::chain::build_backend_manage;

mod examples;
mod meta_contract;
mod sudt;

const EXAMPLES_DIR: &'static str = "../../godwoken-scripts/c/build/examples";
const SUM_BIN_NAME: &'static str = "sum-generator";

lazy_static! {
    static ref SUM_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&EXAMPLES_DIR);
        path.push(&SUM_BIN_NAME);
        let mut f = fs::File::open(&path).expect("load program");
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
}

pub fn new_block_info(block_producer_id: u32, number: u64, timestamp: u64) -> BlockInfo {
    BlockInfo::new_builder()
        .block_producer_id(block_producer_id.pack())
        .number(number.pack())
        .timestamp(timestamp.pack())
        .build()
}

struct DummyChainStore;
impl ChainStore for DummyChainStore {
    fn get_block_hash_by_number(&self, _number: u64) -> Result<Option<H256>, gw_db::error::Error> {
        Err("dummy chain store".to_string().into())
    }
}

pub fn run_contract<S: State + CodeStore>(
    rollup_config: &RollupConfig,
    tree: &mut S,
    from_id: u32,
    to_id: u32,
    args: Bytes,
    block_info: &BlockInfo,
) -> Result<Vec<u8>, TransactionError> {
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
    };
    let generator = Generator::new(backend_manage, account_lock_manage, rollup_ctx);
    let chain_view = DummyChainStore;
    let run_result = generator.execute_transaction(&chain_view, tree, block_info, &raw_tx)?;
    tree.apply_run_result(&run_result).expect("update state");
    Ok(run_result.return_data)
}
