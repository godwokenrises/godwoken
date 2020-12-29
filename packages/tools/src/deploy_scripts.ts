import { Command } from "commander";
import { argv } from "process";
import { Reader, RPC, normalizers } from "ckb-js-toolkit";
import { Cell, Hash, core, utils } from "@ckb-lumos/base";
import { common } from "@ckb-lumos/common-scripts";
import { getConfig, initializeConfig } from "@ckb-lumos/config-manager";
import {
  TransactionSkeleton,
  minimalCellCapacity,
  scriptToAddress,
  sealTransaction,
} from "@ckb-lumos/helpers";
import { Indexer } from "@ckb-lumos/sql-indexer";
import { asyncSleep, waitForBlockSync } from "@ckb-godwoken/base";
import { readFileSync, writeFileSync } from "fs";
import { dirname, join } from "path";
import { exit } from "process";
import * as secp256k1 from "secp256k1";
import Knex from "knex";

const program = new Command();
program
  .requiredOption("-f, --file <file>", "index file for scripts")
  .requiredOption(
    "-o, --output-file <outputFile>",
    "output file for deployments"
  )
  .requiredOption(
    "-s, --sql-connection <sqlConnection>",
    "PostgreSQL connection striong"
  )
  .option("-p, --private-key <privateKey>", "private key to use")
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

  const configs = JSON.parse(readFileSync(program.file, "utf8"));
  const basePath = dirname(program.file);
  const deployments: any = {};
  const address = ckbAddress(program.address, program.privateKey);

  for (const name of Object.keys(configs.programs)) {
    const binaryPath = join(basePath, configs.programs[name]);
    const binary = readFileSync(binaryPath);

    const cell: Cell = {
      cell_output: {
        capacity: "0x0",
        lock: configs.lock,
        type: {
          code_hash:
            "0x00000000000000000000000000000000000000000000000000545950455f4944",
          hash_type: "type",
          args:
            "0x00000000000000000000000000000000000000000000000000545950455f4944",
        },
      },
      data: "0x" + binary.toString("hex"),
    };
    const cellCapacity = minimalCellCapacity(cell);
    cell.cell_output.capacity = "0x" + cellCapacity.toString(16);

    let txSkeleton = TransactionSkeleton({ cellProvider: indexer });
    txSkeleton = txSkeleton.update("outputs", (outputs) => outputs.push(cell));
    txSkeleton = txSkeleton.update("fixedEntries", (fixedEntries) => {
      return fixedEntries.push({
        field: "outputs",
        index: 0,
      });
    });
    txSkeleton = await common.injectCapacity(
      txSkeleton,
      [address],
      cellCapacity
    );
    txSkeleton = txSkeleton.update("fixedEntries", (fixedEntries) => {
      return fixedEntries.push({
        field: "inputs",
        index: 0,
      });
    });
    // Type ID
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
    view.setBigUint64(0, 0n, true);
    typeIdHasher.update(buffer);
    const typeId = typeIdHasher.digestHex();
    txSkeleton = txSkeleton.update("outputs", (outputs) => {
      return outputs.update(0, (output) => {
        output.cell_output.type!.args = typeId;
        return output;
      });
    });
    const typeScript = txSkeleton.get("outputs").get(0)!.cell_output.type!;
    const typeScriptHash = utils
      .ckbHash(core.SerializeScript(normalizers.NormalizeScript(typeScript)))
      .serializeJson();
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
        await waitForBlockSync(indexer, rpc, txWithStatus.tx_status.block_hash);
        break;
      }
    }

    // Write to deployments
    deployments[name] = {
      code_hash: typeScriptHash,
      hash_type: "type",
      args: "0x",
    };
    deployments[`${name}_dep`] = {
      dep_type: "code",
      out_point: {
        tx_hash: txHash,
        index: "0x0",
      },
    };
  }
  deployments["rollup_type_hash"] =
    "0x0000000000000000000000000000000000000000000000000000000000000000";

  writeFileSync(
    program.outputFile,
    JSON.stringify(deployments, null, 2),
    "utf8"
  );
};

run().then(() => {
  console.log("Completed!");
  exit(0);
});
