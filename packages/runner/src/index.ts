import { Command } from "commander";
import { argv } from "process";
import { RPC } from "ckb-js-toolkit";
import { Indexer } from "@ckb-lumos/sql-indexer";
import { Config, ChainService, GenesisSetup } from "@ckb-godwoken/godwoken";
import { DeploymentConfig } from "@ckb-godwoken/base";
import { initializeConfig } from "@ckb-lumos/config-manager";
import { Runner } from "./runner";
import { readFileSync } from "fs";
import Knex from "knex";

interface GenesisStoreConfig {
  type: "genesis";
  genesis: GenesisSetup;
}

type StoreConfig = GenesisStoreConfig;

interface RunnerConfig {
  deploymentConfig: DeploymentConfig;
  godwokenConfig: Config;
  storeConfig: StoreConfig;
}

const program = new Command();
program
  .requiredOption("-c, --config-file <configFile>", "runner config file")
  .requiredOption(
    "-s, --sql-connection <sqlConnection>",
    "PostgreSQL connection striong"
  )
  .requiredOption(
    "-p, --private-key <privateKey>",
    "aggregator private key to use"
  )
  .option("-r, --rpc <rpc>", "rpc path", "http://127.0.0.1:8114");
program.parse(argv);

initializeConfig();
const runnerConfig: RunnerConfig = JSON.parse(
  readFileSync(program.configFile, "utf8")
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

(async () => {
  const runner = new Runner(
    rpc,
    indexer,
    chainService,
    runnerConfig.deploymentConfig
  );
  await runner.start();
})().catch((e) => {
  console.error(`Error occurs: ${e}`);
});
