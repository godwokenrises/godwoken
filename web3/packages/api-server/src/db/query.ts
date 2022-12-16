const newrelic = require("newrelic");
import { Hash } from "@ckb-lumos/base";
import {
  Block,
  Transaction,
  Log,
  DBBlock,
  DBTransaction,
  DBLog,
} from "./types";
import "./knex";
import Knex, { knex, Knex as KnexType } from "knex";
import { envConfig } from "../base/env-config";
import { LATEST_MEDIAN_GAS_PRICE } from "./constant";
import {
  formatDecimal,
  toBigIntOpt,
  formatBlock,
  formatTransaction,
  formatLog,
  buildQueryLogAddress,
  bufferToHex,
  hexToBuffer,
  getDatabaseRateLimitingConfiguration,
  buildQueryLogTopics,
  buildQueryLogBlock,
  buildQueryLogId,
} from "./helpers";
import { LimitExceedError } from "../methods/error";
import { FilterParams } from "../base/filter";
import KnexTimeoutError = knex.KnexTimeoutError;
import { logger } from "../base/logger";

const poolMax = envConfig.pgPoolMax || 20;
const GLOBAL_KNEX = Knex({
  client: "postgresql",
  connection: {
    connectionString: envConfig.databaseUrl,
    keepAlive: true,
  },
  pool: { min: 2, max: +poolMax },
});

export class Query {
  private knex: KnexType;

  constructor() {
    this.knex = GLOBAL_KNEX;
  }

  async isConnected() {
    try {
      await this.knex.raw("SELECT 1");
      return true;
    } catch (error: any) {
      logger.error(error.message);
      return false;
    }
  }

  async getEthTxHashByGwTxHash(gwTxHash: Hash): Promise<Hash | undefined> {
    const ethTxHash = await this.knex<DBTransaction>("transactions")
      .select("eth_tx_hash")
      .where({
        hash: hexToBuffer(gwTxHash),
      })
      .first();
    if (ethTxHash == null) {
      return undefined;
    }
    return bufferToHex(ethTxHash.eth_tx_hash);
  }

  async getGwTxHashByEthTxHash(ethTxHash: Hash): Promise<Hash | undefined> {
    const gwTxHash = await this.knex<DBTransaction>("transactions")
      .select("hash")
      .where({
        eth_tx_hash: hexToBuffer(ethTxHash),
      })
      .first();
    if (gwTxHash == null) {
      return undefined;
    }
    return bufferToHex(gwTxHash.hash);
  }

  async getTipBlockNumber(): Promise<bigint | undefined> {
    const blockData = await this.knex<DBBlock>("blocks")
      .select("number")
      .orderBy("number", "desc")
      .first();

    return toBigIntOpt(blockData?.number);
  }

  async getTipBlock(): Promise<Block | undefined> {
    const block = await this.knex<DBBlock>("blocks")
      .orderBy("number", "desc")
      .first();

    if (!block) {
      return undefined;
    }
    return formatBlock(block);
  }

  async getBlockByHash(blockHash: Hash): Promise<Block | undefined> {
    return await this.getBlock({
      hash: hexToBuffer(blockHash),
    });
  }

  async getBlockByNumber(blockNumber: bigint): Promise<Block | undefined> {
    return await this.getBlock({
      number: blockNumber.toString(),
    });
  }

  private async getBlock(
    params: Readonly<Partial<KnexType.MaybeRawRecord<DBBlock>>>
  ): Promise<Block | undefined> {
    const block = await this.knex<DBBlock>("blocks")
      .where(params)
      .first()
      .cache();
    if (!block) {
      return undefined;
    }
    return formatBlock(block);
  }

  // exclude min & include max;
  async getBlocksByNumbers(
    minBlockNumber: bigint,
    maxBlockNumber: bigint
  ): Promise<Block[]> {
    if (minBlockNumber >= maxBlockNumber) {
      return [];
    }
    const blocks = await this.knex<DBBlock>("blocks")
      .where("number", ">", minBlockNumber.toString())
      .andWhere("number", "<=", maxBlockNumber.toString())
      .orderBy("number", "asc")
      .cache();
    return blocks.map((block) => formatBlock(block));
  }

  async getBlockHashesAndNumbersAfterBlockNumber(
    number: bigint,
    order: "desc" | "asc" = "desc"
  ): Promise<{ hash: Hash; number: bigint }[]> {
    const arrayOfHashAndNumber = await this.knex<{
      hash: Buffer;
      number: bigint;
    }>("blocks")
      .select("hash", "number")
      .where("number", ">", number.toString())
      .orderBy("number", order)
      .cache();
    return arrayOfHashAndNumber.map((hn) => {
      return { hash: bufferToHex(hn.hash), number: BigInt(hn.number) };
    });
  }

  async getTransactionsByBlockHash(blockHash: Hash): Promise<Transaction[]> {
    return await this.getTransactions({ block_hash: hexToBuffer(blockHash) });
  }

  async getTransactionsByBlockNumber(
    blockNumber: bigint
  ): Promise<Transaction[]> {
    return await this.getTransactions({ block_number: blockNumber.toString() });
  }

  // Order by `transaction_index`, now only for search txs in a block
  private async getTransactions(
    params: Readonly<Partial<KnexType.MaybeRawRecord<DBTransaction>>>
  ): Promise<Transaction[]> {
    const transactions = await this.knex<DBTransaction>("transactions")
      .where(params)
      .orderBy("transaction_index", "asc")
      .cache();

    return transactions.map((tx) => formatTransaction(tx));
  }

  async getTransactionByHash(hash: Hash): Promise<Transaction | undefined> {
    return await this.getTransaction({
      hash: hexToBuffer(hash),
    });
  }

  async getTransactionByEthTxHash(
    eth_tx_hash: Hash
  ): Promise<Transaction | undefined> {
    return await this.getTransaction({
      eth_tx_hash: hexToBuffer(eth_tx_hash),
    });
  }

  async getTransactionByBlockHashAndIndex(
    blockHash: Hash,
    index: number
  ): Promise<Transaction | undefined> {
    return await this.getTransaction({
      block_hash: hexToBuffer(blockHash),
      transaction_index: index,
    });
  }

  async getTransactionByBlockNumberAndIndex(
    blockNumber: bigint,
    index: number
  ): Promise<Transaction | undefined> {
    return await this.getTransaction({
      block_number: blockNumber.toString(),
      transaction_index: index,
    });
  }

  private async getTransaction(
    params: Readonly<Partial<KnexType.MaybeRawRecord<DBTransaction>>>
  ): Promise<Transaction | undefined> {
    const transaction = await this.knex<DBTransaction>("transactions")
      .where(params)
      .first()
      .cache();

    if (transaction == null) {
      return undefined;
    }

    return formatTransaction(transaction);
  }

  async getTransactionEthHashesByBlockHash(blockHash: Hash): Promise<Hash[]> {
    return await this.getTransactionEthHashes({
      block_hash: hexToBuffer(blockHash),
    });
  }

  async getTransactionEthHashesByBlockNumber(
    blockNumber: bigint
  ): Promise<Hash[]> {
    return await this.getTransactionEthHashes({
      block_number: blockNumber.toString(),
    });
  }

  // Order by `transaction_index`, only for search tx eth hashes in a block.
  private async getTransactionEthHashes(
    params: Readonly<Partial<KnexType.MaybeRawRecord<DBTransaction>>>
  ): Promise<Hash[]> {
    const transactionHashes = await this.knex<DBTransaction>("transactions")
      .select("eth_tx_hash")
      .where(params)
      .orderBy("transaction_index", "asc")
      .cache();

    return transactionHashes.map((tx) => bufferToHex(tx.eth_tx_hash));
  }

  // undefined means not found
  async getBlockTransactionCountByHash(blockHash: Hash): Promise<number> {
    return await this.getBlockTransactionCount({
      block_hash: hexToBuffer(blockHash),
    });
  }

  async getBlockTransactionCountByNumber(blockNumber: bigint): Promise<number> {
    return await this.getBlockTransactionCount({
      block_number: blockNumber.toString(),
    });
  }

  private async getBlockTransactionCount(
    params: Readonly<Partial<KnexType.MaybeRawRecord<DBTransaction>>>
  ): Promise<number> {
    const data = await this.knex<DBTransaction>("transactions")
      .where(params)
      .count()
      .cache();

    const count: number = +data[0].count;

    return count;
  }

  async getTransactionAndLogsByHash(
    txHash: Hash
  ): Promise<[Transaction, Log[]] | undefined> {
    const tx = await this.knex<DBTransaction>("transactions")
      .where({ hash: hexToBuffer(txHash) })
      .first()
      .cache();

    if (!tx) {
      return undefined;
    }

    const logs = await this.knex<DBLog>("logs")
      .where({
        transaction_hash: hexToBuffer(txHash),
      })
      .orderBy("log_index", "asc")
      .cache();

    return [formatTransaction(tx), logs.map((log) => formatLog(log))];
  }

  async getTipLog() {
    let log = await this.knex<DBLog>("logs")
      .orderBy("id", "desc")
      .first()
      .cache();
    if (log != null) {
      return formatLog(log);
    }
    return null;
  }

  /**
   * @throws LimitExceedError - the number of queried results are over `MAX_QUERY_NUMBER`.
   */
  async getLogsByFilter(
    { addresses, topics, fromBlock, toBlock, blockHash }: FilterParams,
    lastPollId: bigint = BigInt(-1)
  ): Promise<Log[]> {
    const { MAX_QUERY_NUMBER, MAX_QUERY_TIME_MILSECS } =
      getDatabaseRateLimitingConfiguration();

    // NOTE: In this SQL, there is no `ORDER BY id` as combining `ORDER BY` and `LIMIT` consumes too much time when the
    // results are large. Instead, logs are sorted outside the database:
    // - When the number of query results exceeds $MAX_QUERY_NUMBER, an error is returned.
    // - When the queried results are less than or equal to $MAX_QUERY_NUMBER, sorting takes only a short time
    //
    // NOTE: Using CTE to SELECT "logs" then JOIN "transactions" is more efficient than directly SELECT JOIN
    let selectLogs = this.knex<DBLog>("logs")
      .modify(buildQueryLogAddress, addresses)
      .modify(buildQueryLogTopics, topics)
      .modify(buildQueryLogBlock, fromBlock, toBlock, blockHash)
      .modify(buildQueryLogId, lastPollId)
      .limit(MAX_QUERY_NUMBER + 1);
    let selectLogsJoinTransactions = this.knex<DBLog>("transactions")
      .with("logs", selectLogs)
      .select("logs.*", "transactions.eth_tx_hash")
      .join("logs", { "logs.transaction_hash": "transactions.hash" });
    let logs: DBLog[] = await selectLogsJoinTransactions
      .timeout(MAX_QUERY_TIME_MILSECS, { cancel: true })
      .cache()
      .catch((knexError: any) => {
        if (knexError instanceof KnexTimeoutError) {
          throw new LimitExceedError(`query timeout exceeded`);
        }
        throw knexError;
      });

    if (logs.length > MAX_QUERY_NUMBER) {
      throw new LimitExceedError(
        `query returned more than ${MAX_QUERY_NUMBER} results`
      );
    }

    return logs
      .map((log) => formatLog(log))
      .sort((aLog, bLog) => Number(aLog.id - bLog.id));
  }

  // Latest ${LATEST_MEDIAN_GAS_PRICE} transactions median gas_price
  async getMedianGasPrice(): Promise<bigint> {
    const sql = `SELECT (PERCENTILE_CONT(0.5) WITHIN GROUP(ORDER BY gas_price)) AS median FROM (SELECT gas_price FROM transactions ORDER BY id DESC LIMIT ?) AS gas_price;`;
    const result = await newrelic.startSegment(
      "getMedianGasPrice",
      true,
      async () => this.knex.raw(sql, [LATEST_MEDIAN_GAS_PRICE])
    );
    const median = result.rows[0]?.median;
    if (median == null) {
      return BigInt(0);
    }

    return formatDecimal(median.toString());
  }
}
