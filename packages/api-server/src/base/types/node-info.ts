import { Hash, HexNumber, HexString, Script } from "@ckb-lumos/base";
import {
  EoaScriptType,
  BackendType,
  NodeMode,
  GwScriptType,
} from "@godwoken-web3/godwoken";

export interface EoaScript {
  typeHash: Hash;
  script: Script;
  eoaType: EoaScriptType;
}

export interface BackendInfo {
  validatorCodeHash: Hash;
  generatorCodeHash: Hash;
  validatorScriptTypeHash: Hash;
  backendType: BackendType;
}

export interface GwScript {
  typeHash: Hash;
  script: Script;
  scriptType: GwScriptType;
}

export interface RollupCell {
  typeHash: Hash;
  typeScript: Script;
}

export interface RollupConfig {
  requiredStakingCapacity: HexNumber;
  challengeMaturityBlocks: HexNumber;
  finalityBlocks: HexNumber;
  rewardBurnRate: HexNumber;
  chainId: HexNumber;
}

export interface GaslessTxSupport {
  entrypointAddress: HexString;
}

export interface NodeInfo {
  backends: Array<BackendInfo>;
  eoaScripts: Array<EoaScript>;
  gwScripts: Array<GwScript>;
  rollupCell: RollupCell;
  rollupConfig: RollupConfig;
  version: string;
  mode: NodeMode;
  gaslessTxSupport?: GaslessTxSupport;
}
