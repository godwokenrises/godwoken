import { Hash, Script } from "@ckb-lumos/base";

export interface DeploymentConfig {
  rollup_type_hash: Hash;

  deposition_lock: Script;
  custodian_lock: Script;
  state_validator_type: Script;
}
