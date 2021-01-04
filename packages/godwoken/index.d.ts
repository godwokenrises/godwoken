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
  aggregator_id: HexNumber;
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
  aggregator_id: HexNumber;
  deposition_requests: HexString[]; // gw_types::packed::DepositionRequest[]
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
  return_data: HexString;
  account_count?: HexNumber;
  new_scripts: Record<Hash, HexString>;
  new_data: Record<Hash, HexString>;
}

export interface GenesisWithGlobalState {
  genesis: HexString; // gw_types::packed::L2Block
  global_state: HexString; // gw_types::packed::GlobalState
}

export function buildGenesisBlock(
  config: GenesisConfig
): Promise<GenesisWithGlobalState>;

export class ChainService {
  constructor(config: Config, headerInfo: HexString);
  sync(syncParam: SyncParam): Promise<SyncEvent>;
  produceBlock(
    produceBlockParam: ProduceBlockParam
  ): Promise<ProduceBlockResult>;
  submitL2Transaction(l2Transaction: HexString): Promise<RunResult>;
  submitWithdrawalRequest(withdrawalRequest: HexString): Promise<void>;
  execute(l2Transaction: HexString): Promise<RunResult>;
  getBalance(accountId: number, sudtId: number): Promise<HexNumber>;
  getStorageAt(accountId: number, rawKey: Hash): Promise<Hash>;
  getAccountIdByScriptHash(hash: Hash): Promise<number | undefined>;
  getNonce(accountId: number): Promise<number>;
  getScriptHash(accountId: number): Promise<Hash>;
  getScript(scriptHash: Hash): Promise<Script | undefined>;
  getDataHash(dataHash: Hash): Promise<boolean>;
  getData(dataHash: Hash): Promise<HexString | undefined>;
  tip(): HexString; // gw_bytes::packed::L2Block
  lastSynced(): HexString; // gw_bytes::packed::HeaderInfo
  status(): Status;
  config(): Config;
}
