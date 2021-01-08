use crate::blockchain::Script as JsonScript;
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::{JsonBytes, Uint32, Uint64};
use gw_chain::{chain, next_block_context};
use gw_types::{core, packed, prelude::*};

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

use crate::godwoken::{CancelChallenge, ChallengeContext, TxReceipt};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct SyncParam {
    pub reverts: Vec<RevertedL1Action>,
    /// contains transitions from fork point to new tips
    pub updates: Vec<L1Action>,
    pub next_block_context: NextBlockContext,
}

impl From<SyncParam> for chain::SyncParam {
    fn from(json: SyncParam) -> chain::SyncParam {
        let SyncParam {
            reverts,
            updates,
            next_block_context,
        } = json;
        Self {
            reverts: reverts.into_iter().map(|r| r.into()).collect(),
            updates: updates.into_iter().map(|u| u.into()).collect(),
            next_block_context: next_block_context.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct L1Action {
    /// transaction
    pub transaction: JsonBytes,
    /// transactions' header info
    pub header_info: JsonBytes,
    pub context: L1ActionContext,
}

impl From<L1Action> for chain::L1Action {
    fn from(json: L1Action) -> chain::L1Action {
        let L1Action {
            transaction,
            header_info,
            context,
        } = json;
        // let transaction_slice: &[u8] = transaction.into_bytes().as_ref();
        let transaction_bytes = transaction.into_bytes();
        let header_info_bytes = header_info.into_bytes();
        Self {
            transaction: packed::Transaction::from_slice(transaction_bytes.as_ref())
                .expect("Build packed::Transaction from slice"),
            header_info: packed::HeaderInfo::from_slice(header_info_bytes.as_ref())
                .expect("Build packed::HeaderInfo from slice"),
            context: context.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct RevertedL1Action {
    /// prev global state
    pub prev_global_state: JsonBytes,
    /// transaction
    pub transaction: JsonBytes,
    /// transactions' header info
    pub header_info: JsonBytes,
    pub context: L1ActionContext,
}

impl From<RevertedL1Action> for chain::RevertedL1Action {
    fn from(json: RevertedL1Action) -> chain::RevertedL1Action {
        let RevertedL1Action {
            prev_global_state,
            transaction,
            header_info,
            context,
        } = json;
        let prev_global_state_bytes = prev_global_state.into_bytes();
        let transaction_bytes = transaction.into_bytes();
        let header_info_bytes = header_info.into_bytes();
        Self {
            prev_global_state: packed::GlobalState::from_slice(prev_global_state_bytes.as_ref())
                .expect("Build packed::GlobalState from slice"),
            transaction: packed::Transaction::from_slice(transaction_bytes.as_ref())
                .expect("Build packed::Transaction from slice"),
            header_info: packed::HeaderInfo::from_slice(header_info_bytes.as_ref())
                .expect("Build packed::HeaderInfo from slice"),
            context: context.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct NextBlockContext {
    pub aggregator_id: Uint32,
    pub timestamp: Uint64,
}

impl From<NextBlockContext> for next_block_context::NextBlockContext {
    fn from(json: NextBlockContext) -> next_block_context::NextBlockContext {
        let NextBlockContext {
            aggregator_id,
            timestamp,
        } = json;
        Self {
            aggregator_id: aggregator_id.into(),
            timestamp: timestamp.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum L1ActionContext {
    SubmitTxs {
        /// deposition requests
        deposition_requests: Vec<JsonBytes>,
    },
    Challenge {
        context: JsonBytes,
    },
    CancelChallenge {
        context: JsonBytes,
    },
    Revert {
        context: JsonBytes,
    },
}

impl Default for L1ActionContext {
    fn default() -> Self {
        L1ActionContext::SubmitTxs {
            deposition_requests: vec![],
        }
    }
}

impl From<L1ActionContext> for chain::L1ActionContext {
    fn from(json: L1ActionContext) -> chain::L1ActionContext {
        match json {
            L1ActionContext::SubmitTxs {
                deposition_requests,
            } => chain::L1ActionContext::SubmitTxs {
                deposition_requests: deposition_requests
                    .into_iter()
                    .map(|d| {
                        let d_bytes = d.into_bytes();
                        packed::DepositionRequest::from_slice(d_bytes.as_ref())
                            .expect("Build packed::DepositionRequest from slice")
                    })
                    .collect(),
            },
            L1ActionContext::Challenge {
                context: start_challenge,
            } => {
                let start_challenge_bytes = start_challenge.into_bytes();
                chain::L1ActionContext::Challenge {
                    context: packed::StartChallenge::from_slice(start_challenge_bytes.as_ref())
                        .expect("Build packed::StartChallenge from slice"),
                }
            }
            L1ActionContext::CancelChallenge {
                context: cancel_challenge,
            } => {
                let cancel_challenge_bytes = cancel_challenge.into_bytes();
                chain::L1ActionContext::CancelChallenge {
                    context: packed::CancelChallenge::from_slice(cancel_challenge_bytes.as_ref())
                        .expect("Build packed::CancelChallenge from slice"),
                }
            }
            L1ActionContext::Revert {
                context: start_challenge,
            } => {
                let start_challenge_bytes = start_challenge.into_bytes();
                chain::L1ActionContext::Revert {
                    context: packed::StartChallenge::from_slice(start_challenge_bytes.as_ref())
                        .expect("Build packed::StartChallenge from slice"),
                }
            }
        }
    }
}

/// sync method returned events
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SyncEvent {
    // success
    Success,
    // found a invalid block
    BadBlock {
        context: ChallengeContext,
    },
    // found a invalid challenge
    BadChallenge {
        witness: CancelChallenge,
        tx_receipt: TxReceipt,
    },
    // the rollup is in a challenge
    WaitChallenge,
}

impl From<chain::SyncEvent> for SyncEvent {
    fn from(sync_event: chain::SyncEvent) -> SyncEvent {
        match sync_event {
            chain::SyncEvent::Success => SyncEvent::Success,
            chain::SyncEvent::BadBlock(challenge_context) => SyncEvent::BadBlock {
                context: challenge_context.into(),
            },
            chain::SyncEvent::BadChallenge {
                witness,
                tx_receipt,
            } => SyncEvent::BadChallenge {
                witness: witness.into(),
                tx_receipt: tx_receipt.into(),
            },
            chain::SyncEvent::WaitChallenge => SyncEvent::WaitChallenge,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct ProduceBlockParam {
    /// aggregator of this block
    pub aggregator_id: Uint32,
    pub deposition_requests: Vec<JsonBytes>,
}

impl From<ProduceBlockParam> for chain::ProduceBlockParam {
    fn from(json: ProduceBlockParam) -> chain::ProduceBlockParam {
        let ProduceBlockParam {
            aggregator_id,
            deposition_requests,
        } = json;
        Self {
            aggregator_id: aggregator_id.into(),
            deposition_requests: deposition_requests
                .into_iter()
                .map(|d| {
                    let d_bytes = d.into_bytes();
                    packed::DepositionRequest::from_slice(d_bytes.as_ref())
                        .expect("Build packed::DepositionRequest from slice")
                })
                .collect(),
        }
    }
}
impl From<chain::ProduceBlockParam> for ProduceBlockParam {
    fn from(json: chain::ProduceBlockParam) -> ProduceBlockParam {
        let chain::ProduceBlockParam {
            aggregator_id,
            deposition_requests,
        } = json;
        Self {
            aggregator_id: aggregator_id.into(),
            deposition_requests: deposition_requests
                .into_iter()
                .map(|d| JsonBytes::from_bytes(d.as_bytes()))
                .collect(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Halting,
}

impl Default for Status {
    fn default() -> Self {
        Status::Running
    }
}

impl From<Status> for core::Status {
    fn from(json: Status) -> Self {
        match json {
            Status::Running => core::Status::Running,
            Status::Halting => core::Status::Halting,
        }
    }
}
impl From<core::Status> for Status {
    fn from(status: core::Status) -> Self {
        match status {
            core::Status::Running => Status::Running,
            core::Status::Halting => Status::Halting,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct Config {
    pub chain: ChainConfig,
    pub store: StoreConfig,
    pub genesis: GenesisConfig,
    pub aggregator: Option<AggregatorConfig>,
}

impl From<Config> for gw_config::Config {
    fn from(json: Config) -> gw_config::Config {
        Self {
            chain: json.chain.into(),
            store: json.store.into(),
            genesis: json.genesis.into(),
            aggregator: match json.aggregator {
                Some(aggregator) => Some(aggregator.into()),
                None => None,
            },
        }
    }
}
impl From<gw_config::Config> for Config {
    fn from(config: gw_config::Config) -> Config {
        Self {
            chain: config.chain.into(),
            store: config.store.into(),
            genesis: config.genesis.into(),
            aggregator: match config.aggregator {
                Some(aggregator) => Some(aggregator.into()),
                None => None,
            },
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct AggregatorConfig {
    pub account_id: Uint32,
}

impl From<AggregatorConfig> for gw_config::AggregatorConfig {
    fn from(json: AggregatorConfig) -> gw_config::AggregatorConfig {
        Self {
            account_id: json.account_id.into(),
        }
    }
}
impl From<gw_config::AggregatorConfig> for AggregatorConfig {
    fn from(aggregator_config: gw_config::AggregatorConfig) -> AggregatorConfig {
        Self {
            account_id: aggregator_config.account_id.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct GenesisConfig {
    pub timestamp: Uint64,
}
impl From<GenesisConfig> for gw_config::GenesisConfig {
    fn from(json: GenesisConfig) -> gw_config::GenesisConfig {
        let GenesisConfig { timestamp } = json;
        Self {
            timestamp: timestamp.into(),
        }
    }
}
impl From<gw_config::GenesisConfig> for GenesisConfig {
    fn from(genesis_config: gw_config::GenesisConfig) -> GenesisConfig {
        let gw_config::GenesisConfig { timestamp } = genesis_config;
        Self {
            timestamp: timestamp.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct StoreConfig {
    pub path: PathBuf,
}

impl From<StoreConfig> for gw_config::StoreConfig {
    fn from(json: StoreConfig) -> gw_config::StoreConfig {
        Self {
            path: json.path.into(),
        }
    }
}
impl From<gw_config::StoreConfig> for StoreConfig {
    fn from(config: gw_config::StoreConfig) -> StoreConfig {
        Self {
            path: config.path.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct ChainConfig {
    pub rollup_type_script: JsonScript,
}

impl From<ChainConfig> for gw_config::ChainConfig {
    fn from(json: ChainConfig) -> gw_config::ChainConfig {
        Self {
            rollup_type_script: json.rollup_type_script.into(),
        }
    }
}
impl From<gw_config::ChainConfig> for ChainConfig {
    fn from(chain_config: gw_config::ChainConfig) -> ChainConfig {
        Self {
            rollup_type_script: chain_config.rollup_type_script.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct ProduceBlockResult {
    pub block: JsonBytes,
    pub global_state: JsonBytes,
}

impl From<chain::ProduceBlockResult> for ProduceBlockResult {
    fn from(produce_block_result: chain::ProduceBlockResult) -> ProduceBlockResult {
        let block_bytes = produce_block_result.block.as_bytes();
        let global_state_bytes = produce_block_result.global_state.as_bytes();
        Self {
            block: JsonBytes::from_bytes(block_bytes),
            global_state: JsonBytes::from_bytes(global_state_bytes),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct RunResult {
    pub read_values: HashMap<H256, H256>,
    pub write_values: HashMap<H256, H256>,
    pub return_data: JsonBytes,
    pub account_count: Option<Uint32>,
    pub new_scripts: HashMap<H256, JsonBytes>,
    pub write_data: HashMap<H256, JsonBytes>,
    pub read_data: HashMap<H256, Uint32>,
}

impl From<RunResult> for gw_generator::RunResult {
    fn from(json: RunResult) -> gw_generator::RunResult {
        let RunResult {
            read_values,
            write_values,
            return_data,
            account_count,
            new_scripts,
            write_data,
            read_data,
        } = json;
        let mut to_read_values: HashMap<gw_common::H256, gw_common::H256> = HashMap::new();
        for (k, v) in read_values.iter() {
            to_read_values.insert(k.0.into(), v.0.into());
        }
        let mut to_write_values: HashMap<gw_common::H256, gw_common::H256> = HashMap::new();
        for (k, v) in write_values.iter() {
            to_write_values.insert(k.0.into(), v.0.into());
        }
        let to_account_count = match account_count {
            Some(count) => Some(u32::from(count)),
            None => None,
        };
        let mut to_new_scripts: HashMap<gw_common::H256, Vec<u8>> = HashMap::new();
        for (k, v) in new_scripts.iter() {
            to_new_scripts.insert(k.0.into(), v.as_bytes().to_vec());
        }
        let mut to_write_data: HashMap<gw_common::H256, Vec<u8>> = HashMap::new();
        for (k, v) in write_data.iter() {
            to_write_data.insert(k.0.into(), v.as_bytes().to_vec());
        }
        let read_data = read_data
            .into_iter()
            .map(|(k, v)| {
                let key: gw_common::H256 = k.0.into();
                let v: u32 = v.into();
                (key, v as usize)
            })
            .collect();
        Self {
            read_values: to_read_values,
            write_values: to_write_values,
            return_data: return_data.as_bytes().to_vec(),
            account_count: to_account_count,
            new_scripts: to_new_scripts,
            write_data: to_write_data,
            read_data,
        }
    }
}

impl From<gw_generator::RunResult> for RunResult {
    fn from(run_result: gw_generator::RunResult) -> RunResult {
        let gw_generator::RunResult {
            read_values,
            write_values,
            return_data,
            account_count,
            new_scripts,
            write_data,
            read_data,
        } = run_result;
        let mut to_read_values: HashMap<H256, H256> = HashMap::new();
        for (k, v) in read_values.iter() {
            to_read_values.insert(
                H256((*k as gw_common::H256).into()),
                H256((*v as gw_common::H256).into()),
            );
        }
        let mut to_write_values: HashMap<H256, H256> = HashMap::new();
        for (k, v) in write_values.iter() {
            to_write_values.insert(
                H256((*k as gw_common::H256).into()),
                H256((*v as gw_common::H256).into()),
            );
        }
        let to_account_count = match account_count {
            Some(count) => Some(count.into()),
            None => None,
        };
        let mut to_new_scripts: HashMap<H256, JsonBytes> = HashMap::new();
        for (k, v) in new_scripts.iter() {
            to_new_scripts.insert(
                H256((*k as gw_common::H256).into()),
                JsonBytes::from_vec(v.to_vec()),
            );
        }
        let mut to_write_data: HashMap<H256, JsonBytes> = HashMap::new();
        for (k, v) in write_data.iter() {
            to_write_data.insert(
                H256((*k as gw_common::H256).into()),
                JsonBytes::from_vec(v.to_vec()),
            );
        }
        let read_data = read_data
            .into_iter()
            .map(|(k, v)| {
                let key: [u8; 32] = k.into();
                let value = v as u32;
                (key.into(), value.into())
            })
            .collect();
        Self {
            read_values: to_read_values,
            write_values: to_write_values,
            return_data: JsonBytes::from_vec(return_data),
            account_count: to_account_count,
            new_scripts: to_new_scripts,
            write_data: to_write_data,
            read_data,
        }
    }
}
