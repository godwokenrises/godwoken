import { Hash, HexNumber, HexString } from "@ckb-lumos/base";
import { FilterTopic } from "../base/filter";
import {
  Block,
  Transaction,
  Log,
  DBBlock,
  DBTransaction,
  DBLog,
} from "./types";
import {
  DEFAULT_MAX_QUERY_NUMBER,
  DEFAULT_MAX_QUERY_TIME_MILSECS,
} from "./constant";
import { Knex as KnexType } from "knex";
import { envConfig } from "../base/env-config";

export function toBigIntOpt(
  num: bigint | HexNumber | undefined
): bigint | undefined {
  if (num == null) {
    return num as undefined;
  }

  return BigInt(num);
}

export function formatDecimal(dec: string) {
  const nums = dec.split(".");
  const wholeNum = BigInt(nums[0]);
  const smallNum = nums[1] == null ? 0 : +nums[1];
  if (smallNum > 0) {
    return wholeNum + 1n;
  }
  return wholeNum;
}

export function hexToBuffer(hex: HexString): Buffer {
  return Buffer.from(hex.slice(2), "hex");
}

export function bufferToHex(buf: Buffer): HexString {
  return "0x" + buf.toString("hex");
}

function bufferToHexOpt(buf?: Buffer): HexString | undefined {
  return buf == null ? undefined : bufferToHex(buf);
}

export function formatBlock(block: DBBlock): Block {
  return {
    ...block,
    number: BigInt(block.number),
    gas_limit: BigInt(block.gas_limit),
    gas_used: BigInt(block.gas_used),
    size: BigInt(block.size),
    hash: bufferToHex(block.hash),
    parent_hash: bufferToHex(block.parent_hash),
    miner: bufferToHex(block.miner),
    logs_bloom: "0x",
  };
}

export function formatTransaction(tx: DBTransaction): Transaction {
  return {
    ...tx,
    id: BigInt(tx.id),
    block_number: BigInt(tx.block_number),
    transaction_index: +tx.transaction_index,
    value: BigInt(tx.value),
    nonce: toBigIntOpt(tx.nonce),
    gas_limit: toBigIntOpt(tx.gas_limit),
    gas_price: toBigIntOpt(tx.gas_price),
    v: BigInt(tx.v),
    cumulative_gas_used: toBigIntOpt(tx.cumulative_gas_used),
    gas_used: toBigIntOpt(tx.gas_used),
    exit_code: +tx.exit_code,
    hash: bufferToHex(tx.hash),
    eth_tx_hash: bufferToHex(tx.eth_tx_hash),
    block_hash: bufferToHex(tx.block_hash),
    from_address: bufferToHex(tx.from_address),
    to_address: bufferToHexOpt(tx.to_address),
    input: bufferToHexOpt(tx.input),
    r: bufferToHex(tx.r),
    s: bufferToHex(tx.s),
    contract_address: bufferToHexOpt(tx.contract_address),
    logs_bloom: "0x",
    chain_id: toBigIntOpt(tx.chain_id),
  };
}

export function formatLog(log: DBLog): Log {
  return {
    ...log,
    id: BigInt(log.id),
    transaction_id: BigInt(log.transaction_id),
    transaction_index: +log.transaction_index,
    block_number: BigInt(log.block_number),
    log_index: +log.log_index,
    transaction_hash: bufferToHex(log.transaction_hash),
    eth_tx_hash: log.eth_tx_hash ? bufferToHex(log.eth_tx_hash) : undefined,
    block_hash: bufferToHex(log.block_hash),
    address: bufferToHex(log.address),
    data: bufferToHex(log.data),
    topics: log.topics.map((t) => bufferToHex(t)),
  };
}

export function normalizeQueryAddress(address: HexString) {
  if (address && typeof address === "string") {
    return address.toLowerCase();
  }

  return address;
}

export function normalizeLogQueryAddress(
  address: HexString | HexString[] | undefined
) {
  if (!address) {
    return address;
  }

  if (address && Array.isArray(address)) {
    return address.map((a) => normalizeQueryAddress(a));
  }

  return normalizeQueryAddress(address);
}

export function universalizeAddress(
  address: undefined | HexString | HexString[]
) {
  const normalizedAddress: undefined | HexString | HexString[] =
    normalizeLogQueryAddress(address);
  if (normalizedAddress == null) {
    return [];
  } else if (typeof normalizedAddress === "string") {
    return [normalizedAddress];
  } else {
    return normalizedAddress;
  }
}

/*
return a slice of log array which satisfy the topics matching.
matching rule:
      Topics are order-dependent. 
	Each topic can also be an array of DATA with “or” options.
	[example]:
	
	  A transaction with a log with topics [A, B], 
	  will be matched by the following topic filters:
	    1. [] “anything”
	    2. [A] “A in first position (and anything after)”
	    3. [null, B] “anything in first position AND B in second position (and anything after)”
	    4. [A, B] “A in first position AND B in second position (and anything after)”
	    5. [[A, B], [A, B]] “(A OR B) in first position AND (A OR B) in second position (and anything after)”
	    
	source: https://eth.wiki/json-rpc/API#eth_newFilter
*/
export function filterLogsByTopics(
  logs: Log[],
  filterTopics: FilterTopic[]
): Log[] {
  // match anything
  if (filterTopics.length === 0) {
    return logs;
  }
  // match anything with required length
  if (filterTopics.every((t) => t === null)) {
    return logs.filter((log) => log.topics.length >= filterTopics.length);
  }

  let result: Log[] = [];
  for (let log of logs) {
    let topics = log.topics;
    let length = topics.length;
    let match = length >= filterTopics.length;
    for (let i of [...Array(length).keys()]) {
      if (
        filterTopics[i] &&
        typeof filterTopics[i] === "string" &&
        topics[i] !== filterTopics[i]
      ) {
        match = false;
        break;
      }
      if (
        filterTopics[i] &&
        Array.isArray(filterTopics[i]) &&
        !filterTopics[i]?.includes(topics[i])
      ) {
        match = false;
        break;
      }
    }
    if (!match) {
      continue;
    }
    result.push(log);
  }
  return result;
}

export function filterLogsByAddress(
  logs: Log[],
  _address: HexString | undefined
): Log[] {
  const address = normalizeLogQueryAddress(_address);
  // match anything
  if (!address) {
    return logs;
  }

  let result: Log[] = [];
  for (let log of logs) {
    if (log.address === address) {
      result.push(log);
    }
  }
  return result;
}

export function getDatabaseRateLimitingConfiguration() {
  const MAX_QUERY_NUMBER = envConfig["maxQueryNumber"]
    ? +envConfig["maxQueryNumber"]
    : DEFAULT_MAX_QUERY_NUMBER;
  const MAX_QUERY_TIME_MILSECS = envConfig["maxQueryTimeInMilliseconds"]
    ? +envConfig["maxQueryTimeInMilliseconds"]
    : DEFAULT_MAX_QUERY_TIME_MILSECS;

  return {
    MAX_QUERY_NUMBER,
    MAX_QUERY_TIME_MILSECS,
  };
}

export function buildQueryLogAddress(
  queryBuilder: KnexType.QueryBuilder,
  address: HexString | HexString[] | undefined
) {
  if (address && address.length !== 0) {
    const queryAddress = Array.isArray(address) ? [...address] : [address];
    queryBuilder.whereIn(
      "address",
      queryAddress.map((addr) => hexToBuffer(addr))
    );
  }
}

/*
return a slice of log array which satisfy the topics matching.
matching rule:
      Topics are order-dependent.
	Each topic can also be an array of DATA with “or” options.
	[example]:

	  A transaction with a log with topics [A, B],
	  will be matched by the following topic filters:
	    1. [] “anything”
	    2. [A] “A in first position (and anything after)”
	    3. [null, B] “anything in first position AND B in second position (and anything after)”
	    4. [A, B] “A in first position AND B in second position (and anything after)”
	    5. [[A, B], [A, B]] “(A OR B) in first position AND (A OR B) in second position (and anything after)”

	source: https://eth.wiki/json-rpc/API#eth_newFilter
*/
export function buildQueryLogTopics(
  queryBuilder: KnexType.QueryBuilder,
  topics: FilterTopic[]
) {
  if (topics.length !== 0) {
    queryBuilder.whereRaw(`array_length(topics, 1) >= ?`, [topics.length]);
  }

  topics.forEach((topic, index) => {
    if (topic == null) {
      // discard always-matched topic
    } else if (typeof topic === "string") {
      const pgTopicIndex = index + 1;
      queryBuilder.where(`topics[${pgTopicIndex}]`, "=", hexToBuffer(topic));
    } else {
      const pgTopicIndex = index + 1;
      queryBuilder.whereIn(
        `topics[${pgTopicIndex}]`,
        topic.map((subtopic) => hexToBuffer(subtopic))
      );
    }
  });
}

export function buildQueryLogBlock(
  queryBuilder: KnexType.QueryBuilder,
  fromBlock: bigint,
  toBlock: bigint,
  blockHash?: Hash
) {
  if (blockHash != null) {
    queryBuilder.where("block_hash", hexToBuffer(blockHash));
  } else {
    queryBuilder.whereBetween("block_number", [
      fromBlock.toString(),
      toBlock.toString(),
    ]);
  }
}

export function buildQueryLogId(
  queryBuilder: KnexType.QueryBuilder,
  lastPollId: bigint = BigInt(-1)
) {
  if (lastPollId !== BigInt(-1)) {
    queryBuilder.where("id", ">", lastPollId.toString());
  }
}

// chainId = 0 means non-EIP155 tx
// v = v(0/1) + chainId * 2 + 35 OR v = v(0/1) + 27
export function getRealV(v: bigint, chainId?: bigint): bigint {
  if (![0n, 1n].includes(v)) {
    throw new Error("V value must be 0 / 1");
  }
  return v + (chainId == null || chainId === 0n ? 27n : chainId * 2n + 35n);
}
