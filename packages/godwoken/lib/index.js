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
}

module.exports = { ChainService };
