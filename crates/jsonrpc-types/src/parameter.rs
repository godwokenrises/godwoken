use ckb_jsonrpc_types::{JsonBytes, Script as JsonScript, Uint32, Uint64};
use ckb_types::packed as ckb_packed;
use ckb_types::H256;
use gw_chain::{chain, next_block_context, tx_pool};
use gw_types::{
    packed::{CancelChallenge, DepositionRequest, HeaderInfo, StartChallenge},
    prelude::*,
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct SyncParam {
    pub reverts: Vec<L1Action>,
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
            transaction: ckb_packed::Transaction::from_slice(transaction_bytes.as_ref())
                .expect("Build packed::Transaction from slice"),
            header_info: HeaderInfo::from_slice(header_info_bytes.as_ref())
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
                        DepositionRequest::from_slice(d_bytes.as_ref())
                            .expect("Build packed::DepositionRequest from slice")
                    })
                    .collect(),
            },
            L1ActionContext::Challenge {
                context: start_challenge,
            } => {
                let start_challenge_bytes = start_challenge.into_bytes();
                chain::L1ActionContext::Challenge {
                    context: StartChallenge::from_slice(start_challenge_bytes.as_ref())
                        .expect("Build packed::StartChallenge from slice"),
                }
            }
            L1ActionContext::CancelChallenge {
                context: cancel_challenge,
            } => {
                let cancel_challenge_bytes = cancel_challenge.into_bytes();
                chain::L1ActionContext::CancelChallenge {
                    context: CancelChallenge::from_slice(cancel_challenge_bytes.as_ref())
                        .expect("Build packed::CancelChallenge from slice"),
                }
            }
            L1ActionContext::Revert {
                context: start_challenge,
            } => {
                let start_challenge_bytes = start_challenge.into_bytes();
                chain::L1ActionContext::Revert {
                    context: StartChallenge::from_slice(start_challenge_bytes.as_ref())
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
    BadBlock { context: JsonBytes },
    // found a invalid challenge
    BadChallenge { context: JsonBytes },
    // the rollup is in a challenge
    WaitChallenge,
}

impl From<chain::SyncEvent> for SyncEvent {
    fn from(sync_event: chain::SyncEvent) -> SyncEvent {
        match sync_event {
            chain::SyncEvent::Success => SyncEvent::Success,
            chain::SyncEvent::BadBlock(start_challenge) => SyncEvent::BadBlock {
                context: JsonBytes::from_bytes(start_challenge.as_bytes()),
            },
            chain::SyncEvent::BadChallenge(cancel_challenge) => SyncEvent::BadBlock {
                context: JsonBytes::from_bytes(cancel_challenge.as_bytes()),
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
                    DepositionRequest::from_slice(d_bytes.as_ref())
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

impl From<Status> for chain::Status {
    fn from(json: Status) -> Self {
        match json {
            Status::Running => chain::Status::Running,
            Status::Halting => chain::Status::Halting,
        }
    }
}
impl From<chain::Status> for Status {
    fn from(status: chain::Status) -> Self {
        match status {
            chain::Status::Running => Status::Running,
            chain::Status::Halting => Status::Halting,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct Config {
    pub chain: ChainConfig,
    pub consensus: ConsensusConfig,
    pub rpc: RPC,
    pub genesis: GenesisConfig,
    pub aggregator: Option<AggregatorConfig>,
}

impl From<Config> for gw_config::Config {
    fn from(json: Config) -> gw_config::Config {
        Self {
            chain: json.chain.into(),
            consensus: json.consensus.into(),
            rpc: json.rpc.into(),
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
            consensus: config.consensus.into(),
            rpc: config.rpc.into(),
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
    pub signer: SignerConfig,
}

impl From<AggregatorConfig> for gw_config::AggregatorConfig {
    fn from(json: AggregatorConfig) -> gw_config::AggregatorConfig {
        Self {
            account_id: json.account_id.into(),
            signer: json.signer.into(),
        }
    }
}
impl From<gw_config::AggregatorConfig> for AggregatorConfig {
    fn from(aggregator_config: gw_config::AggregatorConfig) -> AggregatorConfig {
        Self {
            account_id: aggregator_config.account_id.into(),
            signer: aggregator_config.signer.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct SignerConfig {}

impl From<SignerConfig> for gw_config::SignerConfig {
    fn from(_json: SignerConfig) -> gw_config::SignerConfig {
        Self {}
    }
}
impl From<gw_config::SignerConfig> for SignerConfig {
    fn from(_signer_config: gw_config::SignerConfig) -> SignerConfig {
        Self {}
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct ConsensusConfig {
    pub aggregator_id: Uint32,
}

impl From<ConsensusConfig> for gw_config::ConsensusConfig {
    fn from(json: ConsensusConfig) -> gw_config::ConsensusConfig {
        Self {
            aggregator_id: json.aggregator_id.into(),
        }
    }
}
impl From<gw_config::ConsensusConfig> for ConsensusConfig {
    fn from(consensus_config: gw_config::ConsensusConfig) -> ConsensusConfig {
        Self {
            aggregator_id: consensus_config.aggregator_id.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct GenesisConfig {
    pub initial_aggregator_script: JsonScript,
    pub initial_deposition: Uint64,
    pub timestamp: Uint64,
}
impl From<GenesisConfig> for gw_config::GenesisConfig {
    fn from(json: GenesisConfig) -> gw_config::GenesisConfig {
        let GenesisConfig {
            initial_aggregator_script,
            initial_deposition,
            timestamp,
        } = json;
        Self {
            initial_aggregator_script: initial_aggregator_script.into(),
            initial_deposition: initial_deposition.into(),
            timestamp: timestamp.into(),
        }
    }
}
impl From<gw_config::GenesisConfig> for GenesisConfig {
    fn from(genesis_config: gw_config::GenesisConfig) -> GenesisConfig {
        let gw_config::GenesisConfig {
            initial_aggregator_script,
            initial_deposition,
            timestamp,
        } = genesis_config;
        Self {
            initial_aggregator_script: initial_aggregator_script.into(),
            initial_deposition: initial_deposition.into(),
            timestamp: timestamp.into(),
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
pub struct RPC {
    pub listen: String,
}

impl From<RPC> for gw_config::RPC {
    fn from(json: RPC) -> gw_config::RPC {
        Self {
            listen: json.listen,
        }
    }
}
impl From<gw_config::RPC> for RPC {
    fn from(rpc: gw_config::RPC) -> RPC {
        Self { listen: rpc.listen }
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
    pub return_data: Vec<u8>,
    pub account_count: Option<Uint32>,
    pub new_scripts: HashMap<H256, Vec<u8>>,
    pub new_data: HashMap<H256, Vec<u8>>,
}

impl From<RunResult> for gw_generator::RunResult {
    fn from(json: RunResult) -> gw_generator::RunResult {
        let RunResult {
            read_values,
            write_values,
            return_data,
            account_count,
            new_scripts,
            new_data,
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
            to_new_scripts.insert(k.0.into(), v.to_vec());
        }
        let mut to_new_data: HashMap<gw_common::H256, Vec<u8>> = HashMap::new();
        for (k, v) in new_data.iter() {
            to_new_data.insert(k.0.into(), v.to_vec());
        }
        Self {
            read_values: to_read_values,
            write_values: to_write_values,
            return_data: return_data,
            account_count: to_account_count,
            new_scripts: to_new_scripts,
            new_data: to_new_data,
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
            new_data,
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
        let mut to_new_scripts: HashMap<H256, Vec<u8>> = HashMap::new();
        for (k, v) in new_scripts.iter() {
            to_new_scripts.insert(H256((*k as gw_common::H256).into()), v.to_vec());
        }
        let mut to_new_data: HashMap<H256, Vec<u8>> = HashMap::new();
        for (k, v) in new_data.iter() {
            to_new_data.insert(H256((*k as gw_common::H256).into()), v.to_vec());
        }
        Self {
            read_values: to_read_values,
            write_values: to_write_values,
            return_data: return_data,
            account_count: to_account_count,
            new_scripts: to_new_scripts,
            new_data: to_new_data,
        }
    }
}
