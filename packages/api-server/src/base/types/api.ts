import { Hash, HexNumber, HexString } from "@ckb-lumos/base";

export interface EthTransaction {
  hash: Hash;
  // when pending, blockNumber & blockHash = null
  blockHash: Hash | null;
  blockNumber: HexNumber | null;
  transactionIndex: HexNumber | null;
  from: HexString;
  to: HexString | null;
  gas: HexNumber;
  gasPrice: HexNumber;
  input: HexString;
  nonce: HexNumber;
  value: HexNumber;
  v: HexNumber;
  r: HexNumber;
  s: HexNumber;
}

export interface EthBlock {
  // when pending, number & hash & nonce & logsBloom = pending
  number: HexNumber | null;
  hash: Hash;
  parentHash: Hash;
  gasLimit: HexNumber;
  gasUsed: HexNumber;
  miner: HexString;
  size: HexNumber;
  logsBloom: HexString;
  transactions: (EthTransaction | Hash)[];
  timestamp: HexNumber;
  mixHash: Hash;
  nonce: HexNumber;
  stateRoot: Hash;
  sha3Uncles: Hash;
  receiptsRoot: Hash;
  transactionsRoot: Hash;
  uncles: [];
  difficulty: HexNumber;
  totalDifficulty: HexNumber;
  extraData: HexString;
}

export interface FailedReason {
  status_code: HexNumber;
  status_type: string;
  message: string;
}

export interface EthTransactionReceipt {
  transactionHash: Hash;
  transactionIndex: HexNumber;
  blockHash: Hash;
  blockNumber: HexNumber;
  from: HexString;
  to: HexString | null;
  gasUsed: HexNumber;
  cumulativeGasUsed: HexNumber;
  logsBloom: HexString;
  logs: EthLog[];
  contractAddress: HexString | null;
  status: HexNumber; // 0 => failed, 1 => success
  failed_reason?: FailedReason; // null if success
}

export interface EthLog {
  // when pending logIndex, transactionIndex, transactionHash, blockHash, blockNumber = null
  address: HexString;
  blockHash: Hash | null;
  blockNumber: HexNumber | null;
  transactionIndex: HexNumber | null;
  transactionHash: Hash | null;
  data: HexString;
  logIndex: HexNumber | null;
  topics: HexString[];
  removed: boolean;
}

export interface EthNewHead {
  number: HexNumber;
  hash: Hash;
  parentHash: Hash;
  gasLimit: HexNumber;
  gasUsed: HexNumber;
  miner: HexString;
  logsBloom: HexString;
  timestamp: HexNumber;
  mixHash: Hash;
  nonce: HexNumber;
  stateRoot: Hash;
  sha3Uncles: Hash;
  receiptsRoot: Hash;
  transactionsRoot: Hash;
  difficulty: HexNumber;
  extraData: HexString;
  baseFeePerGas: HexNumber;
}
