use crate::system_tests::test_mode_control::TestModeRpc;
use gw_jsonrpc_types::test_mode::{ChallengeType, ShouldProduceBlock};

pub fn issue_bad_challenge(block_number: u64) -> Result<(), String> {
    log::info!("[test mode contro]: issue bad challenge");
    let mut test_mode_rpc = TestModeRpc::new();
    let mut i = 0;
    while i < 1 {
        let ret = test_mode_rpc.should_produce_block()?;
        if let ShouldProduceBlock::Yes = ret {
            test_mode_rpc.issue_challenge(block_number, 0, ChallengeType::TxSignature)?;
            i += 1;
            log::info!(
                "issue challenge: block number {}, target_index 0, ChallengeType TxExecution",
                block_number
            );
        }
    }
    Ok(())
}
