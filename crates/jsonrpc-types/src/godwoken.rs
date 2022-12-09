use crate::blockchain::Script;
use anyhow::{anyhow, Error as JsonError};
use ckb_fixed_hash::{H160, H256};
use ckb_jsonrpc_types::{JsonBytes, Uint128, Uint32, Uint64};
use gw_types::core::Timepoint;
use gw_types::{bytes::Bytes, offchain, packed, prelude::*};
use serde::{Deserialize, Serialize};
use std::convert::{TryFrom, TryInto};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct RawL2Transaction {
    pub chain_id: Uint64, // chain id
    pub from_id: Uint32,
    pub to_id: Uint32,
    pub nonce: Uint32,
    pub args: JsonBytes,
}

impl From<RawL2Transaction> for packed::RawL2Transaction {
    fn from(tx: RawL2Transaction) -> Self {
        let RawL2Transaction {
            chain_id,
            from_id,
            to_id,
            nonce,
            args,
        } = tx;
        let args: Bytes = args.into_bytes();
        packed::RawL2Transaction::new_builder()
            .chain_id(u64::from(chain_id).pack())
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
        let chain_id: u64 = raw_l2_transaction.chain_id().unpack();
        Self {
            from_id: from_id.into(),
            to_id: to_id.into(),
            nonce: nonce.into(),
            chain_id: chain_id.into(),
            args: JsonBytes::from_bytes(raw_l2_transaction.args().unpack()),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct L2Transaction {
    pub raw: RawL2Transaction,
    pub signature: JsonBytes,
}

impl From<L2Transaction> for packed::L2Transaction {
    fn from(tx: L2Transaction) -> Self {
        let L2Transaction { raw, signature } = tx;

        packed::L2Transaction::new_builder()
            .raw(raw.into())
            .signature(signature.into_bytes().pack())
            .build()
    }
}

impl From<packed::L2Transaction> for L2Transaction {
    fn from(l2_transaction: packed::L2Transaction) -> L2Transaction {
        Self {
            raw: l2_transaction.raw().into(),
            signature: JsonBytes::from_bytes(l2_transaction.signature().unpack()),
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
    // The actual type is `u8`
    pub service_flag: Uint32,
    pub data: JsonBytes,
}

impl From<LogItem> for packed::LogItem {
    fn from(json: LogItem) -> packed::LogItem {
        let LogItem {
            account_id,
            service_flag,
            data,
        } = json;
        packed::LogItem::new_builder()
            .account_id(account_id.value().pack())
            .service_flag((service_flag.value() as u8).into())
            .data(data.into_bytes().pack())
            .build()
    }
}

impl From<packed::LogItem> for LogItem {
    fn from(data: packed::LogItem) -> LogItem {
        let account_id: u32 = data.account_id().unpack();
        let service_flag: u8 = data.service_flag().into();
        let data = JsonBytes::from_bytes(data.data().unpack());
        LogItem {
            account_id: Uint32::from(account_id),
            service_flag: Uint32::from(service_flag as u32),
            data,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct TxReceipt {
    pub tx_witness_hash: H256,
    pub post_state: AccountMerkleState,
    pub read_data_hashes: Vec<H256>,
    pub logs: Vec<LogItem>,
    pub exit_code: Uint32,
}

impl From<TxReceipt> for packed::TxReceipt {
    fn from(json: TxReceipt) -> packed::TxReceipt {
        let TxReceipt {
            tx_witness_hash,
            post_state,
            read_data_hashes,
            logs,
            exit_code,
        } = json;
        let tx_witness_hash: [u8; 32] = tx_witness_hash.into();
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
            .post_state(post_state.into())
            .read_data_hashes(read_data_hashes.pack())
            .logs(logs.pack())
            .exit_code((exit_code.value() as u8).into())
            .build()
    }
}

impl From<packed::TxReceipt> for TxReceipt {
    fn from(data: packed::TxReceipt) -> TxReceipt {
        let tx_witness_hash: [u8; 32] = data.tx_witness_hash().unpack();
        let post_state: AccountMerkleState = data.post_state().into();
        let read_data_hashes: Vec<_> = data
            .read_data_hashes()
            .into_iter()
            .map(|hash| {
                let hash: [u8; 32] = hash.unpack();
                hash.into()
            })
            .collect();
        let logs: Vec<LogItem> = data.logs().into_iter().map(|item| item.into()).collect();
        let exit_code: u8 = data.exit_code().into();
        TxReceipt {
            tx_witness_hash: tx_witness_hash.into(),
            post_state,
            read_data_hashes,
            logs,
            exit_code: (exit_code as u32).into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeTargetType {
    TxExecution,
    TxSignature,
    Withdrawal,
}

impl Default for ChallengeTargetType {
    fn default() -> Self {
        Self::TxExecution
    }
}

impl From<ChallengeTargetType> for packed::Byte {
    fn from(json: ChallengeTargetType) -> packed::Byte {
        match json {
            ChallengeTargetType::TxExecution => packed::Byte::new(0),
            ChallengeTargetType::TxSignature => packed::Byte::new(1),
            ChallengeTargetType::Withdrawal => packed::Byte::new(2),
        }
    }
}

impl From<gw_types::core::ChallengeTargetType> for ChallengeTargetType {
    fn from(core: gw_types::core::ChallengeTargetType) -> ChallengeTargetType {
        match core {
            gw_types::core::ChallengeTargetType::Withdrawal => ChallengeTargetType::Withdrawal,
            gw_types::core::ChallengeTargetType::TxSignature => ChallengeTargetType::TxSignature,
            gw_types::core::ChallengeTargetType::TxExecution => ChallengeTargetType::TxExecution,
        }
    }
}

impl TryFrom<packed::Byte> for ChallengeTargetType {
    type Error = JsonError;

    fn try_from(v: packed::Byte) -> Result<ChallengeTargetType, Self::Error> {
        match u8::from(v) {
            0 => Ok(ChallengeTargetType::TxExecution),
            1 => Ok(ChallengeTargetType::TxSignature),
            2 => Ok(ChallengeTargetType::Withdrawal),
            _ => Err(anyhow!("Invalid challenge target type {}", v)),
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
        let target_type: packed::Byte = challenge_target.target_type();
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
pub struct L2Block {
    pub raw: RawL2Block,
    pub kv_state: Vec<KVPair>,
    pub kv_state_proof: JsonBytes,
    pub transactions: Vec<L2Transaction>,
    pub block_proof: JsonBytes,
    pub withdrawals: Vec<WithdrawalRequest>,
}

impl From<L2Block> for packed::L2Block {
    fn from(json: L2Block) -> packed::L2Block {
        let L2Block {
            raw,
            kv_state,
            kv_state_proof,
            transactions,
            block_proof,
            withdrawals,
        } = json;
        let kv_pair_vec: Vec<packed::KVPair> = kv_state.into_iter().map(|k| k.into()).collect();
        let packed_kv_state = packed::KVPairVec::new_builder().set(kv_pair_vec).build();
        let transaction_vec: Vec<packed::L2Transaction> =
            transactions.into_iter().map(|t| t.into()).collect();
        let packed_transactions = packed::L2TransactionVec::new_builder()
            .set(transaction_vec)
            .build();
        let withdrawal_requests_vec: Vec<packed::WithdrawalRequest> =
            withdrawals.into_iter().map(|w| w.into()).collect();
        let packed_withdrawal_requests = packed::WithdrawalRequestVec::new_builder()
            .set(withdrawal_requests_vec)
            .build();
        packed::L2Block::new_builder()
            .raw(raw.into())
            .kv_state(packed_kv_state)
            .kv_state_proof(kv_state_proof.into_bytes().pack())
            .transactions(packed_transactions)
            .block_proof(block_proof.into_bytes().pack())
            .withdrawals(packed_withdrawal_requests)
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
            withdrawals: l2_block
                .withdrawals()
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
    pub block_producer: JsonBytes,
    pub stake_cell_owner_lock_hash: H256,
    pub timestamp: Uint64,
    pub prev_account: AccountMerkleState,
    pub post_account: AccountMerkleState,
    pub submit_transactions: SubmitTransactions,
    pub submit_withdrawals: SubmitWithdrawals,
    // hash(account_root | account_count) of each withdrawals & transactions
    pub state_checkpoint_list: Vec<H256>,
}

impl From<RawL2Block> for packed::RawL2Block {
    fn from(json: RawL2Block) -> packed::RawL2Block {
        let RawL2Block {
            number,
            parent_block_hash,
            block_producer,
            stake_cell_owner_lock_hash,
            timestamp,
            prev_account,
            post_account,
            submit_transactions,
            submit_withdrawals,
            state_checkpoint_list,
        } = json;

        let state_checkpoint_list = state_checkpoint_list
            .into_iter()
            .map(|checkpoint| checkpoint.pack())
            .pack();
        packed::RawL2Block::new_builder()
            .number(u64::from(number).pack())
            .parent_block_hash(parent_block_hash.pack())
            .block_producer(block_producer.as_bytes().pack())
            .stake_cell_owner_lock_hash(stake_cell_owner_lock_hash.pack())
            .timestamp(u64::from(timestamp).pack())
            .prev_account(prev_account.into())
            .post_account(post_account.into())
            .submit_transactions(submit_transactions.into())
            .submit_withdrawals(submit_withdrawals.into())
            .state_checkpoint_list(state_checkpoint_list)
            .build()
    }
}

impl From<packed::RawL2Block> for RawL2Block {
    fn from(raw_l2_block: packed::RawL2Block) -> RawL2Block {
        let number: u64 = raw_l2_block.number().unpack();
        let block_producer = JsonBytes::from_vec(raw_l2_block.block_producer().unpack());
        let timestamp: u64 = raw_l2_block.timestamp().unpack();
        let state_checkpoint_list = raw_l2_block
            .state_checkpoint_list()
            .into_iter()
            .map(|checkpoint| checkpoint.unpack())
            .collect();
        Self {
            number: number.into(),
            parent_block_hash: raw_l2_block.parent_block_hash().unpack(),
            block_producer,
            stake_cell_owner_lock_hash: raw_l2_block.stake_cell_owner_lock_hash().unpack(),
            timestamp: timestamp.into(),
            prev_account: raw_l2_block.prev_account().into(),
            post_account: raw_l2_block.post_account().into(),
            submit_transactions: raw_l2_block.submit_transactions().into(),
            submit_withdrawals: raw_l2_block.submit_withdrawals().into(),
            state_checkpoint_list,
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
                .withdrawals()
                .into_iter()
                .map(|w| w.into())
                .collect(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum L2BlockStatus {
    Unfinalized,
    Finalized,
    Reverted,
}

impl Default for L2BlockStatus {
    fn default() -> Self {
        Self::Unfinalized
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct L2BlockWithStatus {
    pub block: L2BlockView,
    pub status: L2BlockStatus,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum L2TransactionStatus {
    Pending,
    Committed,
}

impl Default for L2TransactionStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct L2TransactionWithStatus {
    pub transaction: Option<L2TransactionView>,
    pub status: L2TransactionStatus,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WithdrawalStatus {
    Pending,
    Committed,
}

impl Default for WithdrawalStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct WithdrawalWithStatus {
    pub withdrawal: Option<WithdrawalRequestExtra>,
    pub status: WithdrawalStatus,
    pub l1_committed_info: Option<L2BlockCommittedInfo>,
    pub l2_committed_info: Option<L2WithdrawalCommittedInfo>,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct L2WithdrawalCommittedInfo {
    pub block_number: Uint64,
    pub block_hash: H256,
    pub withdrawal_index: Uint32,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct SubmitTransactions {
    pub tx_witness_root: H256,
    pub tx_count: Uint32,
    // hash(account_root | account_count) before apply all transactions
    pub prev_state_checkpoint: H256,
}

impl From<SubmitTransactions> for packed::SubmitTransactions {
    fn from(json: SubmitTransactions) -> packed::SubmitTransactions {
        let SubmitTransactions {
            tx_witness_root,
            tx_count,
            prev_state_checkpoint,
        } = json;
        packed::SubmitTransactions::new_builder()
            .tx_witness_root(tx_witness_root.pack())
            .tx_count(u32::from(tx_count).pack())
            .prev_state_checkpoint(prev_state_checkpoint.pack())
            .build()
    }
}

impl From<packed::SubmitTransactions> for SubmitTransactions {
    fn from(submit_transactions: packed::SubmitTransactions) -> SubmitTransactions {
        let tx_count: u32 = submit_transactions.tx_count().unpack();
        Self {
            tx_witness_root: submit_transactions.tx_witness_root().unpack(),
            tx_count: tx_count.into(),
            prev_state_checkpoint: submit_transactions.prev_state_checkpoint().unpack(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct SubmitWithdrawals {
    pub withdrawal_witness_root: H256,
    pub withdrawal_count: Uint32,
}

impl From<SubmitWithdrawals> for packed::SubmitWithdrawals {
    fn from(json: SubmitWithdrawals) -> packed::SubmitWithdrawals {
        let SubmitWithdrawals {
            withdrawal_witness_root,
            withdrawal_count,
        } = json;
        packed::SubmitWithdrawals::new_builder()
            .withdrawal_witness_root(withdrawal_witness_root.pack())
            .withdrawal_count(u32::from(withdrawal_count).pack())
            .build()
    }
}

impl From<packed::SubmitWithdrawals> for SubmitWithdrawals {
    fn from(data: packed::SubmitWithdrawals) -> SubmitWithdrawals {
        let withdrawal_count: u32 = data.withdrawal_count().unpack();
        Self {
            withdrawal_witness_root: data.withdrawal_witness_root().unpack(),
            withdrawal_count: withdrawal_count.into(),
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

    // As Godwoken switches from v1 to v2 by bumping GlobalState.version from 1 to 2, the last
    // finalized timepoint representation by the finalized block number will be replaced by the
    // finalized timestamp.

    // Before switching to v2, `last_finalized_block_number` should be `Some(block_number)` and
    // `last_finalized_timestamp` should be `None`; afterwards, `last_finalized_block_number` will
    // become `None` and `last_finalized_timestamp` will become `Some(timestamp)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_finalized_block_number: Option<Uint64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_finalized_timestamp: Option<Uint64>,

    pub status: Uint32,
}

impl TryFrom<GlobalState> for packed::GlobalState {
    type Error = JsonError;

    fn try_from(json: GlobalState) -> Result<packed::GlobalState, Self::Error> {
        let GlobalState {
            account,
            block,
            reverted_block_root,
            last_finalized_block_number,
            last_finalized_timestamp,
            status,
        } = json;
        let last_finalized_timepoint = match (last_finalized_block_number, last_finalized_timestamp)
        {
            (Some(block_number), None) => Timepoint::from_block_number(block_number.value()),
            (None, Some(timestamp)) => Timepoint::from_timestamp(timestamp.value()),
            (bn, ts) => {
                return Err(anyhow!(
                    "conflict last_finalized_block_number {:?} and last_finalized_timestamp {:?}",
                    bn,
                    ts
                ));
            }
        };
        let status: u32 = status.into();
        Ok(packed::GlobalState::new_builder()
            .account(account.into())
            .block(block.into())
            .reverted_block_root(reverted_block_root.pack())
            .last_finalized_timepoint(last_finalized_timepoint.full_value().pack())
            .status((status as u8).into())
            .build())
    }
}
impl From<packed::GlobalState> for GlobalState {
    fn from(global_state: packed::GlobalState) -> GlobalState {
        let last_finalized_timepoint =
            Timepoint::from_full_value(global_state.last_finalized_timepoint().unpack());
        let (last_finalized_block_number, last_finalized_timestamp) = match last_finalized_timepoint
        {
            Timepoint::BlockNumber(block_number) => (Some(block_number), None),
            Timepoint::Timestamp(timestamp) => (None, Some(timestamp)),
        };
        let status: u8 = global_state.status().into();
        Self {
            account: global_state.account().into(),
            block: global_state.block().into(),
            reverted_block_root: global_state.reverted_block_root().unpack(),
            last_finalized_block_number: last_finalized_block_number.map(Into::into),
            last_finalized_timestamp: last_finalized_timestamp.map(Into::into),
            status: (status as u32).into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct DepositRequest {
    pub script: Script,
    pub sudt_script_hash: H256,
    pub amount: Uint128,
    pub capacity: Uint64,
}

impl From<DepositRequest> for packed::DepositRequest {
    fn from(json: DepositRequest) -> packed::DepositRequest {
        let DepositRequest {
            script,
            sudt_script_hash,
            amount,
            capacity,
        } = json;
        packed::DepositRequest::new_builder()
            .script(script.into())
            .sudt_script_hash(sudt_script_hash.pack())
            .amount(u128::from(amount).pack())
            .capacity(u64::from(capacity).pack())
            .build()
    }
}

impl From<packed::DepositRequest> for DepositRequest {
    fn from(deposit_request: packed::DepositRequest) -> DepositRequest {
        let amount: u128 = deposit_request.amount().unpack();
        let capacity: u64 = deposit_request.capacity().unpack();
        Self {
            script: deposit_request.script().into(),
            sudt_script_hash: deposit_request.sudt_script_hash().unpack(),
            amount: amount.into(),
            capacity: capacity.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct WithdrawalRequestExtra {
    pub request: WithdrawalRequest,
    pub owner_lock: Script,
}

impl From<WithdrawalRequestExtra> for packed::WithdrawalRequestExtra {
    fn from(json: WithdrawalRequestExtra) -> packed::WithdrawalRequestExtra {
        let WithdrawalRequestExtra {
            request,
            owner_lock,
        } = json;
        packed::WithdrawalRequestExtra::new_builder()
            .request(request.into())
            .owner_lock(owner_lock.into())
            .build()
    }
}

impl From<packed::WithdrawalRequestExtra> for WithdrawalRequestExtra {
    fn from(withdrawal: packed::WithdrawalRequestExtra) -> WithdrawalRequestExtra {
        Self {
            request: withdrawal.request().into(),
            owner_lock: withdrawal.owner_lock().into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct WithdrawalRequest {
    pub raw: RawWithdrawalRequest,
    pub signature: JsonBytes,
}

impl From<WithdrawalRequest> for packed::WithdrawalRequest {
    fn from(json: WithdrawalRequest) -> packed::WithdrawalRequest {
        let WithdrawalRequest { raw, signature } = json;
        packed::WithdrawalRequest::new_builder()
            .raw(raw.into())
            .signature(signature.into_bytes().pack())
            .build()
    }
}

impl From<packed::WithdrawalRequest> for WithdrawalRequest {
    fn from(withdrawal_request: packed::WithdrawalRequest) -> WithdrawalRequest {
        Self {
            raw: withdrawal_request.raw().into(),
            signature: JsonBytes::from_bytes(withdrawal_request.signature().unpack()),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct RawWithdrawalRequest {
    pub nonce: Uint32,
    pub capacity: Uint64,
    pub amount: Uint128,
    pub sudt_script_hash: H256,
    pub account_script_hash: H256,
    pub registry_id: Uint32,
    // layer1 lock to withdraw after challenge period
    pub owner_lock_hash: H256,
    pub chain_id: Uint64,
    pub fee: Uint128,
}

impl From<RawWithdrawalRequest> for packed::RawWithdrawalRequest {
    fn from(json: RawWithdrawalRequest) -> packed::RawWithdrawalRequest {
        let RawWithdrawalRequest {
            nonce,
            capacity,
            amount,
            sudt_script_hash,
            account_script_hash,
            registry_id,
            owner_lock_hash,
            fee,
            chain_id,
        } = json;
        packed::RawWithdrawalRequest::new_builder()
            .nonce(u32::from(nonce).pack())
            .capacity(u64::from(capacity).pack())
            .amount(u128::from(amount).pack())
            .sudt_script_hash(sudt_script_hash.pack())
            .account_script_hash(account_script_hash.pack())
            .registry_id(registry_id.value().pack())
            .owner_lock_hash(owner_lock_hash.pack())
            .chain_id(chain_id.value().pack())
            .fee(u128::from(fee).pack())
            .build()
    }
}

impl From<packed::RawWithdrawalRequest> for RawWithdrawalRequest {
    fn from(raw_withdrawal_request: packed::RawWithdrawalRequest) -> RawWithdrawalRequest {
        let nonce: u32 = raw_withdrawal_request.nonce().unpack();
        let capacity: u64 = raw_withdrawal_request.capacity().unpack();
        let amount: u128 = raw_withdrawal_request.amount().unpack();
        let fee: u128 = raw_withdrawal_request.fee().unpack();
        let chain_id: u64 = raw_withdrawal_request.chain_id().unpack();
        let registry_id: u32 = raw_withdrawal_request.registry_id().unpack();
        Self {
            nonce: nonce.into(),
            capacity: capacity.into(),
            amount: amount.into(),
            sudt_script_hash: raw_withdrawal_request.sudt_script_hash().unpack(),
            account_script_hash: raw_withdrawal_request.account_script_hash().unpack(),
            registry_id: registry_id.into(),
            owner_lock_hash: raw_withdrawal_request.owner_lock_hash().unpack(),
            fee: fee.into(),
            chain_id: chain_id.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct LastL2BlockCommittedInfo {
    pub transaction_hash: H256,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct L2BlockCommittedInfo {
    pub number: Uint64,
    pub block_hash: H256,
    pub transaction_hash: H256,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AllowedEoaType {
    Unknown,
    Eth,
    Tron,
}

impl From<AllowedEoaType> for packed::Byte {
    fn from(json: AllowedEoaType) -> packed::Byte {
        match json {
            AllowedEoaType::Unknown => packed::Byte::new(0),
            AllowedEoaType::Eth => packed::Byte::new(1),
            AllowedEoaType::Tron => packed::Byte::new(2),
        }
    }
}

impl TryFrom<packed::Byte> for AllowedEoaType {
    type Error = JsonError;

    fn try_from(v: packed::Byte) -> Result<Self, Self::Error> {
        match u8::from(v) {
            0 => Ok(AllowedEoaType::Unknown),
            1 => Ok(AllowedEoaType::Eth),
            2 => Ok(AllowedEoaType::Tron),
            _ => Err(anyhow!("invalid allowed eoa type {}", v)),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AllowedContractType {
    Unknown,
    Meta,
    Sudt,
    Polyjuice,
    EthAddrReg,
}

impl From<AllowedContractType> for packed::Byte {
    fn from(json: AllowedContractType) -> packed::Byte {
        match json {
            AllowedContractType::Unknown => packed::Byte::new(0),
            AllowedContractType::Meta => packed::Byte::new(1),
            AllowedContractType::Sudt => packed::Byte::new(2),
            AllowedContractType::Polyjuice => packed::Byte::new(3),
            AllowedContractType::EthAddrReg => packed::Byte::new(4),
        }
    }
}

impl TryFrom<packed::Byte> for AllowedContractType {
    type Error = JsonError;

    fn try_from(v: packed::Byte) -> Result<Self, Self::Error> {
        match u8::from(v) {
            0 => Ok(AllowedContractType::Unknown),
            1 => Ok(AllowedContractType::Meta),
            2 => Ok(AllowedContractType::Sudt),
            3 => Ok(AllowedContractType::Polyjuice),
            4 => Ok(AllowedContractType::EthAddrReg),
            _ => Err(anyhow!("invalid allowed contract type {}", v)),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct AllowedEoaTypeHash {
    pub type_: AllowedEoaType,
    pub hash: H256,
}

impl From<AllowedEoaTypeHash> for packed::AllowedTypeHash {
    fn from(type_hash: AllowedEoaTypeHash) -> Self {
        packed::AllowedTypeHash::new_builder()
            .type_(type_hash.type_.into())
            .hash(type_hash.hash.pack())
            .build()
    }
}

impl From<packed::AllowedTypeHash> for AllowedEoaTypeHash {
    fn from(type_hash: packed::AllowedTypeHash) -> Self {
        let maybe_type_ = type_hash.type_().try_into();

        Self {
            type_: maybe_type_.unwrap_or(AllowedEoaType::Unknown),
            hash: type_hash.hash().unpack(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct AllowedContractTypeHash {
    pub type_: AllowedContractType,
    pub hash: H256,
}

impl From<AllowedContractTypeHash> for packed::AllowedTypeHash {
    fn from(type_hash: AllowedContractTypeHash) -> Self {
        packed::AllowedTypeHash::new_builder()
            .type_(type_hash.type_.into())
            .hash(type_hash.hash.pack())
            .build()
    }
}

impl From<packed::AllowedTypeHash> for AllowedContractTypeHash {
    fn from(type_hash: packed::AllowedTypeHash) -> Self {
        let maybe_type_ = type_hash.type_().try_into();

        Self {
            type_: maybe_type_.unwrap_or(AllowedContractType::Unknown),
            hash: type_hash.hash().unpack(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct RollupConfig {
    pub l1_sudt_script_type_hash: H256,
    pub custodian_script_type_hash: H256,
    pub deposit_script_type_hash: H256,
    pub withdrawal_script_type_hash: H256,
    pub challenge_script_type_hash: H256,
    pub stake_script_type_hash: H256,
    pub l2_sudt_validator_script_type_hash: H256,
    pub burn_lock_hash: H256,
    pub required_staking_capacity: Uint64,
    pub challenge_maturity_blocks: Uint64,
    pub finality_blocks: Uint64,
    pub reward_burn_rate: Uint32, // * reward_burn_rate / 100
    pub chain_id: Uint64,         // chain id
    pub allowed_eoa_type_hashes: Vec<AllowedEoaTypeHash>, // list of script code_hash allowed an EOA(external owned account) to use
    pub allowed_contract_type_hashes: Vec<AllowedContractTypeHash>, // list of script code_hash allowed a contract account to use
}

impl From<RollupConfig> for packed::RollupConfig {
    fn from(json: RollupConfig) -> packed::RollupConfig {
        let RollupConfig {
            l1_sudt_script_type_hash,
            custodian_script_type_hash,
            deposit_script_type_hash,
            withdrawal_script_type_hash,
            challenge_script_type_hash,
            stake_script_type_hash,
            l2_sudt_validator_script_type_hash,
            burn_lock_hash,
            required_staking_capacity,
            challenge_maturity_blocks,
            finality_blocks,
            reward_burn_rate, // * reward_burn_rate / 100
            chain_id,
            allowed_eoa_type_hashes, // list of script code_hash allowed an EOA(external owned account) to use
            allowed_contract_type_hashes, // list of script code_hash allowed a contract account to use
        } = json;
        let required_staking_capacity: u64 = required_staking_capacity.into();
        let challenge_maturity_blocks: u64 = challenge_maturity_blocks.into();
        let finality_blocks: u64 = finality_blocks.into();
        let reward_burn_rate: u32 = reward_burn_rate.into();
        let chain_id: u64 = chain_id.into();
        let reward_burn_rate: u8 = reward_burn_rate.try_into().expect("reward burn rate");
        packed::RollupConfig::new_builder()
            .l1_sudt_script_type_hash(l1_sudt_script_type_hash.pack())
            .custodian_script_type_hash(custodian_script_type_hash.pack())
            .deposit_script_type_hash(deposit_script_type_hash.pack())
            .withdrawal_script_type_hash(withdrawal_script_type_hash.pack())
            .challenge_script_type_hash(challenge_script_type_hash.pack())
            .stake_script_type_hash(stake_script_type_hash.pack())
            .l2_sudt_validator_script_type_hash(l2_sudt_validator_script_type_hash.pack())
            .burn_lock_hash(burn_lock_hash.pack())
            .required_staking_capacity(required_staking_capacity.pack())
            .challenge_maturity_blocks(challenge_maturity_blocks.pack())
            .finality_blocks(finality_blocks.pack())
            .reward_burn_rate(reward_burn_rate.into())
            .chain_id(chain_id.pack())
            .allowed_eoa_type_hashes(allowed_eoa_type_hashes.into_iter().map(From::from).pack())
            .allowed_contract_type_hashes(
                allowed_contract_type_hashes
                    .into_iter()
                    .map(From::from)
                    .pack(),
            )
            .build()
    }
}

impl From<packed::RollupConfig> for RollupConfig {
    fn from(data: packed::RollupConfig) -> RollupConfig {
        let required_staking_capacity: u64 = data.required_staking_capacity().unpack();
        let challenge_maturity_blocks: u64 = data.challenge_maturity_blocks().unpack();
        let finality_blocks: u64 = data.finality_blocks().unpack();
        let reward_burn_date: u8 = data.reward_burn_rate().into();
        let chain_id: u64 = data.chain_id().unpack();
        RollupConfig {
            l1_sudt_script_type_hash: data.l1_sudt_script_type_hash().unpack(),
            custodian_script_type_hash: data.custodian_script_type_hash().unpack(),
            deposit_script_type_hash: data.deposit_script_type_hash().unpack(),
            withdrawal_script_type_hash: data.withdrawal_script_type_hash().unpack(),
            challenge_script_type_hash: data.challenge_script_type_hash().unpack(),
            stake_script_type_hash: data.stake_script_type_hash().unpack(),
            l2_sudt_validator_script_type_hash: data.l2_sudt_validator_script_type_hash().unpack(),
            burn_lock_hash: data.burn_lock_hash().unpack(),
            required_staking_capacity: required_staking_capacity.into(),
            challenge_maturity_blocks: challenge_maturity_blocks.into(),
            finality_blocks: finality_blocks.into(),
            reward_burn_rate: (reward_burn_date as u32).into(),
            chain_id: chain_id.into(),
            allowed_eoa_type_hashes: data
                .allowed_eoa_type_hashes()
                .into_iter()
                .map(From::from)
                .collect(),
            allowed_contract_type_hashes: data
                .allowed_contract_type_hashes()
                .into_iter()
                .map(From::from)
                .collect(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct RunResult {
    // return data
    pub return_data: JsonBytes,
    // log data
    pub logs: Vec<LogItem>,
}

impl From<offchain::RunResult> for RunResult {
    fn from(data: offchain::RunResult) -> RunResult {
        let offchain::RunResult {
            return_data, logs, ..
        } = data;
        RunResult {
            return_data: JsonBytes::from_bytes(return_data),
            logs: logs.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct NodeInfo {
    pub mode: NodeMode,
    pub version: String,
    pub backends: Vec<BackendInfo>,
    pub eoa_scripts: Vec<EoaScript>,
    pub gw_scripts: Vec<GwScript>,
    pub rollup_cell: RollupCell,
    pub rollup_config: NodeRollupConfig,
    // Web3 as of 1.9.0 cannot handle null values in node info. So we omit this
    // field instead of saying it's null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gasless_tx_support: Option<GaslessTxSupportConfig>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "lowercase")]
pub enum NodeMode {
    FullNode,
    Test,
    ReadOnly,
}

impl Default for NodeMode {
    fn default() -> Self {
        NodeMode::ReadOnly
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct BackendInfo {
    pub validator_code_hash: H256,
    pub generator_code_hash: H256,
    pub validator_script_type_hash: H256,
    pub backend_type: BackendType,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum BackendType {
    Unknown,
    Meta,
    Sudt,
    Polyjuice,
    EthAddrReg,
}

impl Default for BackendType {
    fn default() -> Self {
        BackendType::Unknown
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct GwScript {
    pub type_hash: H256,
    pub script: Script,
    pub script_type: GwScriptType,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum GwScriptType {
    Unknown,
    Deposit,
    Withdraw,
    StateValidator,
    StakeLock,
    CustodianLock,
    ChallengeLock,
    L1Sudt,
    L2Sudt,
    OmniLock,
}

impl Default for GwScriptType {
    fn default() -> Self {
        GwScriptType::Unknown
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct RollupCell {
    pub type_hash: H256,
    pub type_script: Script,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct NodeRollupConfig {
    pub required_staking_capacity: Uint64,
    pub challenge_maturity_blocks: Uint64,
    pub finality_blocks: Uint64,
    pub reward_burn_rate: Uint32,
    pub chain_id: Uint64,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct EoaScript {
    pub type_hash: H256,
    pub script: Script,
    pub eoa_type: EoaScriptType,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum EoaScriptType {
    Unknown,
    Eth,
}

impl Default for EoaScriptType {
    fn default() -> Self {
        EoaScriptType::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
pub struct GaslessTxSupportConfig {
    /// Gasless tx entrypoint address.
    pub entrypoint_address: H160,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct ErrorTxReceipt {
    pub tx_hash: H256,
    pub block_number: Uint64,
    pub return_data: JsonBytes,
    pub last_log: Option<LogItem>,
    // i8 -> u32, actual u8
    pub exit_code: Uint32,
}

impl From<offchain::ErrorTxReceipt> for ErrorTxReceipt {
    fn from(receipt: offchain::ErrorTxReceipt) -> Self {
        let exit_code = receipt.exit_code as u8;
        ErrorTxReceipt {
            tx_hash: H256::from(Into::<[u8; 32]>::into(receipt.tx_hash)),
            block_number: receipt.block_number.into(),
            return_data: JsonBytes::from_bytes(receipt.return_data),
            last_log: receipt.last_log.map(Into::into),
            exit_code: (exit_code as u32).into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct SUDTFeeConfig {
    pub sudt_id: Uint32,
    pub fee_rate_weight: Uint64,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct FeeConfig {
    pub meta_cycles_limit: Uint64,
    pub sudt_cycles_limit: Uint64,
    pub withdraw_cycles_limit: Uint64,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct WithdrawalLockArgs {
    pub account_script_hash: H256,
    pub withdrawal_block_hash: H256,

    // As Godwoken switches from v1 to v2 by bumping GlobalState.version from 1 to 2, the withdrawn
    // timepoint representation by the withdrawn block number will be replaced by the withdrawn
    // block timestamp.

    // Before switching to v2, `withdrawal_block_number` should be `Some(block_number)` and
    // `withdrawal_finalized_timestamp` should be `None`; afterwards, `withdrawal_block_number` will
    // become `None` and `withdrawal_finalized_timestamp` will become `Some(block_timestamp)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdrawal_block_number: Option<Uint64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdrawal_finalized_timestamp: Option<Uint64>,

    // layer1 lock to withdraw after challenge period
    pub owner_lock_hash: H256,
}

impl TryFrom<WithdrawalLockArgs> for packed::WithdrawalLockArgs {
    type Error = JsonError;

    fn try_from(json: WithdrawalLockArgs) -> Result<packed::WithdrawalLockArgs, Self::Error> {
        let WithdrawalLockArgs {
            account_script_hash,
            withdrawal_block_hash,
            withdrawal_block_number,
            withdrawal_finalized_timestamp,
            owner_lock_hash,
        } = json;
        let finalized_timepoint = match (withdrawal_block_number, withdrawal_finalized_timestamp) {
            (Some(block_number), None) => Timepoint::from_block_number(block_number.value()),
            (None, Some(timestamp)) => Timepoint::from_timestamp(timestamp.value()),
            (bn, bt) => {
                return Err(anyhow!(
                    "conflict withdrawal_block_number {:?} and withdrawal_finalized_timestamp {:?}",
                    bn,
                    bt
                ));
            }
        };

        Ok(packed::WithdrawalLockArgs::new_builder()
            .account_script_hash(account_script_hash.pack())
            .withdrawal_block_hash(withdrawal_block_hash.pack())
            .finalized_timepoint(finalized_timepoint.full_value().pack())
            .owner_lock_hash(owner_lock_hash.pack())
            .build())
    }
}

impl From<packed::WithdrawalLockArgs> for WithdrawalLockArgs {
    fn from(data: packed::WithdrawalLockArgs) -> WithdrawalLockArgs {
        let finalized_timepoint = Timepoint::from_full_value(data.finalized_timepoint().unpack());
        let (withdrawal_block_number, withdrawal_finalized_timestamp) = match finalized_timepoint {
            Timepoint::BlockNumber(block_number) => (Some(block_number), None),
            Timepoint::Timestamp(timestamp) => (None, Some(timestamp)),
        };
        Self {
            account_script_hash: data.account_script_hash().unpack(),
            owner_lock_hash: data.owner_lock_hash().unpack(),
            withdrawal_block_hash: data.withdrawal_block_hash().unpack(),
            withdrawal_block_number: withdrawal_block_number.map(Into::into),
            withdrawal_finalized_timestamp: withdrawal_finalized_timestamp.map(Into::into),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct RegistryAddress {
    pub registry_id: Uint32,
    pub address: JsonBytes,
}

impl From<RegistryAddress> for gw_common::registry_address::RegistryAddress {
    fn from(json: RegistryAddress) -> gw_common::registry_address::RegistryAddress {
        let RegistryAddress {
            registry_id,
            address,
        } = json;
        gw_common::registry_address::RegistryAddress::new(
            registry_id.value(),
            address.as_bytes().to_vec(),
        )
    }
}

impl From<gw_common::registry_address::RegistryAddress> for RegistryAddress {
    fn from(data: gw_common::registry_address::RegistryAddress) -> RegistryAddress {
        let gw_common::registry_address::RegistryAddress {
            registry_id,
            address,
        } = data;
        RegistryAddress {
            registry_id: registry_id.into(),
            address: JsonBytes::from_vec(address),
        }
    }
}
