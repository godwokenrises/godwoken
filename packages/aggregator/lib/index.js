const addon = require("../native");

const fs = require("fs");

function defaultLogger(level, message) {
  console.log(`[${level}] ${message}`);
}

class Chain {
  construct(
    configPath,
    indexer,
    {
      pollIntervalSeconds = 2,
      livenessCheckIntervalSeconds = 5,
      logger = defaultLogger,
    } = {}
  ) {
    this.indexer = indexer;
    this.livenessCheckIntervalSeconds = livenessCheckIntervalSeconds;
    this.logger = logger;
    this.config = parseConfig(configPath);
    this.nativeChain = new addon.NativeChain(configPath);
  }

  start() {
    this.nativeChain.start_rpc_server();
  }

  startForever() {
    this.nativeChain.start();
    setInterval(() => {
      if (!this.nativeChain.running()) {
        this.logger("error", "Native chain has stopped, maybe check the log?");
        this.nativeChain.start();
      }
    }, this.livenessCheckIntervalSeconds * 1000);
  }

  stop() {
    this.nativeChain.stop();
  }

  // Sync Rollup related data from CKB network:
  // 1. L1->L2 user deposition transaction
  // 2. L1 aggregator deposition-collect transaction
  // 3. L2->L1 aggregator withdraw transaction
  // 4. L2->L1 user force-withdraw transaction
  // 5. L2 aggregator submit-block transaction
  // 6. L2 aggregator rever-block transaction(challenge with fraud proof)
  sync() {
    this.nativeChain.sync();
  }

  //
  produce_block() {}
}

function parseConfig(configPath) {
  let rawData = fs.readFileSync(configPath);
  return JSON.parse(rawData);
}

module.exports = { Chain };
