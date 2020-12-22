var addon = require("../native");

class ChainService {
  constructor(config) {
    this.config = config;
    this.nativeChain = new addon.NativeChain(JSON.stringify(config));
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

  getStorageAt(rawKey) {
    this.nativeChain.getStorageAt(rawKey);
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

module.exports = { ChainService };
