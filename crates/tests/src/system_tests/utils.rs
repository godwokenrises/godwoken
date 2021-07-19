use ckb_jsonrpc_types::{Uint32, Uint64};
use gw_jsonrpc_types::{
    godwoken::GlobalState,
    test_mode::TestModePayload,
    test_mode::{ChallengeType, ShouldProduceBlock},
};
use gw_tools::{
    account, deploy_scripts::ScriptsDeploymentResult, godwoken_rpc::GodwokenRpcClient, transfer,
    utils,
};
use std::{path::Path, thread::sleep, time::Duration};

pub const FULL_NODE_MODE_GODWOKEN_RPC_URL: &str = "http://127.0.0.1:8119";
pub const TEST_MODE_GODWOKEN_RPC_URL: &str = "http://127.0.0.1:8129";
pub const CKB_RPC_URL: &str = "http://127.0.0.1:8114";

pub struct TestModeRpc {
    pub godwoken_rpc: GodwokenRpcClient,
}

impl TestModeRpc {
    pub fn new(godwoken_rpc_url: &str) -> Self {
        TestModeRpc {
            godwoken_rpc: GodwokenRpcClient::new(godwoken_rpc_url),
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
    let mut test_mode_rpc = TestModeRpc::new(TEST_MODE_GODWOKEN_RPC_URL);
    test_mode_rpc.get_global_state()
}

pub fn issue_blocks(count: i32) -> Result<(), String> {
    log::info!("[test mode control]: issue test block");
    let mut test_mode_rpc = TestModeRpc::new(TEST_MODE_GODWOKEN_RPC_URL);
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

pub fn package_a_transaction(
    from_privkey_path: &Path,
    to_privkey_path: &Path,
    config_path: &Path,
    deployment_results_path: &Path,
) -> Result<(), String> {
    let from_id = get_account(
        TEST_MODE_GODWOKEN_RPC_URL,
        from_privkey_path,
        config_path,
        deployment_results_path,
    )?;
    let to_id = get_account(
        TEST_MODE_GODWOKEN_RPC_URL,
        to_privkey_path,
        config_path,
        deployment_results_path,
    )?;

    log::info!("from id: {}, to id: {}", from_id, to_id);
    submit_a_transaction(
        TEST_MODE_GODWOKEN_RPC_URL,
        from_privkey_path,
        config_path,
        deployment_results_path,
        to_id,
    )?;

    let mut test_mode_rpc = TestModeRpc::new(TEST_MODE_GODWOKEN_RPC_URL);
    test_mode_rpc.issue_block()?;

    Ok(())
}

pub fn get_account(
    godwoken_rpc_url: &str,
    privkey_path: &Path,
    config_path: &Path,
    deployment_results_path: &Path,
) -> Result<u32, String> {
    let mut full_node_godwoken_rpc = GodwokenRpcClient::new(godwoken_rpc_url);

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

    // // deposit to create a account
    // if from_id.is_err() {
    //     deposit_ckb::deposit_ckb(
    //         privkey_path,
    //         deployment_results_path,
    //         config_path,
    //         "10000",
    //         "0.0001",
    //         CKB_RPC_URL,
    //         None,
    //         FULL_NODE_MODE_GODWOKEN_RPC_URL,
    //     )?;
    // }

    from_id?.ok_or("get account error".to_owned())
}

pub fn submit_a_transaction(
    godwoken_rpc_url: &str,
    from_privkey_path: &Path,
    config_path: &Path,
    deployment_results_path: &Path,
    to: u32,
) -> Result<(), String> {
    transfer::submit_l2_transaction(
        godwoken_rpc_url,
        from_privkey_path,
        &to.to_string(),
        1u32,
        "100",
        "1",
        config_path,
        deployment_results_path,
    )
}
