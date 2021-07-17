use crate::system_tests::test_mode_control::TestModeRpc;
use gw_jsonrpc_types::test_mode::ChallengeType;

pub fn issue_bad_challenge(block_number: u64) -> Result<(), String> {
    log::info!("[test mode contro]: issue bad challenge");
    let mut test_mode_rpc = TestModeRpc::new();
    let challenge_type = ChallengeType::TxSignature;
    test_mode_rpc.issue_challenge(block_number, 0, challenge_type)?;
    log::info!(
        "issue challenge: block number {}, target_index 0, ChallengeType {:?}",
        block_number,
        challenge_type
    );

    Ok(())
}
