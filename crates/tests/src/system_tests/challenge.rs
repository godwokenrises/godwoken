use ckb_jsonrpc_types::{Uint32, Uint64};
use gw_jsonrpc_types::{godwoken::GlobalState, test_mode::{ChallengeType, ShouldProduceBlock, TestModePayload}};
use gw_tools::godwoken_rpc::GodwokenRpcClient;
use std::{thread::sleep, time::Duration};

const GODWOKEN_RPC_URL: &str = "http://127.0.0.1:8119";

struct TestModeRpc {
    godwoken_rpc: GodwokenRpcClient,
}

impl TestModeRpc {
    fn get_global_state(&mut self) -> Result<GlobalState, String> {
        self.godwoken_rpc.tests_get_global_state()
    }

    fn should_produce_block(&mut self) -> Result<ShouldProduceBlock, String> {
        self.godwoken_rpc.tests_should_produce_block()
    }
    
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
    let mut test_mode_rpc = TestModeRpc {
        godwoken_rpc: GodwokenRpcClient::new(GODWOKEN_RPC_URL),
    };
    let mut i = 0;
    while i < count {
        let ret = test_mode_rpc.should_produce_block()?;
        if let ShouldProduceBlock::Yes = ret {
            test_mode_rpc.issue_block()?;
            let state = test_mode_rpc.get_global_state()?;
            println!("state is: {:?}", state);
            i += 1;
            log::info!("issue blocks: {}", i);
            sleep(Duration::from_secs(1));
        }
    }
    log::info!("Finished.");
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
