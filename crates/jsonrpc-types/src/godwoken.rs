use crate::blockchain::Script;
use crate::fixed_bytes::Byte65;
use ckb_jsonrpc_types::{JsonBytes, Uint128, Uint32, Uint64};
use gw_types::bytes::Bytes;
use gw_types::{packed, prelude::*, H256};
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

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct StartChallenge {
    pub block_hash: H256,     // hash of challenged block
    pub block_number: Uint64, // number of challenged block
    pub tx_index: Uint32,     // challenge tx
}

impl From<StartChallenge> for packed::StartChallenge {
    fn from(json: StartChallenge) -> packed::StartChallenge {
        let StartChallenge {
            block_hash,
            block_number,
            tx_index,
        } = json;
        packed::StartChallenge::new_builder()
            .block_hash(block_hash.pack())
            .block_number(u64::from(block_number).pack())
            .tx_index(u32::from(tx_index).pack())
            .build()
    }
}

impl From<packed::StartChallenge> for StartChallenge {
    fn from(start_challenge: packed::StartChallenge) -> StartChallenge {
        let block_number: u64 = start_challenge.block_number().unpack();
        let tx_index: u32 = start_challenge.tx_index().unpack();
        Self {
            block_hash: start_challenge.block_hash().unpack(),
            block_number: Uint64::from(block_number),
            tx_index: Uint32::from(tx_index),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct CancelChallenge {
    pub l2block: L2Block,
    pub block_proof: JsonBytes,
    pub kv_state: Vec<KVPair>,
    pub kv_state_proof: JsonBytes,
}

impl From<CancelChallenge> for packed::CancelChallenge {
    fn from(json: CancelChallenge) -> packed::CancelChallenge {
        let CancelChallenge {
            l2block,
            block_proof,
            kv_state,
            kv_state_proof,
        } = json;
        let kv_pair_vec: Vec<packed::KVPair> = kv_state.into_iter().map(|k| k.into()).collect();
        let packed_kv_state_vec = packed::KVPairVec::new_builder().set(kv_pair_vec).build();

        packed::CancelChallenge::new_builder()
            .l2block(l2block.into())
            .block_proof(block_proof.into_bytes().pack())
            .kv_state(packed_kv_state_vec)
            .kv_state_proof(kv_state_proof.into_bytes().pack())
            .build()
    }
}

impl From<packed::CancelChallenge> for CancelChallenge {
    fn from(cancel_challenge: packed::CancelChallenge) -> CancelChallenge {
        Self {
            l2block: cancel_challenge.l2block().into(),
            block_proof: JsonBytes::from_bytes(cancel_challenge.block_proof().unpack()),
            kv_state: cancel_challenge
                .kv_state()
                .into_iter()
                .map(|k| k.into())
                .collect(),
            kv_state_proof: JsonBytes::from_bytes(cancel_challenge.kv_state_proof().unpack()),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct L2Block {
    pub raw: RawL2Block,
    pub signature: Byte65,
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
            signature,
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
            .signature(signature.into())
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
            signature: l2_block.signature().into(),
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
    pub aggregator_id: Uint32,
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
            aggregator_id,
            stake_cell_owner_lock_hash,
            timestamp,
            prev_account,
            post_account,
            submit_transactions,
            withdrawal_requests_root,
        } = json;
        packed::RawL2Block::new_builder()
            .number(u64::from(number).pack())
            .aggregator_id(u32::from(aggregator_id).pack())
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
        let aggregator_id: u32 = raw_l2_block.aggregator_id().unpack();
        let timestamp: u64 = raw_l2_block.timestamp().unpack();
        Self {
            number: number.into(),
            aggregator_id: aggregator_id.into(),
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
    pub status: StatusUnion,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
pub enum StatusUnion {
    Running {},
    Reverting {
        next_block_number: Uint64,
        challenger_id: Uint32,
    },
}

impl Default for StatusUnion {
    fn default() -> Self {
        StatusUnion::Running {}
    }
}
impl From<StatusUnion> for packed::StatusUnion {
    fn from(json: StatusUnion) -> packed::StatusUnion {
        match json {
            StatusUnion::Running {} => {
                packed::StatusUnion::Running(packed::Running::new_builder().build())
            }
            StatusUnion::Reverting {
                next_block_number,
                challenger_id,
            } => {
                let reverting = packed::Reverting::new_builder()
                    .next_block_number(u64::from(next_block_number).pack())
                    .challenger_id(u32::from(challenger_id).pack())
                    .build();
                packed::StatusUnion::Reverting(reverting)
            }
        }
    }
}

impl From<packed::StatusUnion> for StatusUnion {
    fn from(status_union: packed::StatusUnion) -> StatusUnion {
        match status_union {
            packed::StatusUnion::Running(_running) => StatusUnion::Running {},
            packed::StatusUnion::Reverting(reverting) => {
                let next_block_number: u64 = reverting.next_block_number().unpack();
                let challenger_id: u32 = reverting.challenger_id().unpack();
                StatusUnion::Reverting {
                    next_block_number: next_block_number.into(),
                    challenger_id: challenger_id.into(),
                }
            }
        }
    }
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
        let status: packed::Status = packed::Status::new_builder().set(status).build();
        packed::GlobalState::new_builder()
            .account(account.into())
            .block(block.into())
            .reverted_block_root(reverted_block_root.pack())
            .last_finalized_block_number(last_finalized_block_number.pack())
            .status(status)
            .build()
    }
}
impl From<packed::GlobalState> for GlobalState {
    fn from(global_state: packed::GlobalState) -> GlobalState {
        let last_finalized_block_number: u64 = global_state.last_finalized_block_number().unpack();
        Self {
            account: global_state.account().into(),
            block: global_state.block().into(),
            reverted_block_root: global_state.reverted_block_root().unpack(),
            last_finalized_block_number: last_finalized_block_number.into(),
            status: global_state.status().to_enum().into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct DepositionRequest {
    pub script: Script,
    pub sudt_script: Script,
    pub amount: Uint128,
    pub capacity: Uint64,
}

impl From<DepositionRequest> for packed::DepositionRequest {
    fn from(json: DepositionRequest) -> packed::DepositionRequest {
        let DepositionRequest {
            script,
            sudt_script,
            amount,
            capacity,
        } = json;
        packed::DepositionRequest::new_builder()
            .script(script.into())
            .sudt_script(sudt_script.into())
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
            sudt_script: deposition_request.sudt_script().into(),
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
    // layer1 cell to receive the withdraw
    pub lock_hash: H256,
    pub sudt_script_hash: H256,
    pub account_script_hash: H256,
}

impl From<RawWithdrawalRequest> for packed::RawWithdrawalRequest {
    fn from(json: RawWithdrawalRequest) -> packed::RawWithdrawalRequest {
        let RawWithdrawalRequest {
            nonce,
            capacity,
            amount,
            lock_hash,
            sudt_script_hash,
            account_script_hash,
        } = json;
        packed::RawWithdrawalRequest::new_builder()
            .nonce(u32::from(nonce).pack())
            .capacity(u64::from(capacity).pack())
            .amount(u128::from(amount).pack())
            .lock_hash(lock_hash.pack())
            .sudt_script_hash(sudt_script_hash.pack())
            .account_script_hash(account_script_hash.pack())
            .build()
    }
}

impl From<packed::RawWithdrawalRequest> for RawWithdrawalRequest {
    fn from(raw_withdrawal_request: packed::RawWithdrawalRequest) -> RawWithdrawalRequest {
        let nonce: u32 = raw_withdrawal_request.nonce().unpack();
        let capacity: u64 = raw_withdrawal_request.capacity().unpack();
        let amount: u128 = raw_withdrawal_request.amount().unpack();
        Self {
            nonce: nonce.into(),
            capacity: capacity.into(),
            amount: amount.into(),
            lock_hash: raw_withdrawal_request.lock_hash().unpack(),
            sudt_script_hash: raw_withdrawal_request.sudt_script_hash().unpack(),
            account_script_hash: raw_withdrawal_request.account_script_hash().unpack(),
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
