import { Hash, HexNumber, HexString, Script } from "@ckb-lumos/base";

export type U32 = number;
export type U64 = bigint;
export type U128 = bigint;

export type HexU32 = HexNumber;
export type HexU64 = HexNumber;
export type HexU128 = HexNumber;
export type HexU256 = HexNumber;

// null means `pending`
export type BlockParameter = U64 | null;

export interface LogItem {
  account_id: HexU32;
  // The actual type is `u8`
  service_flag: HexU32;
  data: HexString;
}

export interface RunResult {
  return_data: HexString;
  logs: LogItem[];
}

/**
 * @see {@link https://github.com/nervosnetwork/godwoken/blob/c4be58f30744aef83717e2a12d60fe4d50b165ab/crates/jsonrpc-types/src/godwoken.rs#L1310-L1317}
 */
export interface ErrorTxReceipt {
  tx_hash: Hash;
  block_number: HexU64;
  return_data: HexString;
  last_log?: LogItem;
  exit_code: HexU32;
}

export function isErrorTxReceipt(obj: any): obj is ErrorTxReceipt {
  return (
    "tx_hash" in obj &&
    "block_number" in obj &&
    "return_data" in obj &&
    "exit_code" in obj
  );
}

export interface RawL2Transaction {
  chain_id: HexU64;
  from_id: HexU32;
  to_id: HexU32;
  nonce: HexU32;
  args: HexString;
}

export interface L2Transaction {
  raw: RawL2Transaction;
  signature: HexString;
}

export interface L2TransactionWithStatus {
  transaction: L2Transaction;
  tx_status: {
    status: "committed" | "pending";
    block_hash?: Hash;
  };
}

export interface L2TransactionReceipt {
  tx_witness_hash: Hash;
  post_state: AccountMerkleState;
  read_data_hashes: Hash[];
  logs: LogItem[];
  exit_code: HexNumber;
}

export interface AccountMerkleState {
  merkle_root: Hash;
  count: HexU32;
}

export enum NodeMode {
  FullNode = "fullnode",
  ReadOnly = "readonly",
  Test = "test",
}

export enum EoaScriptType {
  Eth = "eth",
}

export interface EoaScript {
  type_hash: HexString;
  script: Script;
  eoa_type: EoaScriptType;
}

export enum BackendType {
  Unknown = "unknown",
  Meta = "meta",
  Sudt = "sudt",
  Polyjuice = "polyjuice",
  EthAddrReg = "eth_addr_reg",
}
export interface BackendInfo {
  validator_code_hash: HexString;
  generator_code_hash: HexString;
  validator_script_type_hash: HexString;
  backend_type: BackendType;
}

export enum GwScriptType {
  Deposit = "deposit",
  Withdraw = "withdraw",
  StateValidator = "state_validator",
  StakeLock = "stake_lock",
  CustodianLock = "custodian_lock",
  ChallengeLock = "challenge_lock",
  L1Sudt = "l1_sudt",
  L2Sudt = "l2_sudt",
  OmniLock = "omni_lock",
}
export interface GwScript {
  type_hash: HexString;
  script: Script;
  script_type: GwScriptType;
}

export interface RollupCell {
  type_hash: HexString;
  type_script: Script;
}

export interface RollupConfig {
  required_staking_capacity: HexNumber;
  challenge_maturity_blocks: HexNumber;
  finality_blocks: HexNumber;
  reward_burn_rate: HexNumber;
  chain_id: HexNumber;
}
export interface NodeInfo {
  backends: Array<BackendInfo>;
  eoa_scripts: Array<EoaScript>;
  gw_scripts: Array<GwScript>;
  rollup_cell: RollupCell;
  rollup_config: RollupConfig;
  version: string;
  mode: NodeMode;
}
export interface RegistryAddress {
  registry_id: HexU32;
  address: HexString;
}

export enum SudtArgsType {
  SUDTQuery = "SUDTQuery",
  SUDTTransfer = "SUDTTransfer",
}

export interface SudtQuery {
  address: HexString;
}

export interface SudtTransfer {
  to_address: HexString;
  amount: HexU256;
  fee: Fee;
}

export interface SudtArgs {
  type: SudtArgsType;
  value: SudtQuery | SudtTransfer;
}

export enum EthAddrRegArgsType {
  EthToGw = "EthToGw",
  GwToEth = "GwToEth",
  SetMapping = "SetMapping",
  BatchSetMapping = "BatchSetMapping",
}
export interface EthAddrRegArgs {
  type: EthAddrRegArgsType;
  value: SetMapping | BatchSetMapping | EthToGw | GwToEth;
}

export interface BatchSetMapping {
  gw_script_hashes: Hash[];
  fee: Fee;
}

export interface SetMapping {
  gw_script_hash: Hash;
  fee: Fee;
}

export interface EthToGw {
  eth_address: HexString;
}

export interface GwToEth {
  gw_script_hash: HexString;
}

export interface Fee {
  registry_id: HexU32;
  amount: HexU128;
}

export enum MetaContractArgsType {
  CreateAccount = "CreateAccount",
  BatchCreateEthAccounts = "BatchCreateEthAccounts",
}
export interface MetaContractArgs {
  type: MetaContractArgsType;
  value: CreateAccount | BatchCreateEthAccounts;
}

export interface CreateAccount {
  script: Script;
  fee: Fee;
}

export interface BatchCreateEthAccounts {
  scripts: Script[];
  fee: Fee;
}
