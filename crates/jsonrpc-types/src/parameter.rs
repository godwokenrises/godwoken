use crate::blockchain::Script;
use crate::godwoken::{CancelChallenge, GlobalState, L2Block, StartChallenge};
use ckb_jsonrpc_types::{
    Script as CkbJsonScript, Transaction as CkbJsonTransaction, Uint128, Uint32, Uint64,
};
use ckb_types::{H160, H256};
use gw_chain::{chain, next_block_context};
use gw_generator::generator;
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
    /// transaction info
    pub transaction_info: TransactionInfo,
    /// transactions' header info
    pub header_info: HeaderInfo,
    pub context: L1ActionContext,
}

impl From<L1Action> for chain::L1Action {
    fn from(json: L1Action) -> chain::L1Action {
        let L1Action {
            transaction_info,
            header_info,
            context,
        } = json;
        Self {
            transaction_info: transaction_info.into(),
            header_info: header_info.into(),
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

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct TransactionInfo {
    pub transaction: CkbJsonTransaction,
    pub block_hash: H256,
}

impl From<TransactionInfo> for chain::TransactionInfo {
    fn from(json: TransactionInfo) -> chain::TransactionInfo {
        let TransactionInfo {
            transaction,
            block_hash,
        } = json;
        Self {
            transaction: transaction.into(),
            block_hash: block_hash.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct HeaderInfo {
    pub number: Uint64,
    pub block_hash: H256,
}

impl From<HeaderInfo> for chain::HeaderInfo {
    fn from(json: HeaderInfo) -> chain::HeaderInfo {
        let HeaderInfo { number, block_hash } = json;
        Self {
            number: number.into(),
            block_hash: block_hash.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum L1ActionContext {
    SubmitTxs {
        /// deposition requests
        deposition_requests: Vec<DepositionRequest>,
        /// withdrawal requests
        withdrawal_requests: Vec<WithdrawalRequest>,
    },
    Challenge {
        context: StartChallenge,
    },
    CancelChallenge {
        context: CancelChallenge,
    },
    Revert {
        context: StartChallenge,
    },
}

impl Default for L1ActionContext {
    fn default() -> Self {
        L1ActionContext::SubmitTxs {
            deposition_requests: vec![],
            withdrawal_requests: vec![],
        }
    }
}

impl From<L1ActionContext> for chain::L1ActionContext {
    fn from(json: L1ActionContext) -> chain::L1ActionContext {
        match json {
            L1ActionContext::SubmitTxs {
                deposition_requests,
                withdrawal_requests,
            } => chain::L1ActionContext::SubmitTxs {
                deposition_requests: deposition_requests.into_iter().map(|d| d.into()).collect(),
                withdrawal_requests: withdrawal_requests.into_iter().map(|w| w.into()).collect(),
            },
            L1ActionContext::Challenge {
                context: start_challenge,
            } => chain::L1ActionContext::Challenge {
                context: start_challenge.into(),
            },
            L1ActionContext::CancelChallenge {
                context: cancel_challenge,
            } => chain::L1ActionContext::CancelChallenge {
                context: cancel_challenge.into(),
            },
            L1ActionContext::Revert {
                context: cancel_challenge,
            } => chain::L1ActionContext::Revert {
                context: cancel_challenge.into(),
            },
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct DepositionRequest {
    pub script: Script,
    pub sudt_script: Script,
    pub amount: Uint128,
}

impl From<DepositionRequest> for generator::DepositionRequest {
    fn from(json: DepositionRequest) -> generator::DepositionRequest {
        let DepositionRequest {
            script,
            sudt_script,
            amount,
        } = json;
        Self {
            script: script.into(),
            sudt_script: sudt_script.into(),
            amount: amount.into(),
        }
    }
}

impl From<generator::DepositionRequest> for DepositionRequest {
    fn from(deposition_request: generator::DepositionRequest) -> DepositionRequest {
        let generator::DepositionRequest {
            script,
            sudt_script,
            amount,
        } = deposition_request;
        Self {
            script: script.into(),
            sudt_script: sudt_script.into(),
            amount: amount.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct WithdrawalRequest {
    // layer1 ACP cell to receive the withdraw
    pub lock_hash: H256,
    pub sudt_script_hash: H256,
    pub amount: Uint128,
    pub account_script_hash: H256,
}

impl From<WithdrawalRequest> for generator::WithdrawalRequest {
    fn from(json: WithdrawalRequest) -> generator::WithdrawalRequest {
        let WithdrawalRequest {
            lock_hash,
            sudt_script_hash,
            amount,
            account_script_hash,
        } = json;
        let lock_hash_array: [u8; 32] = lock_hash.into();
        let sudt_script_hash_array: [u8; 32] = sudt_script_hash.into();
        let account_script_hash_array: [u8; 32] = account_script_hash.into();
        Self {
            // lock_hash: gw_common::H256::from(json.lock_hash.into()),
            lock_hash: gw_common::H256::from(lock_hash_array),
            sudt_script_hash: gw_common::H256::from(sudt_script_hash_array),
            amount: amount.into(),
            account_script_hash: gw_common::H256::from(account_script_hash_array),
        }
    }
}

impl From<generator::WithdrawalRequest> for WithdrawalRequest {
    fn from(withdrawal_request: generator::WithdrawalRequest) -> WithdrawalRequest {
        let generator::WithdrawalRequest {
            lock_hash,
            sudt_script_hash,
            amount,
            account_script_hash,
        } = withdrawal_request;
        let lock_hash_array: [u8; 32] = lock_hash.into();
        let sudt_script_hash_array: [u8; 32] = sudt_script_hash.into();
        let account_script_hash_array: [u8; 32] = account_script_hash.into();
        Self {
            lock_hash: H256(lock_hash_array),
            sudt_script_hash: H256(sudt_script_hash_array),
            amount: amount.into(),
            account_script_hash: H256(account_script_hash_array),
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
    BadBlock(StartChallenge),
    // found a invalid challenge
    BadChallenge(CancelChallenge),
    // the rollup is in a challenge
    WaitChallenge,
}

impl From<chain::SyncEvent> for SyncEvent {
    fn from(sync_event: chain::SyncEvent) -> SyncEvent {
        match sync_event {
            chain::SyncEvent::Success => SyncEvent::Success,
            chain::SyncEvent::BadBlock(start_challenge) => {
                SyncEvent::BadBlock(start_challenge.into())
            }
            chain::SyncEvent::BadChallenge(cancel_challenge) => {
                SyncEvent::BadChallenge(cancel_challenge.into())
            }
            chain::SyncEvent::WaitChallenge => SyncEvent::WaitChallenge,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct ProduceBlockParam {
    /// aggregator of this block
    pub aggregator_id: Uint32,
    /// deposition requests
    pub deposition_requests: Vec<DepositionRequest>,
    /// user step2 withdrawal requests, collected from RPC
    pub withdrawal_requests: Vec<WithdrawalRequest>,
}

impl From<ProduceBlockParam> for chain::ProduceBlockParam {
    fn from(json: ProduceBlockParam) -> chain::ProduceBlockParam {
        let ProduceBlockParam {
            aggregator_id,
            deposition_requests,
            withdrawal_requests,
        } = json;
        Self {
            aggregator_id: aggregator_id.into(),
            deposition_requests: deposition_requests.into_iter().map(|d| d.into()).collect(),
            withdrawal_requests: withdrawal_requests.into_iter().map(|w| w.into()).collect(),
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
    pub initial_aggregator_pubkey_hash: H160,
    pub initial_deposition: Uint64,
    pub timestamp: Uint64,
}
impl From<GenesisConfig> for gw_config::GenesisConfig {
    fn from(json: GenesisConfig) -> gw_config::GenesisConfig {
        let GenesisConfig {
            initial_aggregator_pubkey_hash,
            initial_deposition,
            timestamp,
        } = json;
        Self {
            initial_aggregator_pubkey_hash: initial_aggregator_pubkey_hash.into(),
            initial_deposition: initial_deposition.into(),
            timestamp: timestamp.into(),
        }
    }
}
impl From<gw_config::GenesisConfig> for GenesisConfig {
    fn from(genesis_config: gw_config::GenesisConfig) -> GenesisConfig {
        let gw_config::GenesisConfig {
            initial_aggregator_pubkey_hash,
            initial_deposition,
            timestamp,
        } = genesis_config;
        Self {
            initial_aggregator_pubkey_hash: initial_aggregator_pubkey_hash.into(),
            initial_deposition: initial_deposition.into(),
            timestamp: timestamp.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct ChainConfig {
    pub rollup_type_script: CkbJsonScript,
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
pub struct L2BlockWithState {
    pub block: L2Block,
    pub global_state: GlobalState,
}

impl From<chain::L2BlockWithState> for L2BlockWithState {
    fn from(l2_block_with_state: chain::L2BlockWithState) -> L2BlockWithState {
        Self {
            block: l2_block_with_state.block.into(),
            global_state: l2_block_with_state.global_state.into(),
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

// impl From<RunResult> for gw_generator::RunResult {
//     fn from(json: RunResult) -> gw_generator::RunResult {
//         let RunResult {
//             read_values,
//             write_values,
//             return_data,
//             account_count,
//             new_scripts,
//             new_data
//         } = json;
//         Self {
//         }
//     }
// }

// impl From<gw_generator::RunResult> for RunResult {
//     fn from(run_result: gw_generator::RunResult) -> RunResult {
//         Self {}
//     }
// }
