export * as types from "./types";
export * as schemas from "../schemas/godwoken";
export * as extensions from "../schemas/extensions";

import { RPC } from "ckb-js-toolkit";
import {
  CellDep,
  HexString,
  HexNumber,
  Hash,
  Indexer,
  Script,
} from "@ckb-lumos/base";

export interface DeploymentConfig {
  deposition_lock: Script;
  custodian_lock: Script;
  state_validator_lock: Script;
  state_validator_type: Script;
  sudt_type: Script;

  deposition_lock_dep: CellDep;
  custodian_lock_dep: CellDep;
  state_validator_lock_dep: CellDep;
  state_validator_type_dep: CellDep;
  sudt_type_dep: CellDep;

  poa_state?: Script;
  poa_state_dep?: CellDep;
}

export function asyncSleep(ms = 0) {
  return new Promise((r) => setTimeout(r, ms));
}

export async function waitForBlockSync(
  indexer: Indexer,
  rpc: RPC,
  blockHash?: Hash,
  blockNumber?: bigint
) {
  if (!blockNumber) {
    const header = await rpc.get_header(blockHash);
    blockNumber = BigInt(header.number);
  }
  while (true) {
    await indexer.waitForSync();
    const tip = await indexer.tip();
    if (tip) {
      const indexedNumber = BigInt(tip.block_number);
      if (indexedNumber >= blockNumber) {
        // TODO: do we need to handle forks?
        break;
      }
    }
    await asyncSleep(2000);
  }
}
