use crate::system_tests::utils::{self, TestModeControlType, TestModeRpc};
use chrono::prelude::*;
use ckb_types::H256;
use gw_jsonrpc_types::{
    godwoken::{GlobalState, L2BlockStatus},
    test_mode::ShouldProduceBlock,
};
use gw_tools::godwoken_rpc::{self, GodwokenRpcClient};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestModeConfig {
    pub loop_interval_secs: u64,
    pub attack_rand_range: u32,
    pub track_record_interval_secs: u64,
    pub check_block_status_interval_min: i64,
    pub rpc_timeout_secs: u64,
    pub transfer_from_privkey_path: PathBuf,
    pub transfer_to_privkey_path: PathBuf,
    pub godwoken_rpc_url: String,
    pub ckb_url: String,
    pub godwoken_config_path: PathBuf,
    pub deployment_results_path: PathBuf,
    pub sentry_dsn: Option<String>,
}

#[derive(Debug)]
struct EventRecord {
    block_hash: String,
    block_number: u64,
    attack_type: AttackType,
    issue_time: DateTime<Utc>,
    check_time: Option<DateTime<Utc>>,
    block_status: Option<L2BlockStatus>,
    result: Option<Result<(), ()>>,
}

#[derive(Debug, Clone, Copy)]
enum AttackType {
    BadBlock,
    BadChallenge,
}

pub struct TestModeControl {
    config: TestModeConfig,
    records: HashMap<H256, EventRecord>,
    normal_block_requests: i32,
    bad_block_attacks: i32,
    bad_challenge_attacks: i32,
    error_number: i32,
}

impl TestModeControl {
    pub fn new(config: TestModeConfig) -> Self {
        TestModeControl {
            config,
            records: HashMap::new(),
            normal_block_requests: 0,
            bad_block_attacks: 0,
            bad_challenge_attacks: 0,
            error_number: 0,
        }
    }

    pub fn run(&mut self) {
        let mut rng = rand::thread_rng();
        let mut test_mode_rpc = TestModeRpc::new(&self.config.godwoken_rpc_url);
        let mut start_time = Instant::now();
        loop {
            let should_produce_block = test_mode_rpc.should_produce_block();
            if let Err(err) = should_produce_block {
                log::info!("Should produce block error: {}", err);
                std::thread::sleep(Duration::from_secs(self.config.loop_interval_secs));
                continue;
            }
            if let Ok(ShouldProduceBlock::Yes) = should_produce_block {
                let dice = rng.gen_range(0..self.config.attack_rand_range);
                match dice {
                    0 => {
                        if let Err(err) = self.attack() {
                            log::info!("Attack failed: {}", err);
                        }
                    }
                    _ => {
                        if let Err(err) = self.issue_normal_block() {
                            log::info!("Produce normal block failed: {}", err);
                        }
                    }
                }
            }
            if start_time.elapsed() >= Duration::from_secs(self.config.track_record_interval_secs) {
                if let Err(err) = self.track_record() {
                    log::info!("track record failed: {}", err);
                }
                start_time = Instant::now();
            }
            thread::sleep(Duration::from_secs(self.config.loop_interval_secs))
        }
    }

    fn issue_normal_block(&mut self) -> Result<(), String> {
        log::info!("produce normal block");
        utils::issue_blocks(&self.config.godwoken_rpc_url, 1)?;
        self.normal_block_requests += 1;
        Ok(())
    }

    fn attack(&mut self) -> Result<(), String> {
        let mut rng = rand::thread_rng();
        let dice = rng.gen_range(0..2);
        match dice {
            0 => self.issue_bad_block(),
            _ => self.issue_bad_challenge(),
        }
    }

    fn issue_bad_block(&mut self) -> Result<(), String> {
        log::info!("try to produce bad block");
        let global_state = utils::get_global_state(&self.config.godwoken_rpc_url)?;
        utils::transfer_and_issue_block(
            TestModeControlType::BadBlock,
            self.config.transfer_from_privkey_path.as_ref(),
            self.config.transfer_to_privkey_path.as_ref(),
            self.config.godwoken_config_path.as_ref(),
            self.config.deployment_results_path.as_ref(),
            &self.config.godwoken_rpc_url,
        )?;
        let block_number = self.wait_block_state_change(
            &self.config.godwoken_rpc_url,
            global_state,
            self.config.rpc_timeout_secs,
        )?;
        log::info!("issue bad block: {}", block_number);
        let mut block_hash_ret = Err("".to_owned());
        for _ in 0..10 {
            block_hash_ret =
                GodwokenRpcClient::new(&self.config.godwoken_rpc_url).get_block_hash(block_number);
            println!("block hash ret is: {:?}", block_hash_ret);
            if block_hash_ret.is_ok() {
                break;
            }
            thread::sleep(Duration::from_secs(5));
        }
        if let Ok(block_hash) = block_hash_ret {
            self.new_attack_record(block_hash, block_number, AttackType::BadBlock)
        } else {
            log::info!("record attack failed");
            Ok(())
        }
    }

    fn issue_bad_challenge(&mut self) -> Result<(), String> {
        log::info!("issue bad challenge");
        let global_state = utils::get_global_state(&self.config.godwoken_rpc_url)?;
        utils::transfer_and_issue_block(
            TestModeControlType::NormalBlock,
            &self.config.transfer_from_privkey_path,
            &self.config.transfer_to_privkey_path,
            &self.config.godwoken_config_path,
            &self.config.deployment_results_path,
            &self.config.godwoken_rpc_url,
        )?;
        let block_number = self.wait_block_state_change(
            &self.config.godwoken_rpc_url,
            global_state,
            self.config.rpc_timeout_secs,
        )?;
        log::info!("issue normal block: {}", block_number);
        utils::issue_control(
            TestModeControlType::Challenge,
            &self.config.godwoken_rpc_url,
            Some(block_number),
        )?;
        thread::sleep(Duration::from_secs(30));
        if let Ok(block_hash) =
            GodwokenRpcClient::new(&self.config.godwoken_rpc_url).get_block_hash(block_number)
        {
            self.new_attack_record(block_hash, block_number, AttackType::BadChallenge)
        } else {
            log::info!("record attack failed");
            Ok(())
        }
    }

    fn wait_block_state_change(
        &self,
        godwoken_rpc_url: &str,
        old_state: GlobalState,
        timeout_secs: u64,
    ) -> Result<u64, String> {
        let retry_timeout = Duration::from_secs(timeout_secs);
        let start_time = Instant::now();
        log::info!("wait state change...");
        while start_time.elapsed() < retry_timeout {
            std::thread::sleep(Duration::from_secs(2));
            let global_state = utils::get_global_state(godwoken_rpc_url)?;
            if global_state.block.count > old_state.block.count {
                let count: u64 = global_state.block.count.into();
                return Ok(count - 1u64);
            }
        }
        Err(format!("Timeout: {:?}", retry_timeout))
    }

    fn new_attack_record(
        &mut self,
        block_hash: H256,
        block_number: u64,
        attack_type: AttackType,
    ) -> Result<(), String> {
        let block_hash_string = hex::encode(&block_hash);
        let event_record = EventRecord {
            block_hash: block_hash_string,
            block_number,
            attack_type,
            issue_time: Utc::now(),
            check_time: None,
            block_status: None,
            result: None,
        };
        log::info!("new attack: {:?}", event_record);
        self.records.insert(block_hash, event_record);
        match attack_type {
            AttackType::BadBlock => self.bad_block_attacks += 1,
            AttackType::BadChallenge => self.bad_challenge_attacks += 1,
        }
        Ok(())
    }

    fn track_record(&mut self) -> Result<(), String> {
        let mut godwoken_rpc_client =
            godwoken_rpc::GodwokenRpcClient::new(&self.config.godwoken_rpc_url);
        let now = Utc::now();
        let track_items = self
            .records
            .iter()
            .filter(|(_, record)| {
                record.check_time.is_none()
                    && now.signed_duration_since(record.issue_time).num_minutes()
                        > self.config.check_block_status_interval_min
            })
            .map(|(block_hash, _)| block_hash.clone())
            .collect::<Vec<H256>>();
        for item in track_items {
            let entry = self.records.get_mut(&item).unwrap();
            entry.check_time = Some(now);
            entry.block_status = godwoken_rpc_client
                .get_block(&item)?
                .map(|block_with_status| block_with_status.status);
            entry.result = match entry.attack_type {
                AttackType::BadChallenge => {
                    if entry.block_status != Some(L2BlockStatus::Reverted) {
                        Some(Ok(()))
                    } else {
                        Some(Err(()))
                    }
                }
                AttackType::BadBlock => {
                    if entry.block_status == Some(L2BlockStatus::Reverted) {
                        Some(Ok(()))
                    } else {
                        Some(Err(()))
                    }
                }
            };
            if let Some(Err(())) = entry.result {
                let error_message = format!(
                    "Attack may be success. block hash: 0x{}\n {:#?}",
                    item, entry
                );
                sentry::capture_message(&error_message, sentry::Level::Error);
                self.error_number += 1;
            }
            log::info!("track attack: block hash: 0x{}\n {:?}", item, entry);
            self.records.remove(&item);
        }
        log::info!(
            "Normal block requests: {}, bad block attacks: {}, bad challenge attacks: {}, error_number: {}, unchecked attacks: {}",
            self.normal_block_requests,
            self.bad_block_attacks,
            self.bad_challenge_attacks,
            self.error_number,
            self.records.len(),
        );
        Ok(())
    }
}
