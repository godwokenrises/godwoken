import { RPC, Reader, normalizers } from "ckb-js-toolkit";
import {
  Cell,
  Header,
  HexNumber,
  HexString,
  OutPoint,
  Transaction,
  denormalizers,
  since as sinceUtils,
  core,
  utils,
} from "@ckb-lumos/base";
import { TransactionSkeletonType } from "@ckb-lumos/helpers";
import { DeploymentConfig, schemas, types } from "@ckb-godwoken/base";
import { Config } from "@ckb-godwoken/godwoken";
import { config as poaConfigModule } from "clerkb-lumos-integrator";

const { DenormalizeScript } = denormalizers;
const { readBigUInt128LE } = utils;

export type Level = "debug" | "info" | "warn" | "error";
export type Logger = (level: Level, message: string) => void;

export type StateValidatorLockGeneratorState = "Yes" | "YesIfFull" | "No";

export interface StateValidatorLockGenerator {
  shouldIssueNewBlock(
    medianTime: HexNumber,
    tipCell: Cell
  ): Promise<StateValidatorLockGeneratorState>;

  fixTransactionSkeleton(
    medianTime: HexNumber,
    txSkeleton: TransactionSkeletonType
  ): Promise<TransactionSkeletonType>;

  cancelIssueBlock(): Promise<void>;
}

export interface GenesisStoreConfig {
  type: "genesis";
  headerInfo: HexString;
}

export type StoreConfig = GenesisStoreConfig;

export interface AlwaysSuccessAggregatorConfig {
  type: "always_success";
}

export interface PoAConfig {
  type: "poa";
  config: poaConfigModule.Config;
}

export type AggregatorConfig = AlwaysSuccessAggregatorConfig | PoAConfig;

export interface RunnerConfig {
  deploymentConfig: DeploymentConfig;
  godwokenConfig: Config;
  storeConfig: StoreConfig;
  aggregatorConfig: AggregatorConfig;
}

export async function scanDepositionCellsInCommittedL2Block(
  l2Block: Transaction,
  config: RunnerConfig,
  rpc: RPC
): Promise<Array<HexString>> {
  const results: Array<HexString> = [];
  for (const input of l2Block.inputs) {
    const cell = await resolveOutPoint(input.previous_output, rpc);
    const entry = await tryExtractDepositionRequest(cell, config);
    if (entry) {
      results.push(entry.packedRequest);
    }
  }
  return results;
}

async function resolveOutPoint(outPoint: OutPoint, rpc: RPC): Promise<Cell> {
  const txStatus = await rpc.get_transaction(outPoint.tx_hash);
  if (!txStatus || !txStatus.transaction) {
    throw new Error(`Transaction ${outPoint.tx_hash} cannot be found!`);
  }
  const tx: Transaction = txStatus.transaction;
  const index = Number(BigInt(outPoint.index));
  if (index >= tx.outputs.length) {
    throw new Error(
      `Transaction ${outPoint.tx_hash} does not have output ${index}!`
    );
  }
  return {
    cell_output: tx.outputs[index],
    data: tx.outputs_data[index],
    out_point: outPoint,
    block_hash: txStatus.tx_status.block_hash,
  };
}

export interface DepositionEntry {
  cell: Cell;
  lockArgs: schemas.DepositionLockArgs;
  request: types.DepositionRequest;
  // Packed binary of gw_types::packed::DepositionRequest type
  packedRequest: HexString;
}

export async function tryExtractDepositionRequest(
  cell: Cell,
  config: RunnerConfig,
  tipHeader?: Header,
  cellHeader?: Header
): Promise<DepositionEntry | undefined> {
  if (
    cell.cell_output.lock.code_hash !==
      config.deploymentConfig.deposition_lock.code_hash ||
    cell.cell_output.lock.hash_type !==
      config.deploymentConfig.deposition_lock.hash_type
  ) {
    return undefined;
  }
  const args = new Reader(cell.cell_output.lock.args);
  if (args.length() < 32) {
    throw new Error("Invalid args length!");
  }
  const rollupTypeHash = args.serializeJson().substr(0, 66);
  const expectedRollupTypeHash = utils
    .ckbHash(
      core.SerializeScript(
        normalizers.NormalizeScript(
          config.godwokenConfig.chain.rollup_type_script
        )
      )
    )
    .serializeJson();
  if (rollupTypeHash !== expectedRollupTypeHash) {
    return undefined;
  }
  const lockArgs = new schemas.DepositionLockArgs(
    args.toArrayBuffer().slice(32)
  );
  if (tipHeader) {
    // Timeout validation
    const packedSince = new Reader(
      lockArgs.getCancelTimeout().raw()
    ).serializeJson();
    // TODO: lumos since validation bug
    if (sinceUtils.validateSince(packedSince, tipHeader, cellHeader)) {
      // Since has reached, meaning deposition request has timed out.
      return undefined;
    }
  }
  let amount = "0x0";
  if (!!cell.cell_output.type) {
    // SUDT
    amount = "0x" + readBigUInt128LE(cell.data).toString(16);
  }
  const sudtScript = cell.cell_output.type || {
    code_hash:
      "0x0000000000000000000000000000000000000000000000000000000000000000",
    hash_type: "data",
    args: "0x0000000000000000000000000000000000000000000000000000000000000000",
  };
  const request = {
    amount,
    capacity: cell.cell_output.capacity,
    script: DenormalizeScript(lockArgs.getLayer2Lock()),
    sudt_script_hash: utils.computeScriptHash(sudtScript),
  };
  const packedRequest = new Reader(
    schemas.SerializeDepositionRequest(
      types.NormalizeDepositionRequest(request)
    )
  ).serializeJson();
  return {
    cell,
    lockArgs,
    request,
    packedRequest,
  };
}
