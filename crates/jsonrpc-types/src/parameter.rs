use crate::layer2::{CancelChallenge, StartChallenge};
use ckb_jsonrpc_types::{JsonBytes, Script, Transaction, Uint128, Uint32, Uint64};
use ckb_types::{core, prelude::*, H256};
use gw_types::{packed, prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct SyncParam {
    pub reverts: Vec<L1Action>,
    /// contains transitions from fork point to new tips
    pub updates: Vec<L1Action>,
    pub next_block_context: NextBlockContext,
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

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct NextBlockContext {
    pub aggregator_id: Uint32,
    pub timestamp: Uint64,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct TransactionInfo {
    pub transaction: Transaction,
    pub block_hash: H256,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct HeaderInfo {
    pub number: Uint64,
    pub block_hash: H256,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
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

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct DepositionRequest {
    pub script: Script,
    pub sudt_script: Script,
    pub amount: Uint128,
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
