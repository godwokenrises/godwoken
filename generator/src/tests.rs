use crate::smt::{DefaultStore, H256, SMT};
use crate::{execute, Context};
use godwoken_types::{
    bytes::Bytes,
    core::CallType,
    packed::{BlockInfo, CallContext},
    prelude::*,
};
use std::{fs, io::Read, path::PathBuf};

const EXAMPLES_DIR: &'static str = "../c/build/examples";
const SUM_BIN_NAME: &'static str = "sum.so";

fn new_block_info(aggregator_id: u32, number: u64, timestamp: u64) -> BlockInfo {
    BlockInfo::new_builder()
        .aggregator_id(aggregator_id.pack())
        .number(number.pack())
        .timestamp(timestamp.pack())
        .build()
}

#[test]
fn test_example_sum() {
    let tree: SMT<DefaultStore<H256>> = SMT::default();
    let from_id: u32 = 2;
    let contract_id: u32 = 3;
    let init_value: u64 = 42;

    // run constructor
    let block_info = new_block_info(0, 0, 0);
    let call_context = CallContext::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_id.pack())
        .call_type(CallType::Construct.into())
        .args(Bytes::from(init_value.to_le_bytes().to_vec()).pack())
        .build();
    let ctx = Context::new(block_info, call_context);
    let program = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&EXAMPLES_DIR);
        path.push(&SUM_BIN_NAME);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    let run_result = execute(&ctx, &tree, &program).expect("construct");
    let return_value = {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&run_result.return_data);
        u64::from_le_bytes(buf)
    };
    assert_eq!(return_value, init_value);

    // run constructor
    println!("result {:?}", run_result);
}
