import { HexString } from "@ckb-lumos/base";
import { FilterFlag, RpcFilterRequest } from "../base/filter";

export interface FilterCacheInDb {
  filter: FilterFlag | RpcFilterRequest;
  lastPoll: HexString;
  // the filter's last poll record:
  //          - for eth_newBlockFilter, the last poll record is the block number (bigint)
  //          - for eth_newPendingTransactionFilter, the last poll record is the pending transaction id (bigint) (currently not support)
  //          - for normal filter, the last poll record is log_id of log (bigint)
}

export interface FilterCache {
  filter: FilterFlag | RpcFilterRequest;
  lastPoll: bigint;
}

export interface AutoCreateAccountCacheValue {
  tx: HexString;
  fromAddress: HexString;
}
