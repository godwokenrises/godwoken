import { HexNumber, Hash } from "@ckb-lumos/base";
export interface SyncParam {
  reverts: L1Action[];
  updates: L1Action[];
  next_block_context: NextBlockContext;
}

export interface L1Action {
  transaction: ArrayBuffer; // ckb_types::packed::Transaction
  header_info: ArrayBuffer; // gw_types::packed::HeaderInfo
  context: SubmitTxs | StartChallenge | CancelChallenge | Revert;
}

export interface NextBlockContext {
  aggregator_id: HexNumber;
  timestamp: HexNumber;
}

export interface SubmitTxs {
  type: "submit_txs";
  deposition_requests: ArrayBuffer[]; // Vec<gw_types::packed::DepositionRequest>
}

export interface StartChallenge {
  type: "start_challenge";
  context: ArrayBuffer; // gw_types::packed::StartChallenge
}

export interface CancelChallenge {
  type: "cancel_challenge";
  context: ArrayBuffer; // gw_types::packed::CancelChallenge
}
export interface Revert {
  type: "revert";
  context: ArrayBuffer; // gw_types::packed::StartChallenge
}

export interface ProduceBlockParam {
  aggregator_id: HexNumber;
  deposition_requests: ArrayBuffer[]; // gw_types::packed::DepositionRequest[]
}

export interface ProduceBlockResult {
  block: ArrayBuffer; // gw_types::packed::L2Block
  global_state: ArrayBuffer; // gw_types::packed::GlobalState
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

export interface Config {
  chain: ChainConfig;
  consensus: ConsensusConfig;
  rpc: RPC;
  genesis: GenesisConfig;
  aggregator?: AggregatorConfig;
}

export interface ChainConfig {
  rollup_type_script: ArrayBuffer; // ckb_types::packed::Script
}

export interface ConsensusConfig {
  aggregator_id: HexNumber;
}

export interface RPC {
  listen: string;
}

export interface GenesisConfig {
  initial_aggregator_script: ArrayBuffer; // ckb_types::packed::Script
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

export interface BranchNode {
  fork_height: HexNumber,
  key: Hash,
  node: Hash,
  sibling: Hash,
}

export interface LeafNode {
  key: Hash,
  value: Hash,
}

export interface BranchMapEntry {
  key: Hash,
  value: BranchNode,
}

export interface LeafMapEntry {
  key: Hash,
  value: LeafNode,
}

export interface GenesisWithSMTState {
  genesis: ArrayBuffer; // gw_types::packed::L2Block
  branches_map: BranchMapEntry[],
  leaves_map: LeafMapEntry[],
}

export function buildGenesisBlock(config: GenesisConfig): Promise<GenesisWithSMTState>;

export class ChainService {
  constructor(config: Config);
  sync(syncParam: SyncParam): Promise<SyncEvent>;
  produce_block(
    produceBlockParam: ProduceBlockParam
  ): Promise<ProduceBlockResult>;
  submitL2Transaction(l2Transaction: ArrayBuffer): Promise<RunResult>;
  execute(l2Transaction: ArrayBuffer): Promise<RunResult>;
  //getStorageAt()
  tip(): ArrayBuffer; // gw_bytes::packed::L2Block
  lastSynced(): ArrayBuffer; // gw_bytes::packed::HeaderInfo
  status(): Status;
  config(): Config;
}
