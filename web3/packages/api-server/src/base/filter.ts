import { Hash, HexString } from "@ckb-lumos/base";
import { BlockParameter } from "../methods/types";

export interface RpcFilterRequest {
  fromBlock?: BlockParameter;
  toBlock?: BlockParameter;
  address?: HexString;
  topics?: FilterTopic[];
  blockHash?: HexString;
}

export enum FilterFlag {
  blockFilter = 1,
  pendingTransaction = 2,
}

export type FilterTopic = null | HexString | HexString[];

export interface FilterParams {
  fromBlock: bigint;
  toBlock: bigint;
  addresses: HexString[];
  topics: FilterTopic[];
  blockHash?: Hash;
}
