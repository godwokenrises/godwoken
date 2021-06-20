use ckb_jsonrpc_types::{Uint32, Uint64};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ShouldProduceBlock {
    Yes,
    YesIfFull,
    No,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeType {
    TxExecution,
    TxSignature,
    WithdrawalSignature,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TestModePayload {
    None,
    BadBlock {
        target_index: Uint32,
        target_type: ChallengeType,
    },
    Challenge {
        block_number: Uint64,
        target_index: Uint32,
        target_type: ChallengeType,
    },
    WaitForChallengeMaturity,
}
