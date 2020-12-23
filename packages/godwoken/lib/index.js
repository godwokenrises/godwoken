var addon = require("../native");

function buildGenesisBlock(config) {
  return JSON.parse(addon.buildGenesisBlock(JSON.stringify(config)));
}

class ChainService {
  constructor(config, genesis) {
    this.config = config;
    this.nativeChain = new addon.NativeChain(
      JSON.stringify(config),
      JSON.stringify(genesis)
    );
  }

  async sync(syncParam) {
    const syncEventString = this.nativeChain.sync(JSON.stringify(syncParam));
    return JSON.parse(syncEventString);
  }

  async produceBlock(produceBlockParam) {
    const produceBlockResult = this.nativeChain.produceBlock(
      JSON.stringify(produceBlockParam)
    );
    return JSON.parse(produceBlockResult);
  }
  async execute(l2Transaction) {
    const runResult = this.nativeChain.execute(l2Transaction);
    return JSON.parse(runResult);
  }

  async submitL2Block(l2Transaction) {
    const runResult = this.nativeChain.submitL2Block(l2Transaction);
    return JSON.parse(runResult);
  }

  async getStorageAt(rawKey) {
    return JSON.parse(this.nativeChain.getStorageAt(rawKey));
  }

  tip() {
    return this.nativeChain.tip();
  }

  lastSynced() {
    return this.nativeChain.lastSynced();
  }

  status() {
    return this.nativeChain.status();
  }

  config() {
    return this.config;
  }
}

module.exports = { ChainService, buildGenesisBlock };
