import { Command } from "commander";
import { argv } from "process";
import { Reader, RPC, normalizers } from "ckb-js-toolkit";
import {
  asyncSleep,
  DeploymentConfig,
  schemas,
  types,
} from "@ckb-godwoken/base";
import { Config, buildGenesisBlock } from "@ckb-godwoken/godwoken";
import { Indexer } from "@ckb-lumos/sql-indexer";
import { Cell, HashType, HexString, core, utils } from "@ckb-lumos/base";
import { common } from "@ckb-lumos/common-scripts";
import { getConfig, initializeConfig } from "@ckb-lumos/config-manager";
import {
  TransactionSkeleton,
  TransactionSkeletonType,
  minimalCellCapacity,
  scriptToAddress,
  sealTransaction,
} from "@ckb-lumos/helpers";
import { readFileSync, writeFileSync } from "fs";
import { dirname, join } from "path";
import { exit } from "process";
import * as secp256k1 from "secp256k1";
import Knex from "knex";
import { config as poaConfigModule } from "clerkb-lumos-integrator";

const program = new Command();
program
  .requiredOption(
    "-d, --deployment-file <deploymentFile>",
    "deployment info file for scripts"
  )
  .requiredOption(
    "-c, --config-file <configFile>",
    "config file for godwoken setup"
  )
  .requiredOption(
    "-o, --output-file <outputFile>",
    "output file for complete godwoken runner setup"
  )
  .requiredOption(
    "-s, --sql-connection <sqlConnection>",
    "PostgreSQL connection striong"
  )
  .requiredOption("-p, --private-key <privateKey>", "private key to use")
  .option(
    "-e, --poa-setup-file <poaSetupFile>",
    "poa setup file, use PoA lock if this is present"
  )
  .option("-a, --address <address>", "address to use")
  .option("-r, --rpc <rpc>", "rpc path", "http://127.0.0.1:8114");
program.parse(argv);

function ckbAddress(address: any, privateKey: any) {
  if (address) {
    return address;
  }
  const privateKeyBuffer = new Reader(privateKey).toArrayBuffer();
  const publicKeyArray = secp256k1.publicKeyCreate(
    new Uint8Array(privateKeyBuffer)
  );
  const publicKeyHash = utils
    .ckbHash(publicKeyArray.buffer)
    .serializeJson()
    .substr(0, 42);
  const scriptConfig = getConfig().SCRIPTS.SECP256K1_BLAKE160!;
  const script = {
    code_hash: scriptConfig.CODE_HASH,
    hash_type: scriptConfig.HASH_TYPE,
    args: publicKeyHash,
  };
  return scriptToAddress(script);
}

function calculateTypeId(
  txSkeleton: TransactionSkeletonType,
  outputIndex: number
): HexString {
  const firstInput = {
    previous_output: txSkeleton.get("inputs").get(0)!.out_point,
    since: txSkeleton.get("inputSinces").get(0) || "0x0",
  };
  const typeIdHasher = new utils.CKBHasher();
  typeIdHasher.update(
    core.SerializeCellInput(normalizers.NormalizeCellInput(firstInput))
  );
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  view.setBigUint64(0, BigInt(outputIndex), true);
  typeIdHasher.update(buffer);
  return typeIdHasher.digestHex();
}

const run = async () => {
  if (!program.privateKey && !program.address) {
    throw new Error("You must either provide privateKey or address!");
  }

  initializeConfig();
  const rpc = new RPC(program.rpc);
  const knex = Knex({
    client: "postgresql",
    connection: program.sqlConnection,
  });
  const indexer = new Indexer(program.rpc, knex);
  indexer.startForever();
  await indexer.waitForSync();
  console.log("Syncing done!");

  let poaConfig: poaConfigModule.Config | undefined = undefined;
  if (program.poaSetupFile) {
    poaConfig = poaConfigModule.readConfig(program.poaSetupFile);
  }
  const deploymentConfig: DeploymentConfig = JSON.parse(
    readFileSync(program.deploymentFile, "utf8")
  );
  const godwokenConfig: Config = JSON.parse(
    readFileSync(program.configFile, "utf8")
  );
  const address = ckbAddress(program.address, program.privateKey);
  const genesis = await buildGenesisBlock(godwokenConfig.genesis);

  let txSkeleton = TransactionSkeleton({ cellProvider: indexer });
  txSkeleton = txSkeleton.update("cellDeps", (cellDeps) =>
    cellDeps.push(deploymentConfig.state_validator_type_dep)
  );

  // Insert main rollup cell
  const cell: Cell = {
    cell_output: {
      capacity: "0x0",
      lock: deploymentConfig.state_validator_lock,
      type: {
        code_hash: deploymentConfig.state_validator_type.code_hash,
        hash_type: deploymentConfig.state_validator_type.hash_type,
        args:
          "0x00000000000000000000000000000000000000000000000000545950455f4944",
      },
    },
    data: genesis.global_state,
  };
  cell.cell_output.capacity = "0x" + minimalCellCapacity(cell).toString(16);
  txSkeleton = txSkeleton
    .update("outputs", (outputs) => outputs.push(cell))
    .update("fixedEntries", (fixedEntries) => {
      return fixedEntries.push({
        field: "outputs",
        index: 0,
      });
    });

  if (poaConfig) {
    // PoA requires 64 byte lock args
    txSkeleton = txSkeleton.update("outputs", (outputs) => {
      return outputs.update(0, (output) => {
        output.cell_output.lock.args =
          "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
        output.cell_output.capacity =
          "0x" + minimalCellCapacity(output).toString(16);
        return output;
      });
    });
    // Insert PoA data cell and PoA setup cell
    const medianTime =
      BigInt((await rpc.get_blockchain_info()).median_time) / 1000n;
    const startOutputIndex = txSkeleton.get("outputs").count();
    // Generate PoA Setup cell
    const poaSetupCell = {
      cell_output: {
        capacity: "0x0",
        lock: {
          code_hash: deploymentConfig.poa_state!.code_hash,
          hash_type: deploymentConfig.poa_state!.hash_type,
          args:
            "0x0000000000000000000000000000000000000000000000000000000000000000",
        },
        type: {
          code_hash:
            "0x00000000000000000000000000000000000000000000000000545950455f4944",
          hash_type: "type" as HashType,
          args:
            "0x0000000000000000000000000000000000000000000000000000000000000000",
        },
      },
      data: new Reader(
        poaConfigModule.serializePoASetup(poaConfig.poa_setup)
      ).serializeJson(),
    };
    poaSetupCell.cell_output.capacity =
      "0x" + minimalCellCapacity(poaSetupCell).toString(16);
    // Generate PoA Data cell
    const poaData = {
      round_initial_subtime: medianTime,
      subblock_subtime: medianTime,
      subblock_index: 0,
      aggregator_index: 0,
    };
    const poaDataCell = {
      cell_output: {
        capacity: "0x0",
        lock: {
          code_hash: deploymentConfig.poa_state!.code_hash,
          hash_type: deploymentConfig.poa_state!.hash_type,
          args:
            "0x0000000000000000000000000000000000000000000000000000000000000000",
        },
        type: {
          code_hash:
            "0x00000000000000000000000000000000000000000000000000545950455f4944",
          hash_type: "type" as HashType,
          args:
            "0x0000000000000000000000000000000000000000000000000000000000000000",
        },
      },
      data: new Reader(
        poaConfigModule.serializePoAData(poaData)
      ).serializeJson(),
    };
    poaDataCell.cell_output.capacity =
      "0x" + minimalCellCapacity(poaDataCell).toString(16);
    // Insert both cells into transaction skeleton, inject capacity to hold the 2
    // new cells, as well as the newly added lock args.
    txSkeleton = txSkeleton
      .update("outputs", (outputs) => {
        return outputs.push(poaSetupCell).push(poaDataCell);
      })
      .update("fixedEntries", (fixedEntries) => {
        return fixedEntries
          .push({
            field: "outputs",
            index: 1,
          })
          .push({
            field: "outputs",
            index: 2,
          });
      });
  }

  // Provide capacity for all outputs.
  let capacity = 0n;
  for (const cell of txSkeleton.get("outputs").toArray()) {
    capacity += minimalCellCapacity(cell);
  }
  txSkeleton = await common.injectCapacity(txSkeleton, [address], capacity);
  txSkeleton = txSkeleton.update("fixedEntries", (fixedEntries) => {
    return fixedEntries.push({
      field: "inputs",
      index: 0,
    });
  });

  // Setup Type IDs
  const typeId = calculateTypeId(txSkeleton, 0);
  txSkeleton = txSkeleton.update("outputs", (outputs) => {
    return outputs.update(0, (output) => {
      output.cell_output.type!.args = typeId;
      return output;
    });
  });
  const typeScript = txSkeleton.get("outputs").get(0)!.cell_output.type!;
  if (poaConfig) {
    const poaSetupCellTypeId = calculateTypeId(txSkeleton, 1);
    const poaDataCellTypeId = calculateTypeId(txSkeleton, 2);
    txSkeleton = txSkeleton.update("outputs", (outputs) => {
      // Use the combination of PoA setup cell's type ID, and PoA data cell's type ID
      // as PoA's main lock args.
      const args = poaSetupCellTypeId + poaDataCellTypeId.substr(2);
      return outputs.update(0, (output) => {
        output.cell_output.lock.args = args;
        return output;
      });
    });
    const lockScriptHash = utils.computeScriptHash(
      txSkeleton.get("outputs").get(0)!.cell_output.lock
    );
    txSkeleton = txSkeleton.update("outputs", (outputs) => {
      return outputs
        .update(1, (output) => {
          output.cell_output.type!.args = poaSetupCellTypeId;
          output.cell_output.lock.args = lockScriptHash;
          return output;
        })
        .update(2, (output) => {
          output.cell_output.type!.args = poaDataCellTypeId;
          output.cell_output.lock.args = lockScriptHash;
          return output;
        });
    });
  }

  // L2Block is kept in witness field.
  txSkeleton = txSkeleton.update("witnesses", (witnesses) => {
    return witnesses.update(0, (witness) => {
      const originalWitnessArgs = new core.WitnessArgs(new Reader(witness));
      const witnessArgs: any = {};
      if (originalWitnessArgs.getLock().hasValue()) {
        witnessArgs.lock = new Reader(
          originalWitnessArgs.getLock().value().raw()
        ).serializeJson();
      }
      if (originalWitnessArgs.getInputType().hasValue()) {
        witnessArgs.input_type = new Reader(
          originalWitnessArgs.getInputType().value().raw()
        ).serializeJson();
      }
      witnessArgs.output_type = genesis.genesis;
      return new Reader(
        core.SerializeWitnessArgs(normalizers.NormalizeWitnessArgs(witnessArgs))
      ).serializeJson();
    });
  });

  txSkeleton = await common.payFeeByFeeRate(
    txSkeleton,
    [address],
    BigInt(1000)
  );
  txSkeleton = common.prepareSigningEntries(txSkeleton);

  const signatures = [];
  for (const { message } of txSkeleton.get("signingEntries").toArray()) {
    if (!program.privateKey) {
      throw new Error("Implement signing prompt!");
    }
    const signObject = secp256k1.ecdsaSign(
      new Uint8Array(new Reader(message).toArrayBuffer()),
      new Uint8Array(new Reader(program.privateKey).toArrayBuffer())
    );
    const signatureBuffer = new ArrayBuffer(65);
    const signatureArray = new Uint8Array(signatureBuffer);
    signatureArray.set(signObject.signature, 0);
    signatureArray.set([signObject.recid], 64);
    const signature = new Reader(signatureBuffer).serializeJson();
    signatures.push(signature);
  }
  const tx = sealTransaction(txSkeleton, signatures);
  const txHash = await rpc.send_transaction(tx);
  console.log(`Transaction ${txHash} sent!`);

  // Wait for tx to land on chain.
  while (true) {
    await asyncSleep(1000);
    const txWithStatus = await rpc.get_transaction(txHash);
    if (
      txWithStatus &&
      txWithStatus.tx_status &&
      txWithStatus.tx_status.status === "committed"
    ) {
      break;
    }
  }

  const txWithStatus = await rpc.get_transaction(txHash);
  const blockHash = txWithStatus.tx_status.block_hash;
  const header = await rpc.get_header(blockHash);

  const headerInfo = {
    number: header.number,
    block_hash: blockHash,
  };
  const packedHeaderInfo = schemas.SerializeHeaderInfo(
    types.NormalizeHeaderInfo(headerInfo)
  );
  godwokenConfig.chain = {
    rollup_type_script: typeScript,
  };
  const runnerConfig = {
    deploymentConfig,
    godwokenConfig,
    storeConfig: {
      type: "genesis",
      headerInfo: new Reader(packedHeaderInfo).serializeJson(),
    },
    aggregatorConfig: undefined as any,
  };
  if (poaConfig) {
    runnerConfig.aggregatorConfig = {
      type: "poa",
      config: poaConfig,
    };
  } else {
    runnerConfig.aggregatorConfig = { type: "always_success" };
  }

  writeFileSync(
    program.outputFile,
    JSON.stringify(runnerConfig, null, 2),
    "utf8"
  );
};

run().then(() => {
  console.log("Completed!");
  exit(0);
});
