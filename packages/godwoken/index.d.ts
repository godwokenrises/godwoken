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
  timestamp: HexNumber;
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
  type: "submit_txs";
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
  type: "start_challenge";
  block_hash: Hash;
  block_number: HexNumber;
  tx_index: Hash;
}

export interface CancelChallenge {
  type: "cancel_challenge";
  l1block: L2Block;
  block_proof: ArrayBuffer;
  kv_state: KVPair[];
  kv_state_proof: ArrayBuffer;
}
export interface Revert {
  type: "revert";
  block_hash: Hash;
  block_number: HexNumber;
  tx_index: Hash;
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

export type SyncEvent =
  | SuccessEvent
  | BadBlockEvent
  | BadChallengeEvent
  | WaitChallengeEvent;

export interface SuccessEvent {
  type: "success";
}
export interface BadBlockEvent {
  type: "bad_block";
}
export interface BadChallengeEvent {
  type: "bad_challenge";
}
export interface WaitChallengeEvent {
  type: "wait_challenge";
}

export type Status = "Running" | "Halting";

export interface L2Transaction {
  raw: RawL2Transaction;
  signature: ArrayBuffer;
}

export interface RawL2Transaction {
  from_id: HexNumber;
  to_id: HexNumber;
  nonce: HexNumber;
  args: ArrayBuffer;
}

export interface Config {
  chain: ChainConfig;
  consensus: ConsensusConfig;
  rpc: RPC;
  genesis: GenesisConfig;
  aggregator?: AggregatorConfig;
}

export interface ChainConfig {
  rollup_type_script: Script;
}

export interface ConsensusConfig {
  aggregator_id: HexNumber;
}

export interface RPC {
  listen: string;
}

export interface GenesisConfig {
  initial_aggregator_pubkey_hash: Hash;
  initial_deposition: HexNumber;
  timestamp: HexNumber;
}

export interface AggregatorConfig {
  account_id: HexNumber;
  signer: SignerConfig;
}

export interface SignerConfig {}

export interface RunResult {
  read_values: Record<Hash, Hash>;
  write_values: Record<Hash, Hash>;
  return_data: ArrayBuffer;
  account_count?: HexNumber;
  new_scripts: Record<Hash, ArrayBuffer>;
  new_data: Record<Hash, ArrayBuffer>;
}

export class ChainService {
  constructor(config: Config);
  sync(syncParam: SyncParam): Promise<SyncEvent>;
  produce_block(
    produceBlockParam: ProduceBlockParam
  ): Promise<L2BlockWithState>;
  submitL2Transaction(l2Transaction: L2Transaction): Promise<RunResult>;
  execute(l2Transaction: L2Transaction): Promise<RunResult>;
  tip(): L2Block;
  lastSynced(): HeaderInfo;
  status(): Status;
}
