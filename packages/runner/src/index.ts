import { Command } from "commander";
import { argv } from "process";
import { RPC } from "ckb-js-toolkit";
import { Indexer } from "@ckb-lumos/sql-indexer";
import { Config, ChainService } from "@ckb-godwoken/godwoken";
import { DeploymentConfig } from "@ckb-godwoken/base";
import { initializeConfig } from "@ckb-lumos/config-manager";
import { JsonrpcServer } from "./jsonrpc_server";
import { Runner } from "./runner";
import { Level, RunnerConfig } from "./utils";
import { readFileSync } from "fs";
import Knex from "knex";
import deepFreeze from "deep-freeze-strict";

const program = new Command();
// TODO: private key should come from an environment variable or config file,
// cli arguments is not secure enough.
program
  .requiredOption("-c, --config-file <configFile>", "runner config file")
  .requiredOption(
    "-s, --sql-connection <sqlConnection>",
    "PostgreSQL connection striong"
  )
  .option(
    "-p, --private-key <privateKey>",
    "aggregator private key to use, when omitted, readOnly mode will be used"
  )
  .option("-l, --listen <listen>", "JSONRPC listen path", "8119");
program.parse(argv);

initializeConfig();
const runnerConfig: RunnerConfig = deepFreeze(
  JSON.parse(readFileSync(program.configFile, "utf8"))
);

const rpc = new RPC(runnerConfig.godwokenConfig.rpc.listen);
const knex = Knex({
  client: "postgresql",
  connection: program.sqlConnection,
});
const indexer = new Indexer(runnerConfig.godwokenConfig.rpc.listen, knex);
indexer.startForever();

if (runnerConfig.storeConfig.type !== "genesis") {
  throw new Error("Only genesis store config is supported now!");
}

const chainService = new ChainService(
  runnerConfig.godwokenConfig,
  runnerConfig.storeConfig.genesis
);

function defaultLogger(level: Level, message: string) {
  console.log(`[${level}] ${message}`);
}

const jsonrpcServer = new JsonrpcServer(
  chainService,
  program.listen,
  !!program.privateKey,
  defaultLogger
);

const runner = new Runner(
  rpc,
  indexer,
  chainService,
  runnerConfig,
  program.privateKey,
  defaultLogger
);

Promise.all([jsonrpcServer.start(), runner.start()]).catch((e) => {
  console.error(`Error occurs: ${e}`);
});
