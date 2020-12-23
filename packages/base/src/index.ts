export * as types from "./types";
export * as schemas from "../schemas/godwoken";

import { CellDep, HexString, HexNumber, Hash, Script } from "@ckb-lumos/base";

export interface DeploymentConfig {
  deposition_lock: Script;
  custodian_lock: Script;
  state_validator_lock: Script;
  state_validator_type: Script;

  deposition_lock_dep: CellDep;
  custodian_lock_dep: CellDep;
  state_validator_lock_dep: CellDep;
  state_validator_type_dep: CellDep;
}
