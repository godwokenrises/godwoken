import { Hash, HexNumber, HexString } from "@ckb-lumos/base";
export type Error = {
  code?: number;
  message: string;
} | null;

export type SyningStatus =
  | false
  | {
      startingBlock: number;
      currentBlock: number;
      highestBlock: number;
    };

export type Response = number | string | boolean | SyningStatus | Array<string>;

export type Callback = (err: Error, res?: any | Response) => void;

export type BlockTag = "latest" | "earliest" | "pending";

// Eip1898 support block hash and block number
export interface BlockSpecifier {
  blockNumber?: HexNumber;
  blockHash?: Hash;
  requireCanonical?: boolean;
}

export type BlockParameter = HexNumber | BlockTag | BlockSpecifier;

export interface TransactionCallObject {
  from?: HexString;
  to: HexString;
  gas?: HexNumber;
  gasPrice?: HexNumber;
  value?: HexNumber;
  data?: HexNumber;
}
export interface LogItem {
  account_id: HexNumber;
  service_flag: HexNumber;
  data: HexString;
}
export interface SudtOperationLog {
  sudtId: number;
  fromId: number;
  toId: number;
  amount: bigint;
}

export interface SudtPayFeeLog {
  sudtId: number;
  fromId: number;
  blockProducerId: number;
  amount: bigint;
}

export interface PolyjuiceSystemLog {
  gasUsed: bigint;
  cumulativeGasUsed: bigint;
  createdAddress: string;
  statusCode: number;
}

export interface PolyjuiceUserLog {
  address: HexString;
  data: HexString;
  topics: HexString[];
}

export type GodwokenLog =
  | SudtOperationLog
  | SudtPayFeeLog
  | PolyjuiceSystemLog
  | PolyjuiceUserLog;
