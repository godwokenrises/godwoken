import { HexNumber, Hash, Transaction, Script } from "@ckb-lumos/base";
export interface SyncParam {
  reverts: L1Action[];
  updates: L1Action[];
  next_block_context: NextBlockContext;
}

export interface L1Action {
  transaction_info: TransactionInfo;
  header_info: HeaderInfo;
  context: SubmitTxs | StartChallenge | CancelChallenge | Revert;
}

export interface NextBlockContext {
  aggregator_id: HexNumber;
  timestammp: HexNumber;
}

export interface TransactionInfo {
  transaction: Transaction;
  block_hash: Hash;
}

export interface HeaderInfo {
  number: HexNumber;
  block_hash: Hash;
}

export interface SubmitTxs {
  deposition_requests: DepositionRequest[];
  withdrawal_requests: WithdrawalRequest[];
}

export interface DepositionRequest {
  script: Script;
  sudt_script: Script;
  amount: HexNumber;
}

export interface WithdrawalRequest {
  lock_hash: Hash;
  sudt_script_hash: Hash;
  amount: HexNumber;
  account_script_hash: Hash;
}

export interface StartChallenge {
  block_hash: Hash;
  block_number: HexNumber;
  tx_index: Hash;
}

export interface CancelChallenge {
  l1block: L2Block;
  block_proof: ArrayBuffer;
  kv_state: KVPair[];
  kv_state_proof: ArrayBuffer;
}

export interface L2Block {
  number: HexNumber;
  aggregator_id: HexNumber;
  stake_cell_owner_lock_hash: ArrayBuffer;
  timestamp: HexNumber;
  prev_account: AccountMerkleState;
  post_account: AccountMerkleState;
  submit_transactions?: SubmitTransactions;
}

export interface KVPair {
  k: ArrayBuffer;
  v: ArrayBuffer;
}

export interface AccountMerkleState {
  merkle_root: ArrayBuffer;
  count: HexNumber;
}

export interface SubmitTransactions {
  tx_witness_root: ArrayBuffer;
  tx_count: HexNumber;
  // hash(account_root | account_count) before each transaction
  compacted_post_root_list: ArrayBuffer[];
}

export interface ProduceBlockParam {
  aggregator_id: HexNumber;
  deposition_requests: DepositionRequest[];
  withdrawal_requests: WithdrawalRequest[];
}

export interface L2BlockWithState {
  block: L2Block;
  global_state: GlobalState;
}

export interface GlobalState {
  account: AccountMerkleState;
  block: BlockMerkleState;
  status: Status;
}

export interface BlockMerkleState {
  merkle_root: ArrayBuffer;
  count: HexNumber;
}

export type Revert = StartChallenge;

export type SyncEvent =
  | "Success"
  | "BadBlock"
  | "BadChallenge"
  | "WaitChallenge";

export type Status = "Running" | "Halting";

export class ChainService {
  sync(syncParam: SyncParam): SyncEvent;
  produce_block(produceBlockParam: ProduceBlockParam): L2BlockWithState;
  tip(): L2Block;
  lastSynced(): HeaderInfo;
  status(): Status;
}
