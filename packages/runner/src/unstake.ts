import { Command } from "commander";
import { argv } from "process";
import { Reader, RPC, normalizers } from "ckb-js-toolkit";
import { RunnerConfig } from "./utils";
import deepFreeze from "deep-freeze-strict";
import { readFileSync } from "fs";
import { getConfig, initializeConfig } from "@ckb-lumos/config-manager";
import Knex from "knex";
import { Indexer } from "@ckb-lumos/sql-indexer";
import {
  TransactionSkeleton,
  scriptToAddress,
  sealTransaction,
  createTransactionFromSkeleton,
} from "@ckb-lumos/helpers";
import {
  Cell,
  CellDep,
  HexString,
  WitnessArgs,
  core,
  utils,
  QueryOptions,
} from "@ckb-lumos/base";
import { common } from "@ckb-lumos/common-scripts";
import { key } from "@ckb-lumos/hd";
import * as secp256k1 from "secp256k1";
import { types, schemas } from "@ckb-godwoken/base";

const program = new Command();
program
  .requiredOption("-f, --config-file <configFile>", "runner config file")
  .requiredOption(
    "-s, --sql-connection <sqlConnection>",
    "PostgreSQL connection striong"
  )
  .option("-p, --private-key <privateKey>", "private key to use");
program.parse(argv);

const runnerConfig: RunnerConfig = deepFreeze(
  JSON.parse(readFileSync(program.configFile, "utf8"))
);

function privateKeyToPublicKeyHash(privateKey: any) {
  const privateKeyBuffer = new Reader(privateKey).toArrayBuffer();
  const publicKeyArray = secp256k1.publicKeyCreate(
    new Uint8Array(privateKeyBuffer)
  );
  const publicKeyHash = utils
    .ckbHash(publicKeyArray.buffer)
    .serializeJson()
    .substr(0, 42);
  return publicKeyHash;
}

function publicKeyHashToAddress(publicKeyHash: any) {
  const scriptConfig = getConfig().SCRIPTS.SECP256K1_BLAKE160!;
  const script = {
    code_hash: scriptConfig.CODE_HASH,
    hash_type: scriptConfig.HASH_TYPE,
    args: publicKeyHash,
  };
  return scriptToAddress(script);
}

function publicKeyHashToLockHash(publicKeyHash: any) {
  const scriptConfig = getConfig().SCRIPTS.SECP256K1_BLAKE160!;
  const script = {
    code_hash: scriptConfig.CODE_HASH,
    hash_type: scriptConfig.HASH_TYPE,
    args: publicKeyHash,
  };
  return utils.computeScriptHash(script);
}

function unpackStakeLockArgs(packedStakeLockArgs: HexString) {
  const buffer = new Reader(packedStakeLockArgs).toArrayBuffer();
  const array = new Uint8Array(buffer);
  const stakeLockArgs = array.slice(32);
  return types.DenormalizeStakeLockArgs(
    new schemas.StakeLockArgs(stakeLockArgs.buffer)
  );
}

function getRollupTypeHash(runnerConfig: RunnerConfig): HexString {
  return utils
    .ckbHash(
      core.SerializeScript(
        normalizers.NormalizeScript(
          runnerConfig.godwokenConfig.chain.rollup_type_script
        )
      )
    )
    .serializeJson();
}

async function queryValidStakeCell(
  indexer: Indexer,
  ownerLockHash: HexString,
  runnerConfig: RunnerConfig,
  lastFinalizedBlockNumber: HexString
): Promise<Cell> {
  const stakeCellQueryOptions: QueryOptions = {
    lock: {
      script: {
        code_hash: runnerConfig.deploymentConfig.stake_lock.code_hash,
        hash_type: runnerConfig.deploymentConfig.stake_lock.hash_type,
        args: getRollupTypeHash(runnerConfig),
      },
      argsLen: "any",
    },
  };
  const cellCollector = indexer.collector(stakeCellQueryOptions);
  for await (const cell of cellCollector.collect()) {
    const stakeLockArgs = unpackStakeLockArgs(cell.cell_output.lock.args);
    if (
      ownerLockHash === stakeLockArgs.owner_lock_hash &&
      BigInt(stakeLockArgs.stake_block_number) <=
        BigInt(lastFinalizedBlockNumber)
    ) {
      return cell;
    }
  }
  throw new Error(
    "No valid stake cell matches the ownerLockHash and stakeBlockNumber is smaller than globalStates's finalized block number"
  );
}

async function queryValidSecp256k1Cell(
  indexer: Indexer,
  publicKeyHash: HexString
): Promise<Cell> {
  const queryOptions: QueryOptions = {
    lock: {
      code_hash: getConfig().SCRIPTS.SECP256K1_BLAKE160!.CODE_HASH,
      hash_type: getConfig().SCRIPTS.SECP256K1_BLAKE160!.HASH_TYPE,
      args: publicKeyHash,
    },
    type: "empty",
  };
  const cellCollector = indexer.collector(queryOptions);
  for await (const cell of cellCollector.collect()) {
    return cell;
  }
  throw new Error("No valid output cell matches the ownerLockHash");
}

async function queryValidRollupCell(indexer: Indexer): Promise<Cell> {
  const queryOptions: QueryOptions = {
    type: runnerConfig.godwokenConfig.chain.rollup_type_script,
  };
  const cellCollector = indexer.collector(queryOptions);
  for await (const cell of cellCollector.collect()) {
    return cell;
  }
  throw new Error("No valid rollup cell found!");
}

function buildSecp256k1WitnessArgsPlaceHolder(): HexString {
  const SECP_SIGNATURE_PLACEHOLDER =
    "0x0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
  const witnessArgs: WitnessArgs = {
    lock: SECP_SIGNATURE_PLACEHOLDER,
  };
  return new Reader(
    core.SerializeWitnessArgs(normalizers.NormalizeWitnessArgs(witnessArgs))
  ).serializeJson();
}

const run = async () => {
  if (!program.privateKey) {
    throw new Error("You must either provide privateKey!");
  }

  initializeConfig();
  const publicKeyHash = privateKeyToPublicKeyHash(program.privateKey);
  let ownerLockHash = program.ownerLockHash;
  if (!program.owenrLockHash) {
    ownerLockHash = publicKeyHashToLockHash(publicKeyHash);
  }
  console.log(`RollupTypeHash: ${getRollupTypeHash(runnerConfig)}`);
  console.log(`OwnerLockHash: ${ownerLockHash}`);
  const rpc = new RPC(runnerConfig.rpc.listen);
  const knex = Knex({
    client: "postgresql",
    connection: program.sqlConnection,
  });
  const indexer = new Indexer(runnerConfig.rpc.listen, knex);
  indexer.startForever();
  await indexer.waitForSync();
  console.log("Syncing done!");

  let txSkeleton = TransactionSkeleton({ cellProvider: indexer });

  // Add stake cell dep
  txSkeleton = txSkeleton.update("cellDeps", (cellDeps) =>
    cellDeps.push(runnerConfig.deploymentConfig.stake_lock_dep)
  );
  // Add rollup cell dep(not state validator dep)
  const rollupCell: Cell = await queryValidRollupCell(indexer);
  const rollupCellDep: CellDep = {
    out_point: rollupCell.out_point!,
    dep_type: "code",
  };
  txSkeleton = txSkeleton.update("cellDeps", (cellDeps) =>
    cellDeps.push(rollupCellDep)
  );
  // Add secp256k1 cell dep
  const secp256k1CellDep: CellDep = {
    out_point: {
      tx_hash: getConfig().SCRIPTS.SECP256K1_BLAKE160!.TX_HASH,
      index: getConfig().SCRIPTS.SECP256K1_BLAKE160!.INDEX,
    },
    dep_type: getConfig().SCRIPTS.SECP256K1_BLAKE160!.DEP_TYPE,
  };
  txSkeleton = txSkeleton.update("cellDeps", (cellDeps) =>
    cellDeps.push(secp256k1CellDep)
  );
  // Add an input cell with owner_lock_hash equals stakeCell's ownerLockHash
  const inputCell: Cell = await queryValidSecp256k1Cell(indexer, publicKeyHash);
  console.log(inputCell);
  txSkeleton = txSkeleton.update("inputs", (inputs) => inputs.push(inputCell));
  txSkeleton = txSkeleton.update("witnesses", (witnesses) =>
    witnesses.push(buildSecp256k1WitnessArgsPlaceHolder())
  );
  // Add input stake cell
  const globalState = types.DenormalizeGlobalState(
    new schemas.GlobalState(new Reader(rollupCell.data).toArrayBuffer())
  );

  const stakeCell: Cell = await queryValidStakeCell(
    indexer,
    ownerLockHash,
    runnerConfig,
    globalState.last_finalized_block_number
  );
  txSkeleton = txSkeleton.update("inputs", (inputs) => inputs.push(stakeCell));
  txSkeleton = txSkeleton.update("witnesses", (witnesses) =>
    witnesses.push("0x")
  );

  // Add output cells
  const capacityMinusFee =
    BigInt(stakeCell.cell_output.capacity) +
    BigInt(inputCell.cell_output.capacity) -
    BigInt(0.001 * 10 ** 8);
  const outputCell: Cell = {
    cell_output: {
      capacity: "0x" + capacityMinusFee.toString(16),
      lock: {
        code_hash: getConfig().SCRIPTS.SECP256K1_BLAKE160!.CODE_HASH,
        hash_type: getConfig().SCRIPTS.SECP256K1_BLAKE160!.HASH_TYPE,
        args: publicKeyHash,
      },
    },
    data: "0x",
  };
  txSkeleton = txSkeleton.update("outputs", (outputs) =>
    outputs.push(outputCell)
  );

  // Only need sign the first input cell
  txSkeleton = common.prepareSigningEntries(txSkeleton);
  const message = txSkeleton.get("signingEntries").get(0)!.message;
  const signature = key.signRecoverable(message, program.privateKey);
  const tx = sealTransaction(txSkeleton, [signature]);
  console.log(JSON.stringify(tx, null, 2));
  try {
    const txHash = await rpc.send_transaction(tx);
    console.log(`Transaction ${txHash} sent!`);
  } catch (e) {
    console.error(e);
  }
};

run().then(() => {
  console.log("Completed!");
  process.exit(0);
});
