import { HexNumber, HexString, Hash, Script } from "@ckb-lumos/base";
export interface SyncParam {
  reverts: L1Action[];
  updates: L1Action[];
  next_block_context: NextBlockContext;
}

export interface L1Action {
  transaction: HexString; // gw_types::packed::Transaction
  header_info: HexString; // gw_types::packed::HeaderInfo
  context: SubmitTxs | StartChallenge | CancelChallenge | Revert;
}

export interface NextBlockContext {
  block_producer_id: HexNumber;
  timestamp: HexNumber;
}

export interface SubmitTxs {
  type: "submit_txs";
  deposition_requests: HexString[]; // Vec<gw_types::packed::DepositionRequest>
}

export interface StartChallenge {
  type: "start_challenge";
  context: HexString; // gw_types::packed::StartChallenge
}

export interface CancelChallenge {
  type: "cancel_challenge";
  context: HexString; // gw_types::packed::CancelChallenge
}
export interface Revert {
  type: "revert";
  context: HexString; // gw_types::packed::StartChallenge
}

export interface ProduceBlockParam {
  block_producer_id: HexNumber;
}

export interface PackageParam {
  deposition_requests: HexString[]; // gw_types::packed::DepositionRequest[]
  max_withdrawal_capacity: HexNumber;
}

export interface ProduceBlockResult {
  block: HexString; // gw_types::packed::L2Block
  global_state: HexString; // gw_types::packed::GlobalState
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
  context: HexString; // gw_types::packed::StartChallenge
}
export interface BadChallengeEvent {
  type: "bad_challenge";
  context: HexString; // gw_types::packed::CancelChallenge
}
export interface WaitChallengeEvent {
  type: "wait_challenge";
}

export type Status = "Running" | "Halting";

export interface Config {
  chain: ChainConfig;
  store: StoreConfig;
  genesis: GenesisConfig;
  rollup: RollupConfig;
  block_producer?: BlockProducerConfig;
}

export interface ChainConfig {
  rollup_type_script: Script;
}

export interface StoreConfig {
  path: string;
}

export interface GenesisConfig {
  timestamp: HexNumber;
}

export interface RollupConfig {
  l1_sudt_script_type_hash: Hash;
  custodian_script_type_hash: Hash;
  deposition_script_type_hash: Hash;
  withdrawal_script_type_hash: Hash;
  challenge_script_type_hash: Hash;
  stake_script_type_hash: Hash;
  l2_sudt_validator_script_type_hash: Hash;
  burn_lock_hash: Hash;
  required_staking_capacity: HexNumber;
  challenge_maturity_blocks: HexNumber;
  finality_blocks: HexNumber;
  reward_burn_rate: HexNumber; // * reward_burn_rate / 100
}

export interface BlockProducerConfig {
  account_id: HexNumber;
}

export interface LogItem {
  account_id: HexNumber;
  data: HexString;
}

export interface RunResult {
  read_values: Record<Hash, Hash>;
  write_values: Record<Hash, Hash>;
  return_data: HexString;
  account_count?: HexNumber;
  new_scripts: Record<Hash, HexString>;
  new_data: Record<Hash, HexString>;
  logs: LogItem[];
}

export interface GenesisWithGlobalState {
  genesis: HexString; // gw_types::packed::L2Block
  global_state: HexString; // gw_types::packed::GlobalState
}

export interface RawWithdrawalRequest {
  nonce: HexNumber;
  // CKB amount
  capacity: HexNumber;
  // SUDT amount
  amount: HexNumber;
  sudt_script_hash: Hash;
  // layer2 account_script_hash
  account_script_hash: Hash;
  // buyer can pay sell_amount and sell_capacity to unlock
  sell_amount: HexNumber;
  sell_capacity: HexNumber;
  // layer1 lock to withdraw after challenge period
  owner_lock_hash: Hash;
  // layer1 lock to receive the payment, must exists on the chain
  payment_lock_hash: Hash;
}
export interface WithdrawalRequest {
  raw: RawWithdrawalRequest;
  signature: HexString;
}
export interface KVPair {
  k: Hash;
  v: Hash;
}
export interface RawL2Transaction {
  from_id: HexNumber;
  to_id: HexNumber;
  nonce: HexNumber;
  args: HexString;
}
export interface L2Transaction {
  raw: RawL2Transaction;
  signature: HexString;
}
export interface L2TransactionView {
  hash: Hash;
  raw: RawL2Transaction;
  signature: HexString;
}
export interface SubmitTransactions {
  tx_witness_root: HexString;
  tx_count: HexNumber;
  // hash(account_root | account_count) before each transaction
  compacted_post_root_list: Hash[];
}
export interface AccountMerkleState {
  merkle_root: Hash;
  count: HexNumber;
}
export interface RawL2Block {
  number: HexNumber;
  parent_block_hash: Hash;
  block_producer_id: HexNumber;
  stake_cell_owner_lock_hash: Hash;
  timestamp: HexNumber;
  prev_account: AccountMerkleState;
  post_account: AccountMerkleState;
  submit_transactions: SubmitTransactions;
  withdrawal_requests_root: Hash;
}

export interface L2Block {
  raw: RawL2Block;
  signature: HexString;
  kv_state: KVPair[];
  kv_state_proof: HexString;
  transactions: L2Transaction[];
  block_proof: HexString;
  withdrawal_requests: WithdrawalRequest[];
}
export interface L2BlockView {
  hash: Hash;
  raw: RawL2Block;
  signature: HexString;
  kv_state: KVPair[];
  kv_state_proof: HexString;
  transactions: L2TransactionView[];
  block_proof: HexString;
  withdrawal_requests: WithdrawalRequest[];
}

export function buildGenesisBlock(
  config: GenesisConfig,
  rollup_config: RollupConfig
): Promise<GenesisWithGlobalState>;

export class ChainService {
  constructor(config: Config, headerInfo: HexString);
  sync(syncParam: SyncParam): Promise<SyncEvent>;
  produceBlock(
    produceBlockParam: ProduceBlockParam,
    packageParam: PackageParam
  ): Promise<ProduceBlockResult>;
  submitL2Transaction(l2Transaction: HexString): Promise<RunResult>;
  submitWithdrawalRequest(withdrawalRequest: HexString): Promise<void>;
  execute(l2Transaction: HexString): Promise<RunResult>;
  getTipBlockNumber(): Promise<HexNumber>;
  getBlockHashByNumber(block_number: number): Promise<Hash>;
  getBlockByNumber(block_number: number): Promise<L2BlockView>;
  getBlock(block_hash: Hash): Promise<L2BlockView>;
  getTransaction(tx_hash: Hash): Promise<L2TransactionView>;
  getBalance(accountId: number, sudtId: number): Promise<HexNumber>;
  getStorageAt(accountId: number, rawKey: Hash): Promise<Hash>;
  getAccountIdByScriptHash(hash: Hash): Promise<number | undefined>;
  getNonce(accountId: number): Promise<number>;
  getScriptHash(accountId: number): Promise<Hash>;
  getScript(scriptHash: Hash): Promise<Script | undefined>;
  getDataHash(dataHash: Hash): Promise<boolean>;
  getData(dataHash: Hash): Promise<HexString | undefined>;
  tip(): L2BlockView; // gw_bytes::packed::L2Block
  lastSynced(): HexString; // gw_bytes::packed::HeaderInfo
  status(): Status;
  config(): Config;
}
