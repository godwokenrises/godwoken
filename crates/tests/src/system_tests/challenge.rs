use ckb_jsonrpc_types::{Uint32, Uint64};
use gw_jsonrpc_types::test_mode::{ChallengeType, ShouldProduceBlock, TestModePayload};
use gw_tools::godwoken_rpc::GodwokenRpcClient;
use std::{thread::sleep, time::Duration};

const GODWOKEN_RPC_URL: &str = "http://127.0.0.1:8119";

struct TestModeRpc {
    godwoken_rpc: GodwokenRpcClient,
}

impl TestModeRpc {
    fn issue_block(&mut self) -> Result<(), String> {
        self.godwoken_rpc.tests_produce_block(TestModePayload::None)
    }

    fn issue_bad_block(
        &mut self,
        target_index: u32,
        target_type: ChallengeType,
    ) -> Result<(), String> {
        self.godwoken_rpc
            .tests_produce_block(TestModePayload::BadBlock {
                target_index: target_index.into(),
                target_type,
            })
    }

    fn issue_challenge(
        &mut self,
        block_number: u64,
        target_index: u32,
        target_type: ChallengeType,
    ) -> Result<(), String> {
        self.godwoken_rpc
            .tests_produce_block(TestModePayload::Challenge {
                block_number: Uint64::from(block_number),
                target_index: Uint32::from(target_index),
                target_type,
            })
    }
}

pub fn issue_test_blocks(count: i32) -> Result<(), String> {
    log::info!("[Test]: issue test blocks");
    Ok(())
}

pub fn issue_bad_block() -> Result<(), String> {
    log::info!("[Test]: issue bad block and revert");
    Ok(())
}

pub fn issue_bad_challenge() -> Result<(), String> {
    log::info!("[Test]: issue bad challenge and cancel");
    Ok(())
}

pub fn check_balance_when_revert() -> Result<(), String> {
    log::info!("[Test]: check balance when revert");
    Ok(())
}

pub fn issue_multi_bad_blocks() -> Result<(), String> {
    log::info!("[Test]: issue multi bad blocks and revert");
    Ok(())
}
