import { Command } from "commander";
import { argv } from "process";
import { initializeConfig } from "@ckb-lumos/config-manager";
import { Indexer } from "@ckb-lumos/sql-indexer";
import { exit } from "process";
import Knex from "knex";

const program = new Command();
program
  .requiredOption(
    "-s, --sql-connection <sqlConnection>",
    "PostgreSQL connection striong"
  )
  .option("-r, --rpc <rpc>", "rpc path", "http://127.0.0.1:8114");
program.parse(argv);

const run = async () => {
  initializeConfig();
  const knex = Knex({
    client: "postgresql",
    connection: program.sqlConnection,
  });
  const indexer = new Indexer(program.rpc, knex);
  indexer.startForever();
  console.log("Starting syncing!");
  await indexer.waitForSync();
  console.log("Syncing done!");
};

run().then(() => {
  console.log("Completed!");
  exit(0);
});
