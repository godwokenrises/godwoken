use crate::system_tests::utils::TestModeRpc;
use gw_jsonrpc_types::test_mode::ShouldProduceBlock;
use rand::Rng;
use std::thread;
use std::time::Duration;

pub struct TestModeConfig {
    pub godwoken_rpc_url: String,
    pub ckb_url: String,
    pub poll_interval: u64,
    pub issue_block_rand_range: u32,
}

pub fn run(config: TestModeConfig) -> Result<(), String> {
    let mut rng = rand::thread_rng();
    let mut test_mode_rpc = TestModeRpc::new(&config.godwoken_rpc_url);
    loop {
        let ret = test_mode_rpc.should_produce_block()?;
        if let ShouldProduceBlock::Yes = ret {
            let dice = rng.gen_range(0..config.issue_block_rand_range);
            match dice {
                0 => attack(&mut test_mode_rpc)?,
                _ => produce_normal_block(&mut test_mode_rpc)?,
            }
        }
        thread::sleep(Duration::from_secs(config.poll_interval))
    }
}

fn attack(test_mode_rpc: &mut TestModeRpc) -> Result<(), String> {
    let mut rng = rand::thread_rng();
    let dice = rng.gen_range(0..2);
    match dice {
        0 => issue_bad_challenge(test_mode_rpc)?,
        _ => produce_bad_block(test_mode_rpc)?,
    }
    test_mode_rpc.issue_block()
}

fn produce_normal_block(test_mode_rpc: &mut TestModeRpc) -> Result<(), String> {
    log::info!("produce normal block");
    test_mode_rpc.issue_block()
}

fn produce_bad_block(test_mode_rpc: &mut TestModeRpc) -> Result<(), String> {
    log::info!("produce bad block");
    test_mode_rpc.issue_block()
}

fn issue_bad_challenge(test_mode_rpc: &mut TestModeRpc) -> Result<(), String> {
    log::info!("issue bad challenge");
    test_mode_rpc.issue_block()
}
