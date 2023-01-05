import test from "ava";
import { JSONResponse, client } from "../www";
import { EthBlock, EthTransaction } from "../../src/base/types/api";

test.before(async (t) => {
  const block: EthBlock =
    (await findNonEmptyBlock()) || (await getGenesisBlock());
  t.context = block;
});

test("eth_protocolVersion", async (t) => {
  const res: JSONResponse = await client.request(t.title, []);
  t.falsy(res.error);
  t.is(res.result, "0x41");
});

test("eth_blockNumber", async (t) => {
  const res: JSONResponse = await client.request(t.title, []);
  t.falsy(res.error);
});

test("eth_getBlockByNumber", async (t) => {
  const block = t.context as EthBlock;
  const res: JSONResponse = await client.request(t.title, [block.number, true]);
  t.falsy(res.error);
  t.is(res.result.number, block.number);
});

test("eth_getBlockByHash", async (t) => {
  const block = t.context as EthBlock;
  const res: JSONResponse = await client.request(t.title, [block.hash, true]);
  t.falsy(res.error);
  t.is(res.result.number, block.number);
  t.deepEqual(res.result, block);
});

test("eth_getBlockTransactionCountByNumber", async (t) => {
  const block = t.context as EthBlock;
  const res: JSONResponse = await client.request(t.title, [block.number]);
  t.falsy(res.error);
  t.is(parseInt(res.result, 16), block.transactions.length);
});

test("eth_getBlockTransactionCountByHash", async (t) => {
  const block = t.context as EthBlock;
  const res: JSONResponse = await client.request(t.title, [block.hash]);
  t.falsy(res.error);
  t.is(parseInt(res.result, 16), block.transactions.length);
});

test("eth_getTransactionByBlockNumberAndIndex", async (t) => {
  const block = t.context as EthBlock;
  if (block.transactions.length === 0) {
    t.pass();
    return;
  }

  const res: JSONResponse = await client.request(t.title, [
    block.number,
    "0x0",
  ]);
  t.falsy(res.error);
  t.deepEqual(res.result, block.transactions[0]);
});

test("eth_getTransactionByBlockHashAndIndex", async (t) => {
  const block = t.context as EthBlock;
  if (block.transactions.length === 0) {
    t.pass();
    return;
  }

  const res: JSONResponse = await client.request(t.title, [block.hash, "0x0"]);
  t.falsy(res.error);
  t.deepEqual(res.result, block.transactions[0]);
});

test("eth_getTransactionByHash", async (t) => {
  const block = t.context as EthBlock;
  if (block.transactions.length === 0) {
    t.pass();
    return;
  }

  const res: JSONResponse = await client.request(t.title, [
    (block.transactions[0] as EthTransaction).hash,
  ]);
  t.falsy(res.error);
  t.deepEqual(res.result, block.transactions[0]);
});

test("eth_getTransactionReceipt", async (t) => {
  const block = t.context as EthBlock;
  if (block.transactions.length === 0) {
    t.pass();
    return;
  }

  const res: JSONResponse = await client.request(t.title, [
    (block.transactions[0] as EthTransaction).hash,
  ]);
  t.falsy(res.error);
  t.is(
    res.result.transactionHash,
    (block.transactions[0] as EthTransaction).hash
  );
});

test("eth_coinbase", async (t) => {
  const res: JSONResponse = await client.request(t.title, []);
  t.falsy(res.error);
  t.is(res.result, "0x0000000000000000000000000000000000000000");
});

test("eth_hashrate", async (t) => {
  const res: JSONResponse = await client.request(t.title, []);
  t.falsy(res.error);
  t.is(res.result, "0x0");
});

test("eth_accounts", async (t) => {
  const res: JSONResponse = await client.request(t.title, []);
  t.falsy(res.error);
  t.deepEqual(res.result, []);
});

test("eth_syncing", async (t) => {
  const res: JSONResponse = await client.request(t.title, []);
  t.falsy(res.error);
});

async function findNonEmptyBlock(): Promise<EthBlock | undefined> {
  let number = 0;
  while (true) {
    let res: JSONResponse = await client.request("eth_getBlockByNumber", [
      "0x" + number.toString(16),
      true,
    ]);
    if (!res.result) {
      break;
    }

    let block: EthBlock = res.result!;
    if (block.transactions.length > 0) {
      return block;
    }

    number++;
  }

  console.warn("There is no non-empty block.");
  return undefined;
}

async function getGenesisBlock(): Promise<EthBlock> {
  const res: JSONResponse = await client.request("eth_getBlockByNumber", [
    "0x0",
    true,
  ]);
  return res.result!;
}
