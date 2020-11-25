const test = require("ava");
const fs = require("fs");
const path = require("path");
const { Chain } = require("../lib");
const { Indexer, TransactionCollector } = require("@ckb-lumos/indexer");
const { depositionLockScript, depositionTransaction0, depositionTransaction1, depositionTransaction2 } = require("./test_cases.js")
const configPath = path.join(__dirname, "..", "config", "dev.config.json");

class MockTransactionCollector extends TransactionCollector {
  async *collect() {
    yield depositionTransaction0;
    yield depositionTransaction1;
    yield depositionTransaction2;
  }
}

test("Init a chain by config", (t) => {
  let chain = new Chain(configPath);
  t.pass();
});

test("Sync rollup data from CKB network", async (t) => {
  let chain = new Chain(configPath);
  const indexer = new Indexer("mockUri", "mockDataPath");
  const queryOption = {
    lock: depositionLockScript,
  };
  const txCollector = new MockTransactionCollector(indexer, queryOption);
  let depositionTransactions = [];
  for await (const tx of txCollector.collect()) {
    depositionTransactions.push(tx);
  }


  chain.sync();

  t.pass();
});
