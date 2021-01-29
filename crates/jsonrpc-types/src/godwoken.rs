use crate::blockchain::Script;
use crate::fixed_bytes::Byte65;
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::{JsonBytes, Uint128, Uint32, Uint64};
use failure::{err_msg, Error as FailureError};
use gw_types::{bytes::Bytes, packed, prelude::*};
use serde::{Deserialize, Serialize};
use std::convert::{TryFrom, TryInto};

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
        let args: Bytes = args.into_bytes();
        packed::RawL2Transaction::new_builder()
            .from_id(u32::from(from_id).pack())
            .to_id(u32::from(to_id).pack())
            .nonce(u32::from(nonce).pack())
            .args(args.pack())
            .build()
    }
}

impl From<packed::RawL2Transaction> for RawL2Transaction {
    fn from(raw_l2_transaction: packed::RawL2Transaction) -> RawL2Transaction {
        let from_id: u32 = raw_l2_transaction.from_id().unpack();
        let to_id: u32 = raw_l2_transaction.to_id().unpack();
        let nonce: u32 = raw_l2_transaction.nonce().unpack();
        Self {
            from_id: from_id.into(),
            to_id: to_id.into(),
            nonce: nonce.into(),
            args: JsonBytes::from_bytes(raw_l2_transaction.args().unpack()),
        }
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

impl From<packed::L2Transaction> for L2Transaction {
    fn from(l2_transaction: packed::L2Transaction) -> L2Transaction {
        Self {
            raw: l2_transaction.raw().into(),
            signature: l2_transaction.signature().into(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct L2TransactionView {
    #[serde(flatten)]
    pub inner: L2Transaction,
    pub hash: H256,
}

impl From<packed::L2Transaction> for L2TransactionView {
    fn from(l2_tx: packed::L2Transaction) -> L2TransactionView {
        let hash = H256::from(l2_tx.raw().hash());
        let inner = L2Transaction::from(l2_tx);
        L2TransactionView { inner, hash }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub struct LogItem {
    pub account_id: Uint32,
    pub data: JsonBytes,
}

impl From<LogItem> for packed::LogItem {
    fn from(json: LogItem) -> packed::LogItem {
        let LogItem { account_id, data } = json;
        packed::LogItem::new_builder()
            .account_id(account_id.value().pack())
            .data(data.into_bytes().pack())
            .build()
    }
}

impl From<packed::LogItem> for LogItem {
    fn from(data: packed::LogItem) -> LogItem {
        let account_id: u32 = data.account_id().unpack();
        let data = JsonBytes::from_bytes(data.data().unpack());
        LogItem {
            account_id: Uint32::from(account_id),
            data,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct TxReceipt {
    pub tx_witness_hash: H256,
    pub compacted_post_account_root: H256,
    pub read_data_hashes: Vec<H256>,
    pub logs: Vec<LogItem>,
}

impl From<TxReceipt> for packed::TxReceipt {
    fn from(json: TxReceipt) -> packed::TxReceipt {
        let TxReceipt {
            tx_witness_hash,
            compacted_post_account_root,
            read_data_hashes,
            logs,
        } = json;
        let tx_witness_hash: [u8; 32] = tx_witness_hash.into();
        let compacted_post_account_root: [u8; 32] = compacted_post_account_root.into();
        let read_data_hashes: Vec<_> = read_data_hashes
            .into_iter()
            .map(|hash| {
                let hash: [u8; 32] = hash.into();
                hash.pack()
            })
            .collect();
        let logs: Vec<packed::LogItem> = logs.into_iter().map(|item| item.into()).collect();
        packed::TxReceipt::new_builder()
            .tx_witness_hash(tx_witness_hash.pack())
            .compacted_post_account_root(compacted_post_account_root.pack())
            .read_data_hashes(read_data_hashes.pack())
            .logs(logs.pack())
            .build()
    }
}

impl From<packed::TxReceipt> for TxReceipt {
    fn from(data: packed::TxReceipt) -> TxReceipt {
        let tx_witness_hash: [u8; 32] = data.tx_witness_hash().unpack();
        let compacted_post_account_root: [u8; 32] = data.compacted_post_account_root().unpack();
        let read_data_hashes: Vec<_> = data
            .read_data_hashes()
            .into_iter()
            .map(|hash| {
                let hash: [u8; 32] = hash.unpack();
                hash.into()
            })
            .collect();
        let logs: Vec<LogItem> = data.logs().into_iter().map(|item| item.into()).collect();
        TxReceipt {
            tx_witness_hash: tx_witness_hash.into(),
            compacted_post_account_root: compacted_post_account_root.into(),
            read_data_hashes,
            logs,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeTargetType {
    Transaction,
    Withdrawal,
}

impl Default for ChallengeTargetType {
    fn default() -> Self {
        Self::Transaction
    }
}

impl From<ChallengeTargetType> for packed::Byte {
    fn from(json: ChallengeTargetType) -> packed::Byte {
        match json {
            ChallengeTargetType::Transaction => packed::Byte::new(0),
            ChallengeTargetType::Withdrawal => packed::Byte::new(1),
        }
    }
}
impl TryFrom<packed::Byte> for ChallengeTargetType {
    type Error = FailureError;

    fn try_from(v: packed::Byte) -> Result<ChallengeTargetType, Self::Error> {
        match u8::from(v) {
            0 => Ok(ChallengeTargetType::Transaction),
            1 => Ok(ChallengeTargetType::Withdrawal),
            _ => Err(err_msg(format!("Invalid challenge target type {}", v))),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct ChallengeTarget {
    pub block_hash: H256,                 // hash of challenged block
    pub target_index: Uint32,             // target index
    pub target_type: ChallengeTargetType, // target type
}

impl From<ChallengeTarget> for packed::ChallengeTarget {
    fn from(json: ChallengeTarget) -> packed::ChallengeTarget {
        let ChallengeTarget {
            block_hash,
            target_index,
            target_type,
        } = json;
        packed::ChallengeTarget::new_builder()
            .block_hash(block_hash.pack())
            .target_index(u32::from(target_index).pack())
            .target_type(target_type.into())
            .build()
    }
}

impl From<packed::ChallengeTarget> for ChallengeTarget {
    fn from(challenge_target: packed::ChallengeTarget) -> ChallengeTarget {
        let target_index: u32 = challenge_target.target_index().unpack();
        let target_type: packed::Byte = challenge_target.target_type().into();
        Self {
            block_hash: challenge_target.block_hash().unpack(),
            target_index: Uint32::from(target_index),
            target_type: target_type.try_into().expect("invalid target type"),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct ChallengeWitness {
    pub raw_l2block: RawL2Block,
    pub block_proof: JsonBytes, // block proof
}

impl From<ChallengeWitness> for packed::ChallengeWitness {
    fn from(json: ChallengeWitness) -> packed::ChallengeWitness {
        let ChallengeWitness {
            raw_l2block,
            block_proof,
        } = json;
        let raw_l2block: packed::RawL2Block = raw_l2block.into();
        packed::ChallengeWitness::new_builder()
            .raw_l2block(raw_l2block)
            .block_proof(block_proof.into_bytes().pack())
            .build()
    }
}

impl From<packed::ChallengeWitness> for ChallengeWitness {
    fn from(data: packed::ChallengeWitness) -> ChallengeWitness {
        let raw_l2block: RawL2Block = data.raw_l2block().into();
        let block_proof = JsonBytes::from_bytes(data.block_proof().unpack());
        Self {
            raw_l2block,
            block_proof,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct ChallengeContext {
    pub target: ChallengeTarget,
    pub witness: ChallengeWitness,
}

impl From<ChallengeContext> for gw_generator::ChallengeContext {
    fn from(json: ChallengeContext) -> gw_generator::ChallengeContext {
        let ChallengeContext { target, witness } = json;
        let target: packed::ChallengeTarget = target.into();
        let witness: packed::ChallengeWitness = witness.into();
        gw_generator::ChallengeContext { target, witness }
    }
}

impl From<gw_generator::ChallengeContext> for ChallengeContext {
    fn from(data: gw_generator::ChallengeContext) -> ChallengeContext {
        let gw_generator::ChallengeContext { target, witness } = data;
        let target: ChallengeTarget = target.into();
        let witness: ChallengeWitness = witness.into();
        Self { target, witness }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct VerifyTransactionWitness {
    pub raw_l2block: RawL2Block,
    pub l2tx: L2Transaction,
    pub kv_state: Vec<KVPair>,
    pub kv_state_proof: JsonBytes,
    pub scripts: Vec<Script>,
    pub return_data_hash: H256,
    pub tx_proof: JsonBytes,
}

impl From<VerifyTransactionWitness> for packed::VerifyTransactionWitness {
    fn from(json: VerifyTransactionWitness) -> packed::VerifyTransactionWitness {
        let VerifyTransactionWitness {
            raw_l2block,
            l2tx,
            kv_state,
            kv_state_proof,
            scripts,
            return_data_hash,
            tx_proof,
        } = json;
        let kv_pair_vec: Vec<packed::KVPair> = kv_state.into_iter().map(|k| k.into()).collect();
        let packed_kv_state_vec = packed::KVPairVec::new_builder().set(kv_pair_vec).build();
        let script_vec: Vec<packed::Script> = scripts.into_iter().map(|s| s.into()).collect();
        let packed_script_vec = packed::ScriptVec::new_builder().set(script_vec).build();

        packed::VerifyTransactionWitness::new_builder()
            .raw_l2block(raw_l2block.into())
            .l2tx(l2tx.into())
            .kv_state(packed_kv_state_vec)
            .kv_state_proof(kv_state_proof.into_bytes().pack())
            .scripts(packed_script_vec)
            .return_data_hash(return_data_hash.pack())
            .tx_proof(tx_proof.into_bytes().pack())
            .build()
    }
}

impl From<packed::VerifyTransactionWitness> for VerifyTransactionWitness {
    fn from(data: packed::VerifyTransactionWitness) -> VerifyTransactionWitness {
        let kv_state: Vec<KVPair> = data.kv_state().into_iter().map(|k| k.into()).collect();
        let scripts: Vec<Script> = data.scripts().into_iter().map(|s| s.into()).collect();

        VerifyTransactionWitness {
            raw_l2block: data.raw_l2block().into(),
            l2tx: data.l2tx().into(),
            kv_state,
            kv_state_proof: JsonBytes::from_bytes(data.kv_state_proof().unpack()),
            scripts,
            return_data_hash: {
                let return_data_hash: [u8; 32] = data.return_data_hash().unpack();
                return_data_hash.into()
            },
            tx_proof: JsonBytes::from_bytes(data.tx_proof().unpack()),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct L2Block {
    pub raw: RawL2Block,
    pub kv_state: Vec<KVPair>,
    pub kv_state_proof: JsonBytes,
    pub transactions: Vec<L2Transaction>,
    pub block_proof: JsonBytes,
    pub withdrawal_requests: Vec<WithdrawalRequest>,
}

impl From<L2Block> for packed::L2Block {
    fn from(json: L2Block) -> packed::L2Block {
        let L2Block {
            raw,
            kv_state,
            kv_state_proof,
            transactions,
            block_proof,
            withdrawal_requests,
        } = json;
        let kv_pair_vec: Vec<packed::KVPair> = kv_state.into_iter().map(|k| k.into()).collect();
        let packed_kv_state = packed::KVPairVec::new_builder().set(kv_pair_vec).build();
        let transaction_vec: Vec<packed::L2Transaction> =
            transactions.into_iter().map(|t| t.into()).collect();
        let packed_transactions = packed::L2TransactionVec::new_builder()
            .set(transaction_vec)
            .build();
        let withdrawal_requests_vec: Vec<packed::WithdrawalRequest> =
            withdrawal_requests.into_iter().map(|w| w.into()).collect();
        let packed_withdrawal_requests = packed::WithdrawalRequestVec::new_builder()
            .set(withdrawal_requests_vec)
            .build();
        packed::L2Block::new_builder()
            .raw(raw.into())
            .kv_state(packed_kv_state)
            .kv_state_proof(kv_state_proof.into_bytes().pack())
            .transactions(packed_transactions)
            .block_proof(block_proof.into_bytes().pack())
            .withdrawal_requests(packed_withdrawal_requests)
            .build()
    }
}

impl From<packed::L2Block> for L2Block {
    fn from(l2_block: packed::L2Block) -> L2Block {
        Self {
            raw: l2_block.raw().into(),
            kv_state: l2_block.kv_state().into_iter().map(|k| k.into()).collect(),
            kv_state_proof: JsonBytes::from_bytes(l2_block.kv_state_proof().unpack()),
            transactions: l2_block
                .transactions()
                .into_iter()
                .map(|t| t.into())
                .collect(),
            block_proof: JsonBytes::from_bytes(l2_block.block_proof().unpack()),
            withdrawal_requests: l2_block
                .withdrawal_requests()
                .into_iter()
                .map(|w| w.into())
                .collect(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct RawL2Block {
    pub number: Uint64,
    pub parent_block_hash: H256,
    pub block_producer_id: Uint32,
    pub stake_cell_owner_lock_hash: H256,
    pub timestamp: Uint64,
    pub prev_account: AccountMerkleState,
    pub post_account: AccountMerkleState,
    pub submit_transactions: SubmitTransactions,
    pub withdrawal_requests_root: H256,
}

impl From<RawL2Block> for packed::RawL2Block {
    fn from(json: RawL2Block) -> packed::RawL2Block {
        let RawL2Block {
            number,
            parent_block_hash,
            block_producer_id,
            stake_cell_owner_lock_hash,
            timestamp,
            prev_account,
            post_account,
            submit_transactions,
            withdrawal_requests_root,
        } = json;
        packed::RawL2Block::new_builder()
            .number(u64::from(number).pack())
            .parent_block_hash(parent_block_hash.pack())
            .block_producer_id(u32::from(block_producer_id).pack())
            .stake_cell_owner_lock_hash(stake_cell_owner_lock_hash.pack())
            .timestamp(u64::from(timestamp).pack())
            .prev_account(prev_account.into())
            .post_account(post_account.into())
            .submit_transactions(submit_transactions.into())
            .withdrawal_requests_root(withdrawal_requests_root.pack())
            .build()
    }
}

impl From<packed::RawL2Block> for RawL2Block {
    fn from(raw_l2_block: packed::RawL2Block) -> RawL2Block {
        let number: u64 = raw_l2_block.number().unpack();
        let block_producer_id: u32 = raw_l2_block.block_producer_id().unpack();
        let timestamp: u64 = raw_l2_block.timestamp().unpack();
        Self {
            number: number.into(),
            parent_block_hash: raw_l2_block.parent_block_hash().unpack(),
            block_producer_id: block_producer_id.into(),
            stake_cell_owner_lock_hash: raw_l2_block.stake_cell_owner_lock_hash().unpack(),
            timestamp: timestamp.into(),
            prev_account: raw_l2_block.prev_account().into(),
            post_account: raw_l2_block.post_account().into(),
            submit_transactions: raw_l2_block.submit_transactions().into(),
            withdrawal_requests_root: raw_l2_block.withdrawal_requests_root().unpack(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct L2BlockView {
    pub raw: RawL2Block,
    pub kv_state: Vec<KVPair>,
    pub kv_state_proof: JsonBytes,
    pub transactions: Vec<L2TransactionView>,
    pub block_proof: JsonBytes,
    pub withdrawal_requests: Vec<WithdrawalRequest>,
    pub hash: H256,
}

impl From<packed::L2Block> for L2BlockView {
    fn from(l2_block: packed::L2Block) -> L2BlockView {
        Self {
            hash: H256::from(l2_block.raw().hash()),
            raw: l2_block.raw().into(),
            kv_state: l2_block.kv_state().into_iter().map(|k| k.into()).collect(),
            kv_state_proof: JsonBytes::from_bytes(l2_block.kv_state_proof().unpack()),
            transactions: l2_block
                .transactions()
                .into_iter()
                .map(|t| t.into())
                .collect(),
            block_proof: JsonBytes::from_bytes(l2_block.block_proof().unpack()),
            withdrawal_requests: l2_block
                .withdrawal_requests()
                .into_iter()
                .map(|w| w.into())
                .collect(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct SubmitTransactions {
    pub tx_witness_root: H256,
    pub tx_count: Uint32,
    // hash(account_root | account_count) before each transaction
    pub compacted_post_root_list: Vec<H256>,
}

impl From<SubmitTransactions> for packed::SubmitTransactions {
    fn from(json: SubmitTransactions) -> packed::SubmitTransactions {
        let SubmitTransactions {
            tx_witness_root,
            tx_count,
            compacted_post_root_list,
        } = json;
        let compacted_post_root_list_vec: Vec<packed::Byte32> = compacted_post_root_list
            .into_iter()
            .map(|c| c.pack())
            .collect();
        packed::SubmitTransactions::new_builder()
            .tx_witness_root(tx_witness_root.pack())
            .tx_count(u32::from(tx_count).pack())
            .compacted_post_root_list(
                packed::Byte32Vec::new_builder()
                    .set(compacted_post_root_list_vec)
                    .build(),
            )
            .build()
    }
}

impl From<packed::SubmitTransactions> for SubmitTransactions {
    fn from(submit_transactions: packed::SubmitTransactions) -> SubmitTransactions {
        let tx_count: u32 = submit_transactions.tx_count().unpack();
        Self {
            tx_witness_root: submit_transactions.tx_witness_root().unpack(),
            tx_count: tx_count.into(),
            compacted_post_root_list: submit_transactions
                .compacted_post_root_list()
                .into_iter()
                .map(|c| c.unpack())
                .collect(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct AccountMerkleState {
    pub merkle_root: H256,
    pub count: Uint32,
}

impl From<AccountMerkleState> for packed::AccountMerkleState {
    fn from(json: AccountMerkleState) -> packed::AccountMerkleState {
        let AccountMerkleState { merkle_root, count } = json;
        packed::AccountMerkleState::new_builder()
            .merkle_root(merkle_root.pack())
            .count(u32::from(count).pack())
            .build()
    }
}

impl From<packed::AccountMerkleState> for AccountMerkleState {
    fn from(account_merkel_state: packed::AccountMerkleState) -> AccountMerkleState {
        let count: u32 = account_merkel_state.count().unpack();
        Self {
            merkle_root: account_merkel_state.merkle_root().unpack(),
            count: count.into(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct BlockMerkleState {
    pub merkle_root: H256,
    pub count: Uint64,
}

impl From<BlockMerkleState> for packed::BlockMerkleState {
    fn from(json: BlockMerkleState) -> packed::BlockMerkleState {
        let count: u64 = json.count.into();
        packed::BlockMerkleState::new_builder()
            .merkle_root(json.merkle_root.pack())
            .count(count.pack())
            .build()
    }
}

impl From<packed::BlockMerkleState> for BlockMerkleState {
    fn from(block_merkle_state: packed::BlockMerkleState) -> BlockMerkleState {
        let count: u64 = block_merkle_state.count().unpack();
        Self {
            merkle_root: block_merkle_state.merkle_root().unpack(),
            count: count.into(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct KVPair {
    pub k: H256,
    pub v: H256,
}

impl From<KVPair> for packed::KVPair {
    fn from(json: KVPair) -> packed::KVPair {
        let KVPair { k, v } = json;
        packed::KVPair::new_builder()
            .k(k.pack())
            .v(v.pack())
            .build()
    }
}

impl From<packed::KVPair> for KVPair {
    fn from(kvpair: packed::KVPair) -> KVPair {
        Self {
            k: kvpair.k().unpack(),
            v: kvpair.v().unpack(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct GlobalState {
    pub account: AccountMerkleState,
    pub block: BlockMerkleState,
    pub reverted_block_root: H256,
    pub last_finalized_block_number: Uint64,
    pub status: Uint32,
}

impl From<GlobalState> for packed::GlobalState {
    fn from(json: GlobalState) -> packed::GlobalState {
        let GlobalState {
            account,
            block,
            reverted_block_root,
            last_finalized_block_number,
            status,
        } = json;
        let last_finalized_block_number: u64 = last_finalized_block_number.into();
        let status: u32 = status.into();
        packed::GlobalState::new_builder()
            .account(account.into())
            .block(block.into())
            .reverted_block_root(reverted_block_root.pack())
            .last_finalized_block_number(last_finalized_block_number.pack())
            .status((status as u8).into())
            .build()
    }
}
impl From<packed::GlobalState> for GlobalState {
    fn from(global_state: packed::GlobalState) -> GlobalState {
        let last_finalized_block_number: u64 = global_state.last_finalized_block_number().unpack();
        let status: u8 = global_state.status().into();
        Self {
            account: global_state.account().into(),
            block: global_state.block().into(),
            reverted_block_root: global_state.reverted_block_root().unpack(),
            last_finalized_block_number: last_finalized_block_number.into(),
            status: (status as u32).into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct DepositionRequest {
    pub script: Script,
    pub sudt_script_hash: H256,
    pub amount: Uint128,
    pub capacity: Uint64,
}

impl From<DepositionRequest> for packed::DepositionRequest {
    fn from(json: DepositionRequest) -> packed::DepositionRequest {
        let DepositionRequest {
            script,
            sudt_script_hash,
            amount,
            capacity,
        } = json;
        packed::DepositionRequest::new_builder()
            .script(script.into())
            .sudt_script_hash(sudt_script_hash.pack())
            .amount(u128::from(amount).pack())
            .capacity(u64::from(capacity).pack())
            .build()
    }
}

impl From<packed::DepositionRequest> for DepositionRequest {
    fn from(deposition_request: packed::DepositionRequest) -> DepositionRequest {
        let amount: u128 = deposition_request.amount().unpack();
        let capacity: u64 = deposition_request.capacity().unpack();
        Self {
            script: deposition_request.script().into(),
            sudt_script_hash: deposition_request.sudt_script_hash().unpack(),
            amount: amount.into(),
            capacity: capacity.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct WithdrawalRequest {
    pub raw: RawWithdrawalRequest,
    pub signature: Byte65,
}

impl From<WithdrawalRequest> for packed::WithdrawalRequest {
    fn from(json: WithdrawalRequest) -> packed::WithdrawalRequest {
        let WithdrawalRequest { raw, signature } = json;
        packed::WithdrawalRequest::new_builder()
            .raw(raw.into())
            .signature(signature.into())
            .build()
    }
}

impl From<packed::WithdrawalRequest> for WithdrawalRequest {
    fn from(withdrawal_request: packed::WithdrawalRequest) -> WithdrawalRequest {
        Self {
            raw: withdrawal_request.raw().into(),
            signature: withdrawal_request.signature().into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct RawWithdrawalRequest {
    pub nonce: Uint32,
    pub capacity: Uint64,
    pub amount: Uint128,
    // buyer can pay sell_amount and sell_capacity to unlock
    pub sell_amount: Uint128,
    pub sell_capacity: Uint64,
    pub sudt_script_hash: H256,
    pub account_script_hash: H256,
    // layer1 lock to withdraw after challenge period
    pub owner_lock_hash: H256,
    // layer1 lock to receive the payment, must exists on the chain
    pub payment_lock_hash: H256,
}

impl From<RawWithdrawalRequest> for packed::RawWithdrawalRequest {
    fn from(json: RawWithdrawalRequest) -> packed::RawWithdrawalRequest {
        let RawWithdrawalRequest {
            nonce,
            capacity,
            amount,
            sell_amount,
            sell_capacity,
            sudt_script_hash,
            account_script_hash,
            owner_lock_hash,
            payment_lock_hash,
        } = json;
        packed::RawWithdrawalRequest::new_builder()
            .nonce(u32::from(nonce).pack())
            .capacity(u64::from(capacity).pack())
            .amount(u128::from(amount).pack())
            .sell_amount(u128::from(sell_amount).pack())
            .sell_capacity(u64::from(sell_capacity).pack())
            .sudt_script_hash(sudt_script_hash.pack())
            .account_script_hash(account_script_hash.pack())
            .owner_lock_hash(owner_lock_hash.pack())
            .payment_lock_hash(payment_lock_hash.pack())
            .build()
    }
}

impl From<packed::RawWithdrawalRequest> for RawWithdrawalRequest {
    fn from(raw_withdrawal_request: packed::RawWithdrawalRequest) -> RawWithdrawalRequest {
        let nonce: u32 = raw_withdrawal_request.nonce().unpack();
        let capacity: u64 = raw_withdrawal_request.capacity().unpack();
        let amount: u128 = raw_withdrawal_request.amount().unpack();
        let sell_capacity: u64 = raw_withdrawal_request.sell_capacity().unpack();
        let sell_amount: u128 = raw_withdrawal_request.sell_amount().unpack();
        Self {
            nonce: nonce.into(),
            capacity: capacity.into(),
            amount: amount.into(),
            sell_capacity: sell_capacity.into(),
            sell_amount: sell_amount.into(),
            sudt_script_hash: raw_withdrawal_request.sudt_script_hash().unpack(),
            account_script_hash: raw_withdrawal_request.account_script_hash().unpack(),
            owner_lock_hash: raw_withdrawal_request.owner_lock_hash().unpack(),
            payment_lock_hash: raw_withdrawal_request.payment_lock_hash().unpack(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct HeaderInfo {
    pub number: Uint64,
    pub block_hash: H256,
}

impl From<HeaderInfo> for packed::HeaderInfo {
    fn from(json: HeaderInfo) -> packed::HeaderInfo {
        let HeaderInfo { number, block_hash } = json;
        packed::HeaderInfo::new_builder()
            .number(u64::from(number).pack())
            .block_hash(block_hash.pack())
            .build()
    }
}
