use crate::system_tests::test_mode_control::TestModeRpc;
use gw_jsonrpc_types::test_mode::{ChallengeType, ShouldProduceBlock};
use gw_tools::{
    account, deploy_scripts::ScriptsDeploymentResult, deposit_ckb, godwoken_rpc::GodwokenRpcClient,
    transfer, utils,
};
use std::path::Path;

pub const FULL_NODE_MODE_GODWOKEN_RPC_URL: &str = "http://127.0.0.1:8119";
pub const TEST_MODE_GODWOKEN_RPC_URL: &str = "http://127.0.0.1:8129";
pub const CKB_RPC_URL: &str = "http://127.0.0.1:8114";

pub fn issue_bad_block(
    from_privkey_path: &Path,
    to_privkey_path: &Path,
    config_path: &Path,
    deployment_results_path: &Path,
) -> Result<(), String> {
    log::info!("[test mode control]: issue bad block");

    let from_id = prepare_account(from_privkey_path, config_path, deployment_results_path)?;
    let to_id = prepare_account(to_privkey_path, config_path, deployment_results_path)?;

    log::info!("from id: {}, to id: {}", from_id, to_id);
    submit_a_transaction_to_test_node(
        from_privkey_path,
        config_path,
        deployment_results_path,
        to_id,
    )?;

    let mut test_mode_rpc = TestModeRpc::new();
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

fn prepare_account(
    privkey_path: &Path,
    config_path: &Path,
    deployment_results_path: &Path,
) -> Result<u32, String> {
    let mut full_node_godwoken_rpc = GodwokenRpcClient::new(FULL_NODE_MODE_GODWOKEN_RPC_URL);

    // check from id
    let deployment_result_string =
        std::fs::read_to_string(deployment_results_path).map_err(|err| err.to_string())?;
    let deployment_result: ScriptsDeploymentResult =
        serde_json::from_str(&deployment_result_string).map_err(|err| err.to_string())?;
    let config = utils::read_config(config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;
    let privkey = account::read_privkey(privkey_path)?;
    let from_address =
        account::privkey_to_short_address(&privkey, &rollup_type_hash, &deployment_result)?;
    let from_id = account::short_address_to_account_id(&mut full_node_godwoken_rpc, &from_address);

    // deposit to create a account
    if from_id.is_err() {
        deposit_ckb::deposit_ckb(
            privkey_path,
            deployment_results_path,
            config_path,
            "10000",
            "0.0001",
            CKB_RPC_URL,
            None,
            FULL_NODE_MODE_GODWOKEN_RPC_URL,
        )?;
    }

    account::short_address_to_account_id(&mut full_node_godwoken_rpc, &from_address)?
        .ok_or("get account error".to_owned())
}

fn submit_a_transaction_to_test_node(
    from_privkey_path: &Path,
    config_path: &Path,
    deployment_results_path: &Path,
    to: u32,
) -> Result<(), String> {
    transfer::submit_l2_transaction(
        TEST_MODE_GODWOKEN_RPC_URL,
        from_privkey_path,
        &to.to_string(),
        1u32,
        "100",
        "1",
        config_path,
        deployment_results_path,
    )
}
