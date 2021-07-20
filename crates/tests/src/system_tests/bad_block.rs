use crate::system_tests::utils::{self, TestModeRpc};
use gw_jsonrpc_types::test_mode::{ChallengeType, ShouldProduceBlock};
use std::path::Path;

pub fn issue_bad_block(
    from_privkey_path: &Path,
    to_privkey_path: &Path,
    config_path: &Path,
    deployment_results_path: &Path,
    godwoken_rpc_url: &str,
) -> Result<(), String> {
    log::info!("[test mode control]: issue bad block");

    let from_id = utils::get_account(
        godwoken_rpc_url,
        from_privkey_path,
        config_path,
        deployment_results_path,
    )?;
    let to_id = utils::get_account(
        godwoken_rpc_url,
        to_privkey_path,
        config_path,
        deployment_results_path,
    )?;

    log::info!("from id: {}, to id: {}", from_id, to_id);
    utils::submit_a_transaction(
        godwoken_rpc_url,
        from_privkey_path,
        config_path,
        deployment_results_path,
        to_id,
    )?;

    let mut test_mode_rpc = TestModeRpc::new(godwoken_rpc_url);
    let mut i = 0;
    while i < 1 {
        let ret = test_mode_rpc.should_produce_block()?;
        if let ShouldProduceBlock::Yes = ret {
            test_mode_rpc.issue_bad_block(0, ChallengeType::TxSignature)?;
            i += 1;
            log::info!("issue bad block");
        }
    }
    Ok(())
}
