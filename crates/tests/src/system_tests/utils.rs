use ckb_jsonrpc_types::{Uint32, Uint64};
use gw_jsonrpc_types::{
    godwoken::GlobalState,
    test_mode::TestModePayload,
    test_mode::{ChallengeType, ShouldProduceBlock},
};
use gw_tools::{
    account, deploy_scripts::ScriptsDeploymentResult, deposit_ckb, godwoken_rpc::GodwokenRpcClient,
    transfer, utils,
};
use rand::Rng;
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub enum TestModeControlType {
    BadBlock,
    NormalBlock,
    Challenge,
}

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

pub fn get_global_state(godwoken_rpc_url: &str) -> Result<GlobalState, String> {
    let mut test_mode_rpc = TestModeRpc::new(godwoken_rpc_url);
    test_mode_rpc.get_global_state()
}

pub fn issue_control(
    test_block_type: TestModeControlType,
    godwoken_rpc_url: &str,
    block_number: Option<u64>,
) -> Result<(), String> {
    log::info!("[test mode control]: issue test block");
    let mut test_mode_rpc = TestModeRpc::new(godwoken_rpc_url);
    let mut i = 0;
    while i < 1 {
        let ret = test_mode_rpc.should_produce_block()?;
        if let ShouldProduceBlock::Yes = ret {
            match test_block_type {
                TestModeControlType::BadBlock => {
                    test_mode_rpc.issue_bad_block(0, ChallengeType::TxSignature)?;
                    log::info!("issue bad block");
                }
                TestModeControlType::Challenge => {
                    let block_number = block_number.ok_or_else(|| "valid block number")?;
                    let challenge_type = ChallengeType::TxSignature;
                    test_mode_rpc.issue_challenge(block_number, 0, challenge_type)?;
                    log::info!(
                        "issue challenge: block number {}, target_index 0, ChallengeType {:?}",
                        block_number,
                        challenge_type
                    );
                }
                _ => {
                    test_mode_rpc.issue_block()?;
                    log::info!("issue normal block");
                }
            }
            i += 1;
        }
    }
    Ok(())
}

pub fn issue_blocks(godwoken_rpc_url: &str, count: i32) -> Result<(), String> {
    log::info!("[test mode control]: issue blocks");
    for i in 0..count {
        issue_control(TestModeControlType::NormalBlock, godwoken_rpc_url, None)?;
        log::info!("issue blocks: {}/{}", i + 1, count);
    }
    Ok(())
}

pub fn transfer_and_issue_block(
    test_block_type: TestModeControlType,
    from_privkey_path: &Path,
    to_privkey_path: &Path,
    config_path: &Path,
    deployment_results_path: &Path,
    godwoken_rpc_url: &str,
) -> Result<(), String> {
    log::info!("[test mode control]: transfer and issue block");
    let from_id = get_account(
        godwoken_rpc_url,
        from_privkey_path,
        config_path,
        deployment_results_path,
    )?;
    let to_id = get_account(
        godwoken_rpc_url,
        to_privkey_path,
        config_path,
        deployment_results_path,
    )?;
    let mut rng = rand::thread_rng();
    let amount = rng.gen_range(0..10);
    log::info!(
        "transfer: from id {} to id {} amount {}",
        from_id,
        to_id,
        &amount
    );
    transfer::submit_l2_transaction(
        godwoken_rpc_url,
        from_privkey_path,
        &to_id.to_string(),
        1u32,
        &amount.to_string(),
        "1",
        config_path,
        deployment_results_path,
    )?;
    issue_control(test_block_type, godwoken_rpc_url, None)?;
    Ok(())
}

pub fn deposit(
    privkey_path: &Path,
    deployment_results_path: &Path,
    config_path: &Path,
    ckb_rpc_url: &str,
    times: u32,
) -> Result<(), String> {
    log::info!("[test mode contro]: deposit");

    for _ in 0..times {
        deposit_ckb::deposit_ckb_to_layer1(
            privkey_path,
            deployment_results_path,
            config_path,
            "10000",
            "0.0001",
            ckb_rpc_url,
            None,
        )?;
    }

    Ok(())
}

fn get_account(
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
