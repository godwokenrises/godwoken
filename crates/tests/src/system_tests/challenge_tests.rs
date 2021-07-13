use crate::system_tests::test_mode_control::TestModeRpc;

pub fn issue_bad_block_and_revert() -> Result<(), String> {
    log::info!("[challenge test]: issue bad block and revert");
    let mut _test_mode_rpc = TestModeRpc::new();
    Ok(())
}

pub fn issue_bad_challenge_and_cancel() -> Result<(), String> {
    log::info!("[Test]: issue bad challenge and cancel");
    Ok(())
}

pub fn check_balance_when_revert() -> Result<(), String> {
    log::info!("[Test]: check balance when revert");
    Ok(())
}

pub fn issue_multi_bad_blocks_and_revert() -> Result<(), String> {
    log::info!("[Test]: issue multi bad blocks and revert");
    Ok(())
}
