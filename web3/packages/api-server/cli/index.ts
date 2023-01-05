import { Command } from "commander";
import { fixEthTxHashRun, listWrongEthTxHashesRun } from "./fix-eth-tx-hash";
import { version as packageVersion } from "../package.json";
import {
  fixLogTransactionIndexRun,
  wrongLogTransactionIndexCountRun,
} from "./fix-log-transaction-index";

const program = new Command();
program.version(packageVersion);

program
  .command("fix-eth-tx-hash")
  .description("Fix eth_tx_hash in database where R or S with leading zeros")
  .option(
    "-d, --database-url <database url>",
    "If not provide, will use env `DATABASE_URL`, throw error if not provided too",
    undefined
  )
  .option(
    "-c, --chain-id <chain id>",
    "Godwoken chain id, if not provoide, will get from RPC",
    undefined
  )
  .option("-r, --rpc <rpc>", "Godwoken / Web3 RPC url", "http://127.0.0.1:8024")
  .action(fixEthTxHashRun);

program
  .command("list-wrong-eth-tx-hashes")
  .description(
    "List transactions which R or S with leading zeros, only list first 20 txs"
  )
  .option("-d, --database-url <database url>", "database url", undefined)
  .action(listWrongEthTxHashesRun);

program
  .command("fix-log-transaction-index")
  .description("Fix wrong log's transaction_index")
  .option(
    "-d, --database-url <database url>",
    "If not provide, will use env `DATABASE_URL`, throw error if not provided too",
    undefined
  )
  .action(fixLogTransactionIndexRun);

program
  .command("wrong-log-transaction-index-count")
  .description("Get log's count which transaction_index is wrong")
  .option("-d, --database-url <database url>", "database url", undefined)
  .action(wrongLogTransactionIndexCountRun);

program.parse();
