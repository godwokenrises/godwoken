import test from "ava";
import {
  formatDecimal,
  filterLogsByTopics,
  buildQueryLogTopics,
} from "../../src/db/helpers";
import { Log } from "../../src/db/types";
import knex from "knex";
import { FilterTopic } from "../../src/base/filter";

test("formatDecimal", (t) => {
  const testCase: { [key: string]: bigint } = {
    "1000": 1000n,
    "1000.01": 1001n,
    "1569.00": 1569n,
    "1433.0": 1433n,
  };

  for (const [key, value] of Object.entries(testCase)) {
    const result = formatDecimal(key);
    t.is(result, value);
  }
});
test("buildQueryLogTopics", async (t) => {
  const buildQuery = (topics: FilterTopic[]): string => {
    let queryBuilder = knex({ client: "pg" }).queryBuilder();
    buildQueryLogTopics(queryBuilder, topics);
    return queryBuilder.toQuery();
  };
  t.is(buildQuery([]), "select *");
  t.is(
    buildQuery(["0xaaaaaa"]),
    `select * where array_length(topics, 1) >= 1 and "topics"[1] = X'aaaaaa'`
  );
  t.is(
    buildQuery([null, "0xaaaaaa"]),
    `select * where array_length(topics, 1) >= 2 and "topics"[2] = X'aaaaaa'`
  );
  t.is(
    buildQuery(["0xaaaaaa", null]),
    `select * where array_length(topics, 1) >= 2 and "topics"[1] = X'aaaaaa'`
  );
  t.is(
    buildQuery(["0xaaaaaa", "0xbbbbbb"]),
    `select * where array_length(topics, 1) >= 2 and "topics"[1] = X'aaaaaa' and "topics"[2] = X'bbbbbb'`
  );
  t.is(
    buildQuery([
      ["0xaaaaaa", "0xbbbbbb"],
      ["0xaaaaaa", "0xbbbbbb", "0xcccccc"],
    ]),
    `select * where array_length(topics, 1) >= 2 and "topics"[1] in (X'aaaaaa', X'bbbbbb') and "topics"[2] in (X'aaaaaa', X'bbbbbb', X'cccccc')`
  );
});

test("match topics", async (t) => {
  const logs: Log[] = [
    {
      id: BigInt(0),
      transaction_hash: "",
      transaction_id: BigInt(0),
      transaction_index: 0,
      block_number: BigInt(0),
      block_hash: "",
      address: "",
      data: "",
      log_index: 0,
      topics: ["a"],
    },
    {
      id: BigInt(0),
      transaction_hash: "",
      transaction_id: BigInt(0),
      transaction_index: 0,
      block_number: BigInt(0),
      block_hash: "",
      address: "",
      data: "",
      log_index: 0,
      topics: ["a", "b"],
    },
    {
      id: BigInt(0),
      transaction_hash: "",
      transaction_id: BigInt(0),
      transaction_index: 0,
      block_number: BigInt(0),
      block_hash: "",
      address: "",
      data: "",
      log_index: 0,
      topics: ["c", "b"],
    },
    {
      id: BigInt(0),
      transaction_hash: "",
      transaction_id: BigInt(0),
      transaction_index: 0,
      block_number: BigInt(0),
      block_hash: "",
      address: "",
      data: "",
      log_index: 0,
      topics: ["b"],
    },
  ];

  const f0: FilterTopic[] = [null, null, null];
  const f1: FilterTopic[] = [];
  const f2: FilterTopic[] = ["a"];
  const f3: FilterTopic[] = [null, "b"];
  const f4: FilterTopic[] = ["a", "b"];
  const f5: FilterTopic[] = [
    ["a", "b"],
    ["a", "b"],
  ];
  const f6: FilterTopic[] = [["a", "c"]];

  t.deepEqual([], filterLogsByTopics(logs, f0));
  t.deepEqual(logs, filterLogsByTopics(logs, f1));
  t.deepEqual(logs.slice(0, 2), filterLogsByTopics(logs, f2));
  t.deepEqual(logs.slice(1, 3), filterLogsByTopics(logs, f3));
  t.deepEqual(logs.slice(1, 2), filterLogsByTopics(logs, f4));
  t.deepEqual(logs.slice(1, 2), filterLogsByTopics(logs, f5));
  t.deepEqual(logs.slice(0, 3), filterLogsByTopics(logs, f6));
});

test("match for empty topics", async (t) => {
  const logs: Log[] = [
    {
      id: BigInt(0),
      transaction_hash: "",
      transaction_id: BigInt(0),
      transaction_index: 0,
      block_number: BigInt(0),
      block_hash: "",
      address: "",
      data: "",
      log_index: 0,
      topics: [],
    },
  ];

  const f1: FilterTopic = [];
  const f2: FilterTopic = [
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
  ];

  // since [] will return anything, so empty topic logs should return as well
  t.deepEqual(logs, filterLogsByTopics(logs, f1));
  t.deepEqual([], filterLogsByTopics(logs, f2));
});
