use crate::fixed_bytes::Byte65;
use ckb_jsonrpc_types::{JsonBytes, Uint32, Uint64};
use ckb_types::{bytes::Bytes, H256};
use gw_types::{packed, prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct RawL2Transaction {
    pub from_id: Uint32,
    pub to_id: Uint32,
    pub nonce: Uint32,
    pub args: JsonBytes,
}

impl From<RawL2Transaction> for packed::RawL2Transaction {
    fn from(tx: RawL2Transaction) -> Self {
        let RawL2Transaction {
            from_id,
            to_id,
            nonce,
            args,
        } = tx;
        let from_id: u32 = from_id.into();
        let to_id: u32 = to_id.into();
        let nonce: u32 = nonce.into();
        let args: Bytes = args.into_bytes();
        packed::RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(to_id.pack())
            .nonce(nonce.pack())
            .args(args.pack())
            .build()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct L2Transaction {
    pub raw: RawL2Transaction,
    pub signature: Byte65,
}

impl From<L2Transaction> for packed::L2Transaction {
    fn from(tx: L2Transaction) -> Self {
        let L2Transaction { raw, signature } = tx;

        packed::L2Transaction::new_builder()
            .raw(raw.into())
            .signature(signature.into())
            .build()
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct L2TransactionView {
    #[serde(flatten)]
    pub inner: L2Transaction,
    pub hash: H256,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct StartChallenge {
    block_hash: H256,     // hash of challenged block
    block_number: Uint64, // number of challenged block
    tx_index: Uint32,     // challenge tx
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct CancelChallenge {
    l2block: L2Block,
    block_proof: Bytes,
    kv_state: Vec<KVPair>,
    kv_state_proof: Bytes,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct L2Block {
    raw: RawL2Block,
    signature: Byte65,
    kv_state: Vec<KVPair>,
    kv_state_proof: Bytes,
    transactions: Vec<L2Transaction>,
    block_proof: Bytes,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct RawL2Block {
    number: Uint64,
    aggregator_id: Uint32,
    stake_cell_owner_lock_hash: H256,
    timestamp: Uint64,
    prev_account: AccountMerkleState,
    post_account: AccountMerkleState,
    submit_transactions: Option<SubmitTransactions>,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct SubmitTransactions {
    tx_witness_root: H256,
    tx_count: Uint32,
    // hash(account_root | account_count) before each transaction
    compacted_post_root_list: Vec<H256>,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct AccountMerkleState {
    merkle_root: H256,
    count: Uint32,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct KVPair {
    k: H256,
    v: H256,
}
