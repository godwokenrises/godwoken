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
import { Cell, HashType, HexString, core, utils } from "@ckb-lumos/base";
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
  .requiredOption("-c, --capacity <capacity>", "stake capacity in CKBs")
  .option("-p, --private-key <privateKey>", "private key to use")
  .option(
    "-h, --owner-lock-hash <ownerLockHash>",
    "L1 owner lock hash for unlocking the stake cell"
  );
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

const run = async () => {
  if (!program.privateKey) {
    throw new Error("You must either provide privateKey!");
  }

  initializeConfig();
  const publicKeyHash = privateKeyToPublicKeyHash(program.privateKey);
  const address = publicKeyHashToAddress(publicKeyHash);
  let ownerLockHash = program.ownerLockHash;
  if (!program.owenrLockHash) {
    ownerLockHash = publicKeyHashToLockHash(publicKeyHash);
  }
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

  console.log(`RollupTypeHash: ${getRollupTypeHash(runnerConfig)}`);
  console.log(`OwnerLockHash: ${ownerLockHash}`);
  // Add stake cell
  const stakeLockArgs = {
    owner_lock_hash: ownerLockHash,
    // default stake_block_number is 0, will be updated to actual L2 block number when this aggregator produce L2 block.
    stake_block_number: "0x0",
  };
  const ckbCapacity = program.capacity * 10 ** 8;
  const cell: Cell = {
    cell_output: {
      capacity: "0x" + BigInt(ckbCapacity).toString(16),
      lock: {
        code_hash: runnerConfig.deploymentConfig.stake_lock.code_hash,
        hash_type: runnerConfig.deploymentConfig.stake_lock.hash_type,
        args: packStakeLockArgs(getRollupTypeHash(runnerConfig), stakeLockArgs),
      },
    },
    data: "0x",
  };
  txSkeleton = txSkeleton.update("outputs", (outputs) => outputs.push(cell));
  // Add input cells and fee
  txSkeleton = await common.injectCapacity(
    txSkeleton,
    [address],
    BigInt(ckbCapacity)
  );
  txSkeleton = await common.payFeeByFeeRate(
    txSkeleton,
    [address],
    BigInt(1000)
  );
  txSkeleton = common.prepareSigningEntries(txSkeleton);

  const message = txSkeleton.get("signingEntries").get(0)!.message;

  const signature = key.signRecoverable(message, program.privateKey);
  const tx = sealTransaction(txSkeleton, [signature]);
  try {
    const txHash = await rpc.send_transaction(tx);
    console.log(`Transaction ${txHash} sent!`);
  } catch (e) {
    console.error(e);
  }
};

function packStakeLockArgs(
  rollupTypeHash: HexString,
  stakeLockArgs: object
): HexString {
  const packedWithdrawalLockArgs = schemas.SerializeStakeLockArgs(
    types.NormalizeStakeLockArgs(stakeLockArgs)
  );
  const buffer = new ArrayBuffer(32 + packedWithdrawalLockArgs.byteLength);
  const array = new Uint8Array(buffer);
  array.set(new Uint8Array(new Reader(rollupTypeHash).toArrayBuffer()), 0);
  array.set(new Uint8Array(packedWithdrawalLockArgs), 32);
  return new Reader(buffer).serializeJson();
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
run().then(() => {
  console.log("Completed!");
  process.exit(0);
});
