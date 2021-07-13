use ckb_jsonrpc_types::{Uint32, Uint64};
use gw_jsonrpc_types::{
    godwoken::GlobalState,
    test_mode::{ChallengeType, ShouldProduceBlock, TestModePayload},
};
use gw_tools::godwoken_rpc::GodwokenRpcClient;
use std::{thread::sleep, time::Duration};

pub const GODWOKEN_RPC_URL: &str = "http://127.0.0.1:8119";

pub struct TestModeRpc {
    godwoken_rpc: GodwokenRpcClient,
}

impl TestModeRpc {
    pub fn new() -> Self {
        TestModeRpc {
            godwoken_rpc: GodwokenRpcClient::new(GODWOKEN_RPC_URL),
        }
    }

    pub fn get_global_state(&mut self) -> Result<GlobalState, String> {
        self.godwoken_rpc.tests_get_global_state()
    }

    pub fn should_produce_block(&mut self) -> Result<ShouldProduceBlock, String> {
        self.godwoken_rpc.tests_should_produce_block()
    }

    pub fn issue_block(&mut self) -> Result<(), String> {
        self.godwoken_rpc.tests_produce_block(TestModePayload::None)
    }

    pub fn issue_bad_block(
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

    pub fn issue_challenge(
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

pub fn get_global_state() -> Result<GlobalState, String> {
    log::info!("[test mode control]: get global state");
    let mut test_mode_rpc = TestModeRpc::new();
    test_mode_rpc.get_global_state()
}

pub fn issue_test_blocks(count: i32) -> Result<(), String> {
    log::info!("[test mode control]: issue test block");
    let mut test_mode_rpc = TestModeRpc::new();
    let mut i = 0;
    while i < count {
        let ret = test_mode_rpc.should_produce_block()?;
        if let ShouldProduceBlock::Yes = ret {
            test_mode_rpc.issue_block()?;
            i += 1;
            log::info!("issue blocks: {}", i);
            sleep(Duration::from_secs(1));
        }
    }
    log::info!("Finished.");
    Ok(())
}

pub fn issue_bad_block() -> Result<(), String> {
    log::info!("[test mode control]: issue bad block");
    let mut test_mode_rpc = TestModeRpc::new();
    let mut i = 0;
    while i < 1 {
        let ret = test_mode_rpc.should_produce_block()?;
        if let ShouldProduceBlock::Yes = ret {
            test_mode_rpc.issue_bad_block(0, ChallengeType::TxExecution)?;
            i += 1;
            log::info!("issue bad block");
            sleep(Duration::from_secs(1));
        }
    }
    Ok(())
}

pub fn issue_challenge(block_number: u64) -> Result<(), String> {
    log::info!("[test mode contro]: issue challenge");
    let mut test_mode_rpc = TestModeRpc::new();
    let mut i = 0;
    while i < 1 {
        let ret = test_mode_rpc.should_produce_block()?;
        if let ShouldProduceBlock::Yes = ret {
            test_mode_rpc.issue_challenge(block_number, 0, ChallengeType::TxExecution)?;
            i += 1;
            log::info!(
                "issue challenge: block number {}, target_index 0, ChallengeType TxExecution",
                block_number
            );
            sleep(Duration::from_secs(1));
        }
    }
    Ok(())
}
