(function (global, factory) {
  typeof exports === 'object' && typeof module !== 'undefined' ? factory(exports) :
  typeof define === 'function' && define.amd ? define(['exports'], factory) :
  (global = typeof globalThis !== 'undefined' ? globalThis : global || self, factory(global.Godwoken = {}));
})(this, (function (exports) { 'use strict';

  function dataLengthError(actual, required) {
      throw new Error(`Invalid data length! Required: ${required}, actual: ${actual}`);
  }

  function assertDataLength(actual, required) {
    if (actual !== required) {
      dataLengthError(actual, required);
    }
  }

  function assertArrayBuffer(reader) {
    if (reader instanceof Object && reader.toArrayBuffer instanceof Function) {
      reader = reader.toArrayBuffer();
    }
    if (!(reader instanceof ArrayBuffer)) {
      throw new Error("Provided value must be an ArrayBuffer or can be transformed into ArrayBuffer!");
    }
    return reader;
  }

  function verifyAndExtractOffsets(view, expectedFieldCount, compatible) {
    if (view.byteLength < 4) {
      dataLengthError(view.byteLength, ">4");
    }
    const requiredByteLength = view.getUint32(0, true);
    assertDataLength(view.byteLength, requiredByteLength);
    if (requiredByteLength === 4) {
      return [requiredByteLength];
    }
    if (requiredByteLength < 8) {
      dataLengthError(view.byteLength, ">8");
    }
    const firstOffset = view.getUint32(4, true);
    if (firstOffset % 4 !== 0 || firstOffset < 8) {
      throw new Error(`Invalid first offset: ${firstOffset}`);
    }
    const itemCount = firstOffset / 4 - 1;
    if (itemCount < expectedFieldCount) {
      throw new Error(`Item count not enough! Required: ${expectedFieldCount}, actual: ${itemCount}`);
    } else if ((!compatible) && itemCount > expectedFieldCount) {
      throw new Error(`Item count is more than required! Required: ${expectedFieldCount}, actual: ${itemCount}`);
    }
    if (requiredByteLength < firstOffset) {
      throw new Error(`First offset is larger than byte length: ${firstOffset}`);
    }
    const offsets = [];
    for (let i = 0; i < itemCount; i++) {
      const start = 4 + i * 4;
      offsets.push(view.getUint32(start, true));
    }
    offsets.push(requiredByteLength);
    for (let i = 0; i < offsets.length - 1; i++) {
      if (offsets[i] > offsets[i + 1]) {
        throw new Error(`Offset index ${i}: ${offsets[i]} is larger than offset index ${i + 1}: ${offsets[i + 1]}`);
      }
    }
    return offsets;
  }

  function serializeTable(buffers) {
    const itemCount = buffers.length;
    let totalSize = 4 * (itemCount + 1);
    const offsets = [];

    for (let i = 0; i < itemCount; i++) {
      offsets.push(totalSize);
      totalSize += buffers[i].byteLength;
    }

    const buffer = new ArrayBuffer(totalSize);
    const array = new Uint8Array(buffer);
    const view = new DataView(buffer);

    view.setUint32(0, totalSize, true);
    for (let i = 0; i < itemCount; i++) {
      view.setUint32(4 + i * 4, offsets[i], true);
      array.set(new Uint8Array(buffers[i]), offsets[i]);
    }
    return buffer;
  }

  class Uint32Vec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        dataLengthError(this.view.byteLength, ">4");
      }
      const requiredByteLength = this.length() * Uint32.size() + 4;
      assertDataLength(this.view.byteLength, requiredByteLength);
      for (let i = 0; i < 0; i++) {
        const item = this.indexAt(i);
        item.validate(compatible);
      }
    }

    indexAt(i) {
      return new Uint32(this.view.buffer.slice(4 + i * Uint32.size(), 4 + (i + 1) * Uint32.size()), { validate: false });
    }

    length() {
      return this.view.getUint32(0, true);
    }
  }

  function SerializeUint32Vec(value) {
    const array = new Uint8Array(4 + Uint32.size() * value.length);
    (new DataView(array.buffer)).setUint32(0, value.length, true);
    for (let i = 0; i < value.length; i++) {
      const itemBuffer = SerializeUint32(value[i]);
      array.set(new Uint8Array(itemBuffer), 4 + i * Uint32.size());
    }
    return array.buffer;
  }

  class BlockMerkleState {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getMerkleRoot() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getCount() {
      return new Uint64(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Uint64.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, BlockMerkleState.size());
      this.getMerkleRoot().validate(compatible);
      this.getCount().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + Uint64.size();
    }
  }

  function SerializeBlockMerkleState(value) {
    const array = new Uint8Array(0 + Byte32.size() + Uint64.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.merkle_root)), 0);
    array.set(new Uint8Array(SerializeUint64(value.count)), 0 + Byte32.size());
    return array.buffer;
  }

  class AccountMerkleState {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getMerkleRoot() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getCount() {
      return new Uint32(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Uint32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, AccountMerkleState.size());
      this.getMerkleRoot().validate(compatible);
      this.getCount().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + Uint32.size();
    }
  }

  function SerializeAccountMerkleState(value) {
    const array = new Uint8Array(0 + Byte32.size() + Uint32.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.merkle_root)), 0);
    array.set(new Uint8Array(SerializeUint32(value.count)), 0 + Byte32.size());
    return array.buffer;
  }

  class GlobalStateV0 {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getRollupConfigHash() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getAccount() {
      return new AccountMerkleState(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + AccountMerkleState.size()), { validate: false });
    }

    getBlock() {
      return new BlockMerkleState(this.view.buffer.slice(0 + Byte32.size() + AccountMerkleState.size(), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size()), { validate: false });
    }

    getRevertedBlockRoot() {
      return new Byte32(this.view.buffer.slice(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size(), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size()), { validate: false });
    }

    getTipBlockHash() {
      return new Byte32(this.view.buffer.slice(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size(), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size()), { validate: false });
    }

    getLastFinalizedBlockNumber() {
      return new Uint64(this.view.buffer.slice(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size(), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size()), { validate: false });
    }

    getStatus() {
      return this.view.getUint8(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size());
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, GlobalStateV0.size());
      this.getRollupConfigHash().validate(compatible);
      this.getAccount().validate(compatible);
      this.getBlock().validate(compatible);
      this.getRevertedBlockRoot().validate(compatible);
      this.getTipBlockHash().validate(compatible);
      this.getLastFinalizedBlockNumber().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size() + 1;
    }
  }

  function SerializeGlobalStateV0(value) {
    const array = new Uint8Array(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size() + 1);
    const view = new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.rollup_config_hash)), 0);
    array.set(new Uint8Array(SerializeAccountMerkleState(value.account)), 0 + Byte32.size());
    array.set(new Uint8Array(SerializeBlockMerkleState(value.block)), 0 + Byte32.size() + AccountMerkleState.size());
    array.set(new Uint8Array(SerializeByte32(value.reverted_block_root)), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size());
    array.set(new Uint8Array(SerializeByte32(value.tip_block_hash)), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size());
    array.set(new Uint8Array(SerializeUint64(value.last_finalized_block_number)), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size());
    view.setUint8(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size(), value.status);
    return array.buffer;
  }

  class GlobalState {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getRollupConfigHash() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getAccount() {
      return new AccountMerkleState(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + AccountMerkleState.size()), { validate: false });
    }

    getBlock() {
      return new BlockMerkleState(this.view.buffer.slice(0 + Byte32.size() + AccountMerkleState.size(), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size()), { validate: false });
    }

    getRevertedBlockRoot() {
      return new Byte32(this.view.buffer.slice(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size(), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size()), { validate: false });
    }

    getTipBlockHash() {
      return new Byte32(this.view.buffer.slice(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size(), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size()), { validate: false });
    }

    getTipBlockTimestamp() {
      return new Uint64(this.view.buffer.slice(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size(), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size()), { validate: false });
    }

    getLastFinalizedBlockNumber() {
      return new Uint64(this.view.buffer.slice(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size(), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size() + Uint64.size()), { validate: false });
    }

    getStatus() {
      return this.view.getUint8(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size() + Uint64.size());
    }

    getVersion() {
      return this.view.getUint8(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size() + Uint64.size() + 1);
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, GlobalState.size());
      this.getRollupConfigHash().validate(compatible);
      this.getAccount().validate(compatible);
      this.getBlock().validate(compatible);
      this.getRevertedBlockRoot().validate(compatible);
      this.getTipBlockHash().validate(compatible);
      this.getTipBlockTimestamp().validate(compatible);
      this.getLastFinalizedBlockNumber().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size() + Uint64.size() + 1 + 1;
    }
  }

  function SerializeGlobalState(value) {
    const array = new Uint8Array(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size() + Uint64.size() + 1 + 1);
    const view = new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.rollup_config_hash)), 0);
    array.set(new Uint8Array(SerializeAccountMerkleState(value.account)), 0 + Byte32.size());
    array.set(new Uint8Array(SerializeBlockMerkleState(value.block)), 0 + Byte32.size() + AccountMerkleState.size());
    array.set(new Uint8Array(SerializeByte32(value.reverted_block_root)), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size());
    array.set(new Uint8Array(SerializeByte32(value.tip_block_hash)), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size());
    array.set(new Uint8Array(SerializeUint64(value.tip_block_timestamp)), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size());
    array.set(new Uint8Array(SerializeUint64(value.last_finalized_block_number)), 0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size());
    view.setUint8(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size() + Uint64.size(), value.status);
    view.setUint8(0 + Byte32.size() + AccountMerkleState.size() + BlockMerkleState.size() + Byte32.size() + Byte32.size() + Uint64.size() + Uint64.size() + 1, value.version);
    return array.buffer;
  }

  class AllowedTypeHash {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getType() {
      return this.view.getUint8(0);
    }

    getHash() {
      return new Byte32(this.view.buffer.slice(0 + 1, 0 + 1 + Byte32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, AllowedTypeHash.size());
      this.getHash().validate(compatible);
    }
    static size() {
      return 0 + 1 + Byte32.size();
    }
  }

  function SerializeAllowedTypeHash(value) {
    const array = new Uint8Array(0 + 1 + Byte32.size());
    const view = new DataView(array.buffer);
    view.setUint8(0, value.type_);
    array.set(new Uint8Array(SerializeByte32(value.hash)), 0 + 1);
    return array.buffer;
  }

  class AllowedTypeHashVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        dataLengthError(this.view.byteLength, ">4");
      }
      const requiredByteLength = this.length() * AllowedTypeHash.size() + 4;
      assertDataLength(this.view.byteLength, requiredByteLength);
      for (let i = 0; i < 0; i++) {
        const item = this.indexAt(i);
        item.validate(compatible);
      }
    }

    indexAt(i) {
      return new AllowedTypeHash(this.view.buffer.slice(4 + i * AllowedTypeHash.size(), 4 + (i + 1) * AllowedTypeHash.size()), { validate: false });
    }

    length() {
      return this.view.getUint32(0, true);
    }
  }

  function SerializeAllowedTypeHashVec(value) {
    const array = new Uint8Array(4 + AllowedTypeHash.size() * value.length);
    (new DataView(array.buffer)).setUint32(0, value.length, true);
    for (let i = 0; i < value.length; i++) {
      const itemBuffer = SerializeAllowedTypeHash(value[i]);
      array.set(new Uint8Array(itemBuffer), 4 + i * AllowedTypeHash.size());
    }
    return array.buffer;
  }

  class RollupConfig {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Byte32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Byte32(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Byte32(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new Byte32(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
      new Byte32(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
      new Byte32(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
      new Byte32(this.view.buffer.slice(offsets[6], offsets[7]), { validate: false }).validate();
      new Byte32(this.view.buffer.slice(offsets[7], offsets[8]), { validate: false }).validate();
      new Uint64(this.view.buffer.slice(offsets[8], offsets[9]), { validate: false }).validate();
      new Uint64(this.view.buffer.slice(offsets[9], offsets[10]), { validate: false }).validate();
      new Uint64(this.view.buffer.slice(offsets[10], offsets[11]), { validate: false }).validate();
      if (offsets[12] - offsets[11] !== 1) {
        throw new Error(`Invalid offset for reward_burn_rate: ${offsets[11]} - ${offsets[12]}`)
      }
      new Uint64(this.view.buffer.slice(offsets[12], offsets[13]), { validate: false }).validate();
      new AllowedTypeHashVec(this.view.buffer.slice(offsets[13], offsets[14]), { validate: false }).validate();
      new AllowedTypeHashVec(this.view.buffer.slice(offsets[14], offsets[15]), { validate: false }).validate();
    }

    getL1SudtScriptTypeHash() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getCustodianScriptTypeHash() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getDepositScriptTypeHash() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getWithdrawalScriptTypeHash() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getChallengeScriptTypeHash() {
      const start = 20;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getStakeScriptTypeHash() {
      const start = 24;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getL2SudtValidatorScriptTypeHash() {
      const start = 28;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getBurnLockHash() {
      const start = 32;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getRequiredStakingCapacity() {
      const start = 36;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getChallengeMaturityBlocks() {
      const start = 40;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getFinalityBlocks() {
      const start = 44;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getRewardBurnRate() {
      const start = 48;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new DataView(this.view.buffer.slice(offset, offset_end)).getUint8(0);
    }

    getChainId() {
      const start = 52;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getAllowedEoaTypeHashes() {
      const start = 56;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new AllowedTypeHashVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getAllowedContractTypeHashes() {
      const start = 60;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new AllowedTypeHashVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeRollupConfig(value) {
    const buffers = [];
    buffers.push(SerializeByte32(value.l1_sudt_script_type_hash));
    buffers.push(SerializeByte32(value.custodian_script_type_hash));
    buffers.push(SerializeByte32(value.deposit_script_type_hash));
    buffers.push(SerializeByte32(value.withdrawal_script_type_hash));
    buffers.push(SerializeByte32(value.challenge_script_type_hash));
    buffers.push(SerializeByte32(value.stake_script_type_hash));
    buffers.push(SerializeByte32(value.l2_sudt_validator_script_type_hash));
    buffers.push(SerializeByte32(value.burn_lock_hash));
    buffers.push(SerializeUint64(value.required_staking_capacity));
    buffers.push(SerializeUint64(value.challenge_maturity_blocks));
    buffers.push(SerializeUint64(value.finality_blocks));
    const rewardBurnRateView = new DataView(new ArrayBuffer(1));
    rewardBurnRateView.setUint8(0, value.reward_burn_rate);
    buffers.push(rewardBurnRateView.buffer);
    buffers.push(SerializeUint64(value.chain_id));
    buffers.push(SerializeAllowedTypeHashVec(value.allowed_eoa_type_hashes));
    buffers.push(SerializeAllowedTypeHashVec(value.allowed_contract_type_hashes));
    return serializeTable(buffers);
  }

  class RawL2Transaction {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Uint64(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Uint32(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Uint32(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new Uint32(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
    }

    getChainId() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getFromId() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getToId() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getNonce() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getArgs() {
      const start = 20;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeRawL2Transaction(value) {
    const buffers = [];
    buffers.push(SerializeUint64(value.chain_id));
    buffers.push(SerializeUint32(value.from_id));
    buffers.push(SerializeUint32(value.to_id));
    buffers.push(SerializeUint32(value.nonce));
    buffers.push(SerializeBytes(value.args));
    return serializeTable(buffers);
  }

  class L2Transaction {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new RawL2Transaction(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    }

    getRaw() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new RawL2Transaction(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getSignature() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeL2Transaction(value) {
    const buffers = [];
    buffers.push(SerializeRawL2Transaction(value.raw));
    buffers.push(SerializeBytes(value.signature));
    return serializeTable(buffers);
  }

  class L2TransactionVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      for (let i = 0; i < offsets.length - 1; i++) {
        new L2Transaction(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
      }
    }

    length() {
      if (this.view.byteLength < 8) {
        return 0;
      } else {
        return this.view.getUint32(4, true) / 4 - 1;
      }
    }

    indexAt(i) {
      const start = 4 + i * 4;
      const offset = this.view.getUint32(start, true);
      let offset_end = this.view.byteLength;
      if (i + 1 < this.length()) {
        offset_end = this.view.getUint32(start + 4, true);
      }
      return new L2Transaction(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeL2TransactionVec(value) {
    return serializeTable(value.map(item => SerializeL2Transaction(item)));
  }

  class SubmitTransactions {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getTxWitnessRoot() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getTxCount() {
      return new Uint32(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Uint32.size()), { validate: false });
    }

    getPrevStateCheckpoint() {
      return new Byte32(this.view.buffer.slice(0 + Byte32.size() + Uint32.size(), 0 + Byte32.size() + Uint32.size() + Byte32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, SubmitTransactions.size());
      this.getTxWitnessRoot().validate(compatible);
      this.getTxCount().validate(compatible);
      this.getPrevStateCheckpoint().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + Uint32.size() + Byte32.size();
    }
  }

  function SerializeSubmitTransactions(value) {
    const array = new Uint8Array(0 + Byte32.size() + Uint32.size() + Byte32.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.tx_witness_root)), 0);
    array.set(new Uint8Array(SerializeUint32(value.tx_count)), 0 + Byte32.size());
    array.set(new Uint8Array(SerializeByte32(value.prev_state_checkpoint)), 0 + Byte32.size() + Uint32.size());
    return array.buffer;
  }

  class SubmitWithdrawals {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getWithdrawalWitnessRoot() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getWithdrawalCount() {
      return new Uint32(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Uint32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, SubmitWithdrawals.size());
      this.getWithdrawalWitnessRoot().validate(compatible);
      this.getWithdrawalCount().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + Uint32.size();
    }
  }

  function SerializeSubmitWithdrawals(value) {
    const array = new Uint8Array(0 + Byte32.size() + Uint32.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.withdrawal_witness_root)), 0);
    array.set(new Uint8Array(SerializeUint32(value.withdrawal_count)), 0 + Byte32.size());
    return array.buffer;
  }

  class RawL2Block {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Uint64(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Byte32(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new Byte32(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
      new Uint64(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
      new AccountMerkleState(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
      new AccountMerkleState(this.view.buffer.slice(offsets[6], offsets[7]), { validate: false }).validate();
      new Byte32Vec(this.view.buffer.slice(offsets[7], offsets[8]), { validate: false }).validate();
      new SubmitWithdrawals(this.view.buffer.slice(offsets[8], offsets[9]), { validate: false }).validate();
      new SubmitTransactions(this.view.buffer.slice(offsets[9], offsets[10]), { validate: false }).validate();
    }

    getNumber() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getBlockProducer() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getParentBlockHash() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getStakeCellOwnerLockHash() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getTimestamp() {
      const start = 20;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getPrevAccount() {
      const start = 24;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new AccountMerkleState(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getPostAccount() {
      const start = 28;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new AccountMerkleState(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getStateCheckpointList() {
      const start = 32;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getSubmitWithdrawals() {
      const start = 36;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new SubmitWithdrawals(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getSubmitTransactions() {
      const start = 40;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new SubmitTransactions(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeRawL2Block(value) {
    const buffers = [];
    buffers.push(SerializeUint64(value.number));
    buffers.push(SerializeBytes(value.block_producer));
    buffers.push(SerializeByte32(value.parent_block_hash));
    buffers.push(SerializeByte32(value.stake_cell_owner_lock_hash));
    buffers.push(SerializeUint64(value.timestamp));
    buffers.push(SerializeAccountMerkleState(value.prev_account));
    buffers.push(SerializeAccountMerkleState(value.post_account));
    buffers.push(SerializeByte32Vec(value.state_checkpoint_list));
    buffers.push(SerializeSubmitWithdrawals(value.submit_withdrawals));
    buffers.push(SerializeSubmitTransactions(value.submit_transactions));
    return serializeTable(buffers);
  }

  class RawL2BlockVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      for (let i = 0; i < offsets.length - 1; i++) {
        new RawL2Block(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
      }
    }

    length() {
      if (this.view.byteLength < 8) {
        return 0;
      } else {
        return this.view.getUint32(4, true) / 4 - 1;
      }
    }

    indexAt(i) {
      const start = 4 + i * 4;
      const offset = this.view.getUint32(start, true);
      let offset_end = this.view.byteLength;
      if (i + 1 < this.length()) {
        offset_end = this.view.getUint32(start + 4, true);
      }
      return new RawL2Block(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeRawL2BlockVec(value) {
    return serializeTable(value.map(item => SerializeRawL2Block(item)));
  }

  class L2Block {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new RawL2Block(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new KVPairVec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new L2TransactionVec(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
      new WithdrawalRequestVec(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
    }

    getRaw() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new RawL2Block(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getKvState() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new KVPairVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getKvStateProof() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getTransactions() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new L2TransactionVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getBlockProof() {
      const start = 20;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getWithdrawals() {
      const start = 24;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new WithdrawalRequestVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeL2Block(value) {
    const buffers = [];
    buffers.push(SerializeRawL2Block(value.raw));
    buffers.push(SerializeKVPairVec(value.kv_state));
    buffers.push(SerializeBytes(value.kv_state_proof));
    buffers.push(SerializeL2TransactionVec(value.transactions));
    buffers.push(SerializeBytes(value.block_proof));
    buffers.push(SerializeWithdrawalRequestVec(value.withdrawals));
    return serializeTable(buffers);
  }

  class DepositRequest {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Uint64(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Uint128(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Byte32(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new Script(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
      new Uint32(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
    }

    getCapacity() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getAmount() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint128(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getSudtScriptHash() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getScript() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getRegistryId() {
      const start = 20;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeDepositRequest(value) {
    const buffers = [];
    buffers.push(SerializeUint64(value.capacity));
    buffers.push(SerializeUint128(value.amount));
    buffers.push(SerializeByte32(value.sudt_script_hash));
    buffers.push(SerializeScript(value.script));
    buffers.push(SerializeUint32(value.registry_id));
    return serializeTable(buffers);
  }

  class DepositRequestVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      for (let i = 0; i < offsets.length - 1; i++) {
        new DepositRequest(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
      }
    }

    length() {
      if (this.view.byteLength < 8) {
        return 0;
      } else {
        return this.view.getUint32(4, true) / 4 - 1;
      }
    }

    indexAt(i) {
      const start = 4 + i * 4;
      const offset = this.view.getUint32(start, true);
      let offset_end = this.view.byteLength;
      if (i + 1 < this.length()) {
        offset_end = this.view.getUint32(start + 4, true);
      }
      return new DepositRequest(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeDepositRequestVec(value) {
    return serializeTable(value.map(item => SerializeDepositRequest(item)));
  }

  class RawWithdrawalRequest {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getNonce() {
      return new Uint32(this.view.buffer.slice(0, 0 + Uint32.size()), { validate: false });
    }

    getChainId() {
      return new Uint64(this.view.buffer.slice(0 + Uint32.size(), 0 + Uint32.size() + Uint64.size()), { validate: false });
    }

    getCapacity() {
      return new Uint64(this.view.buffer.slice(0 + Uint32.size() + Uint64.size(), 0 + Uint32.size() + Uint64.size() + Uint64.size()), { validate: false });
    }

    getAmount() {
      return new Uint128(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint64.size(), 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size()), { validate: false });
    }

    getSudtScriptHash() {
      return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size(), 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size()), { validate: false });
    }

    getAccountScriptHash() {
      return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size(), 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size()), { validate: false });
    }

    getRegistryId() {
      return new Uint32(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size(), 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint32.size()), { validate: false });
    }

    getOwnerLockHash() {
      return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint32.size(), 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint32.size() + Byte32.size()), { validate: false });
    }

    getFee() {
      return new Uint128(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint32.size() + Byte32.size(), 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint32.size() + Byte32.size() + Uint128.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, RawWithdrawalRequest.size());
      this.getNonce().validate(compatible);
      this.getChainId().validate(compatible);
      this.getCapacity().validate(compatible);
      this.getAmount().validate(compatible);
      this.getSudtScriptHash().validate(compatible);
      this.getAccountScriptHash().validate(compatible);
      this.getRegistryId().validate(compatible);
      this.getOwnerLockHash().validate(compatible);
      this.getFee().validate(compatible);
    }
    static size() {
      return 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint32.size() + Byte32.size() + Uint128.size();
    }
  }

  function SerializeRawWithdrawalRequest(value) {
    const array = new Uint8Array(0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint32.size() + Byte32.size() + Uint128.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeUint32(value.nonce)), 0);
    array.set(new Uint8Array(SerializeUint64(value.chain_id)), 0 + Uint32.size());
    array.set(new Uint8Array(SerializeUint64(value.capacity)), 0 + Uint32.size() + Uint64.size());
    array.set(new Uint8Array(SerializeUint128(value.amount)), 0 + Uint32.size() + Uint64.size() + Uint64.size());
    array.set(new Uint8Array(SerializeByte32(value.sudt_script_hash)), 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size());
    array.set(new Uint8Array(SerializeByte32(value.account_script_hash)), 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size());
    array.set(new Uint8Array(SerializeUint32(value.registry_id)), 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size());
    array.set(new Uint8Array(SerializeByte32(value.owner_lock_hash)), 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint32.size());
    array.set(new Uint8Array(SerializeUint128(value.fee)), 0 + Uint32.size() + Uint64.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint32.size() + Byte32.size());
    return array.buffer;
  }

  class WithdrawalRequestVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      for (let i = 0; i < offsets.length - 1; i++) {
        new WithdrawalRequest(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
      }
    }

    length() {
      if (this.view.byteLength < 8) {
        return 0;
      } else {
        return this.view.getUint32(4, true) / 4 - 1;
      }
    }

    indexAt(i) {
      const start = 4 + i * 4;
      const offset = this.view.getUint32(start, true);
      let offset_end = this.view.byteLength;
      if (i + 1 < this.length()) {
        offset_end = this.view.getUint32(start + 4, true);
      }
      return new WithdrawalRequest(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeWithdrawalRequestVec(value) {
    return serializeTable(value.map(item => SerializeWithdrawalRequest(item)));
  }

  class WithdrawalRequest {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new RawWithdrawalRequest(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    }

    getRaw() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new RawWithdrawalRequest(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getSignature() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeWithdrawalRequest(value) {
    const buffers = [];
    buffers.push(SerializeRawWithdrawalRequest(value.raw));
    buffers.push(SerializeBytes(value.signature));
    return serializeTable(buffers);
  }

  class KVPair {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getK() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getV() {
      return new Byte32(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Byte32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, KVPair.size());
      this.getK().validate(compatible);
      this.getV().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + Byte32.size();
    }
  }

  function SerializeKVPair(value) {
    const array = new Uint8Array(0 + Byte32.size() + Byte32.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.k)), 0);
    array.set(new Uint8Array(SerializeByte32(value.v)), 0 + Byte32.size());
    return array.buffer;
  }

  class KVPairVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        dataLengthError(this.view.byteLength, ">4");
      }
      const requiredByteLength = this.length() * KVPair.size() + 4;
      assertDataLength(this.view.byteLength, requiredByteLength);
      for (let i = 0; i < 0; i++) {
        const item = this.indexAt(i);
        item.validate(compatible);
      }
    }

    indexAt(i) {
      return new KVPair(this.view.buffer.slice(4 + i * KVPair.size(), 4 + (i + 1) * KVPair.size()), { validate: false });
    }

    length() {
      return this.view.getUint32(0, true);
    }
  }

  function SerializeKVPairVec(value) {
    const array = new Uint8Array(4 + KVPair.size() * value.length);
    (new DataView(array.buffer)).setUint32(0, value.length, true);
    for (let i = 0; i < value.length; i++) {
      const itemBuffer = SerializeKVPair(value[i]);
      array.set(new Uint8Array(itemBuffer), 4 + i * KVPair.size());
    }
    return array.buffer;
  }

  class BlockInfo {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Bytes(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Uint64(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Uint64(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    }

    getBlockProducer() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getNumber() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getTimestamp() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeBlockInfo(value) {
    const buffers = [];
    buffers.push(SerializeBytes(value.block_producer));
    buffers.push(SerializeUint64(value.number));
    buffers.push(SerializeUint64(value.timestamp));
    return serializeTable(buffers);
  }

  class DepositLockArgs {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Byte32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Script(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Uint64(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new Uint32(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
    }

    getOwnerLockHash() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getLayer2Lock() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getCancelTimeout() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getRegistryId() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeDepositLockArgs(value) {
    const buffers = [];
    buffers.push(SerializeByte32(value.owner_lock_hash));
    buffers.push(SerializeScript(value.layer2_lock));
    buffers.push(SerializeUint64(value.cancel_timeout));
    buffers.push(SerializeUint32(value.registry_id));
    return serializeTable(buffers);
  }

  class CustodianLockArgs {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Byte32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Uint64(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new DepositLockArgs(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    }

    getDepositBlockHash() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getDepositBlockNumber() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getDepositLockArgs() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new DepositLockArgs(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeCustodianLockArgs(value) {
    const buffers = [];
    buffers.push(SerializeByte32(value.deposit_block_hash));
    buffers.push(SerializeUint64(value.deposit_block_number));
    buffers.push(SerializeDepositLockArgs(value.deposit_lock_args));
    return serializeTable(buffers);
  }

  class UnlockCustodianViaRevertWitness {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getDepositLockHash() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, UnlockCustodianViaRevertWitness.size());
      this.getDepositLockHash().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size();
    }
  }

  function SerializeUnlockCustodianViaRevertWitness(value) {
    const array = new Uint8Array(0 + Byte32.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.deposit_lock_hash)), 0);
    return array.buffer;
  }

  class WithdrawalLockArgs {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getWithdrawalBlockHash() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getWithdrawalBlockNumber() {
      return new Uint64(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Uint64.size()), { validate: false });
    }

    getAccountScriptHash() {
      return new Byte32(this.view.buffer.slice(0 + Byte32.size() + Uint64.size(), 0 + Byte32.size() + Uint64.size() + Byte32.size()), { validate: false });
    }

    getOwnerLockHash() {
      return new Byte32(this.view.buffer.slice(0 + Byte32.size() + Uint64.size() + Byte32.size(), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Byte32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, WithdrawalLockArgs.size());
      this.getWithdrawalBlockHash().validate(compatible);
      this.getWithdrawalBlockNumber().validate(compatible);
      this.getAccountScriptHash().validate(compatible);
      this.getOwnerLockHash().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + Uint64.size() + Byte32.size() + Byte32.size();
    }
  }

  function SerializeWithdrawalLockArgs(value) {
    const array = new Uint8Array(0 + Byte32.size() + Uint64.size() + Byte32.size() + Byte32.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.withdrawal_block_hash)), 0);
    array.set(new Uint8Array(SerializeUint64(value.withdrawal_block_number)), 0 + Byte32.size());
    array.set(new Uint8Array(SerializeByte32(value.account_script_hash)), 0 + Byte32.size() + Uint64.size());
    array.set(new Uint8Array(SerializeByte32(value.owner_lock_hash)), 0 + Byte32.size() + Uint64.size() + Byte32.size());
    return array.buffer;
  }

  class UnlockWithdrawalWitness {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        assertDataLength(this.view.byteLength, ">4");
      }
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        new UnlockWithdrawalViaFinalize(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      case 1:
        new UnlockWithdrawalViaRevert(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }

    unionType() {
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        return "UnlockWithdrawalViaFinalize";
      case 1:
        return "UnlockWithdrawalViaRevert";
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }

    value() {
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        return new UnlockWithdrawalViaFinalize(this.view.buffer.slice(4), { validate: false });
      case 1:
        return new UnlockWithdrawalViaRevert(this.view.buffer.slice(4), { validate: false });
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }
  }

  function SerializeUnlockWithdrawalWitness(value) {
    switch (value.type) {
    case "UnlockWithdrawalViaFinalize":
      {
        const itemBuffer = SerializeUnlockWithdrawalViaFinalize(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 0, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    case "UnlockWithdrawalViaRevert":
      {
        const itemBuffer = SerializeUnlockWithdrawalViaRevert(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 1, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    default:
      throw new Error(`Invalid type: ${value.type}`);
    }

  }

  class UnlockWithdrawalViaFinalize {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      verifyAndExtractOffsets(this.view, 0, true);
    }

  }

  function SerializeUnlockWithdrawalViaFinalize(value) {
    const buffers = [];
    return serializeTable(buffers);
  }

  class UnlockWithdrawalViaRevert {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getCustodianLockHash() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, UnlockWithdrawalViaRevert.size());
      this.getCustodianLockHash().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size();
    }
  }

  function SerializeUnlockWithdrawalViaRevert(value) {
    const array = new Uint8Array(0 + Byte32.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.custodian_lock_hash)), 0);
    return array.buffer;
  }

  class StakeLockArgs {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getOwnerLockHash() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getStakeBlockNumber() {
      return new Uint64(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Uint64.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, StakeLockArgs.size());
      this.getOwnerLockHash().validate(compatible);
      this.getStakeBlockNumber().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + Uint64.size();
    }
  }

  function SerializeStakeLockArgs(value) {
    const array = new Uint8Array(0 + Byte32.size() + Uint64.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.owner_lock_hash)), 0);
    array.set(new Uint8Array(SerializeUint64(value.stake_block_number)), 0 + Byte32.size());
    return array.buffer;
  }

  class MetaContractArgs {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        assertDataLength(this.view.byteLength, ">4");
      }
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        new CreateAccount(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      case 1:
        new BatchCreateEthAccounts(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }

    unionType() {
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        return "CreateAccount";
      case 1:
        return "BatchCreateEthAccounts";
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }

    value() {
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        return new CreateAccount(this.view.buffer.slice(4), { validate: false });
      case 1:
        return new BatchCreateEthAccounts(this.view.buffer.slice(4), { validate: false });
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }
  }

  function SerializeMetaContractArgs(value) {
    switch (value.type) {
    case "CreateAccount":
      {
        const itemBuffer = SerializeCreateAccount(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 0, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    case "BatchCreateEthAccounts":
      {
        const itemBuffer = SerializeBatchCreateEthAccounts(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 1, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    default:
      throw new Error(`Invalid type: ${value.type}`);
    }

  }

  class Fee {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getRegistryId() {
      return new Uint32(this.view.buffer.slice(0, 0 + Uint32.size()), { validate: false });
    }

    getAmount() {
      return new Uint128(this.view.buffer.slice(0 + Uint32.size(), 0 + Uint32.size() + Uint128.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, Fee.size());
      this.getRegistryId().validate(compatible);
      this.getAmount().validate(compatible);
    }
    static size() {
      return 0 + Uint32.size() + Uint128.size();
    }
  }

  function SerializeFee(value) {
    const array = new Uint8Array(0 + Uint32.size() + Uint128.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeUint32(value.registry_id)), 0);
    array.set(new Uint8Array(SerializeUint128(value.amount)), 0 + Uint32.size());
    return array.buffer;
  }

  class CreateAccount {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Script(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Fee(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    }

    getScript() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getFee() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Fee(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeCreateAccount(value) {
    const buffers = [];
    buffers.push(SerializeScript(value.script));
    buffers.push(SerializeFee(value.fee));
    return serializeTable(buffers);
  }

  class BatchCreateEthAccounts {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new ScriptVec(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Fee(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    }

    getScripts() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new ScriptVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getFee() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Fee(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeBatchCreateEthAccounts(value) {
    const buffers = [];
    buffers.push(SerializeScriptVec(value.scripts));
    buffers.push(SerializeFee(value.fee));
    return serializeTable(buffers);
  }

  class SUDTArgs {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        assertDataLength(this.view.byteLength, ">4");
      }
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        new SUDTQuery(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      case 1:
        new SUDTTransfer(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }

    unionType() {
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        return "SUDTQuery";
      case 1:
        return "SUDTTransfer";
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }

    value() {
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        return new SUDTQuery(this.view.buffer.slice(4), { validate: false });
      case 1:
        return new SUDTTransfer(this.view.buffer.slice(4), { validate: false });
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }
  }

  function SerializeSUDTArgs(value) {
    switch (value.type) {
    case "SUDTQuery":
      {
        const itemBuffer = SerializeSUDTQuery(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 0, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    case "SUDTTransfer":
      {
        const itemBuffer = SerializeSUDTTransfer(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 1, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    default:
      throw new Error(`Invalid type: ${value.type}`);
    }

  }

  class SUDTQuery {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Bytes(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    }

    getAddress() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeSUDTQuery(value) {
    const buffers = [];
    buffers.push(SerializeBytes(value.address));
    return serializeTable(buffers);
  }

  class SUDTTransfer {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Bytes(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Uint256(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Fee(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    }

    getToAddress() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getAmount() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint256(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getFee() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Fee(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeSUDTTransfer(value) {
    const buffers = [];
    buffers.push(SerializeBytes(value.to_address));
    buffers.push(SerializeUint256(value.amount));
    buffers.push(SerializeFee(value.fee));
    return serializeTable(buffers);
  }

  class ChallengeTarget {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getBlockHash() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getTargetIndex() {
      return new Uint32(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Uint32.size()), { validate: false });
    }

    getTargetType() {
      return this.view.getUint8(0 + Byte32.size() + Uint32.size());
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, ChallengeTarget.size());
      this.getBlockHash().validate(compatible);
      this.getTargetIndex().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + Uint32.size() + 1;
    }
  }

  function SerializeChallengeTarget(value) {
    const array = new Uint8Array(0 + Byte32.size() + Uint32.size() + 1);
    const view = new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.block_hash)), 0);
    array.set(new Uint8Array(SerializeUint32(value.target_index)), 0 + Byte32.size());
    view.setUint8(0 + Byte32.size() + Uint32.size(), value.target_type);
    return array.buffer;
  }

  class ChallengeLockArgs {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new ChallengeTarget(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Script(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    }

    getTarget() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new ChallengeTarget(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getRewardsReceiverLock() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeChallengeLockArgs(value) {
    const buffers = [];
    buffers.push(SerializeChallengeTarget(value.target));
    buffers.push(SerializeScript(value.rewards_receiver_lock));
    return serializeTable(buffers);
  }

  class ChallengeWitness {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new RawL2Block(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    }

    getRawL2Block() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new RawL2Block(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getBlockProof() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeChallengeWitness(value) {
    const buffers = [];
    buffers.push(SerializeRawL2Block(value.raw_l2block));
    buffers.push(SerializeBytes(value.block_proof));
    return serializeTable(buffers);
  }

  class ScriptVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      for (let i = 0; i < offsets.length - 1; i++) {
        new Script(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
      }
    }

    length() {
      if (this.view.byteLength < 8) {
        return 0;
      } else {
        return this.view.getUint32(4, true) / 4 - 1;
      }
    }

    indexAt(i) {
      const start = 4 + i * 4;
      const offset = this.view.getUint32(start, true);
      let offset_end = this.view.byteLength;
      if (i + 1 < this.length()) {
        offset_end = this.view.getUint32(start + 4, true);
      }
      return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeScriptVec(value) {
    return serializeTable(value.map(item => SerializeScript(item)));
  }

  class BlockHashEntry {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getNumber() {
      return new Uint64(this.view.buffer.slice(0, 0 + Uint64.size()), { validate: false });
    }

    getHash() {
      return new Byte32(this.view.buffer.slice(0 + Uint64.size(), 0 + Uint64.size() + Byte32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, BlockHashEntry.size());
      this.getNumber().validate(compatible);
      this.getHash().validate(compatible);
    }
    static size() {
      return 0 + Uint64.size() + Byte32.size();
    }
  }

  function SerializeBlockHashEntry(value) {
    const array = new Uint8Array(0 + Uint64.size() + Byte32.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeUint64(value.number)), 0);
    array.set(new Uint8Array(SerializeByte32(value.hash)), 0 + Uint64.size());
    return array.buffer;
  }

  class BlockHashEntryVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        dataLengthError(this.view.byteLength, ">4");
      }
      const requiredByteLength = this.length() * BlockHashEntry.size() + 4;
      assertDataLength(this.view.byteLength, requiredByteLength);
      for (let i = 0; i < 0; i++) {
        const item = this.indexAt(i);
        item.validate(compatible);
      }
    }

    indexAt(i) {
      return new BlockHashEntry(this.view.buffer.slice(4 + i * BlockHashEntry.size(), 4 + (i + 1) * BlockHashEntry.size()), { validate: false });
    }

    length() {
      return this.view.getUint32(0, true);
    }
  }

  function SerializeBlockHashEntryVec(value) {
    const array = new Uint8Array(4 + BlockHashEntry.size() * value.length);
    (new DataView(array.buffer)).setUint32(0, value.length, true);
    for (let i = 0; i < value.length; i++) {
      const itemBuffer = SerializeBlockHashEntry(value[i]);
      array.set(new Uint8Array(itemBuffer), 4 + i * BlockHashEntry.size());
    }
    return array.buffer;
  }

  class CKBMerkleProof {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Uint32Vec(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Byte32Vec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    }

    getIndices() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getLemmas() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeCKBMerkleProof(value) {
    const buffers = [];
    buffers.push(SerializeUint32Vec(value.indices));
    buffers.push(SerializeByte32Vec(value.lemmas));
    return serializeTable(buffers);
  }

  class CCTransactionWitness {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new L2Transaction(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new RawL2Block(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new CKBMerkleProof(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
      new Uint32(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
      new KVPairVec(this.view.buffer.slice(offsets[6], offsets[7]), { validate: false }).validate();
      new BytesVec(this.view.buffer.slice(offsets[7], offsets[8]), { validate: false }).validate();
      new ScriptVec(this.view.buffer.slice(offsets[8], offsets[9]), { validate: false }).validate();
      new Byte32(this.view.buffer.slice(offsets[9], offsets[10]), { validate: false }).validate();
      new BlockHashEntryVec(this.view.buffer.slice(offsets[10], offsets[11]), { validate: false }).validate();
    }

    getL2Tx() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new L2Transaction(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getRawL2Block() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new RawL2Block(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getTxProof() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new CKBMerkleProof(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getKvStateProof() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getBlockHashesProof() {
      const start = 20;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getAccountCount() {
      const start = 24;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getKvState() {
      const start = 28;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new KVPairVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getLoadData() {
      const start = 32;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new BytesVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getScripts() {
      const start = 36;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new ScriptVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getReturnDataHash() {
      const start = 40;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getBlockHashes() {
      const start = 44;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new BlockHashEntryVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeCCTransactionWitness(value) {
    const buffers = [];
    buffers.push(SerializeL2Transaction(value.l2tx));
    buffers.push(SerializeRawL2Block(value.raw_l2block));
    buffers.push(SerializeCKBMerkleProof(value.tx_proof));
    buffers.push(SerializeBytes(value.kv_state_proof));
    buffers.push(SerializeBytes(value.block_hashes_proof));
    buffers.push(SerializeUint32(value.account_count));
    buffers.push(SerializeKVPairVec(value.kv_state));
    buffers.push(SerializeBytesVec(value.load_data));
    buffers.push(SerializeScriptVec(value.scripts));
    buffers.push(SerializeByte32(value.return_data_hash));
    buffers.push(SerializeBlockHashEntryVec(value.block_hashes));
    return serializeTable(buffers);
  }

  class CCTransactionSignatureWitness {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new RawL2Block(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new L2Transaction(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new CKBMerkleProof(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new KVPairVec(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
      new Uint32(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
      new Script(this.view.buffer.slice(offsets[6], offsets[7]), { validate: false }).validate();
      new Script(this.view.buffer.slice(offsets[7], offsets[8]), { validate: false }).validate();
    }

    getRawL2Block() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new RawL2Block(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getL2Tx() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new L2Transaction(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getTxProof() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new CKBMerkleProof(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getKvState() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new KVPairVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getKvStateProof() {
      const start = 20;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getAccountCount() {
      const start = 24;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getSender() {
      const start = 28;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getReceiver() {
      const start = 32;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeCCTransactionSignatureWitness(value) {
    const buffers = [];
    buffers.push(SerializeRawL2Block(value.raw_l2block));
    buffers.push(SerializeL2Transaction(value.l2tx));
    buffers.push(SerializeCKBMerkleProof(value.tx_proof));
    buffers.push(SerializeKVPairVec(value.kv_state));
    buffers.push(SerializeBytes(value.kv_state_proof));
    buffers.push(SerializeUint32(value.account_count));
    buffers.push(SerializeScript(value.sender));
    buffers.push(SerializeScript(value.receiver));
    return serializeTable(buffers);
  }

  class CCWithdrawalWitness {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new RawL2Block(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new WithdrawalRequest(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Script(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new Script(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
      new CKBMerkleProof(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
      new KVPairVec(this.view.buffer.slice(offsets[6], offsets[7]), { validate: false }).validate();
      new Uint32(this.view.buffer.slice(offsets[7], offsets[8]), { validate: false }).validate();
    }

    getRawL2Block() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new RawL2Block(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getWithdrawal() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new WithdrawalRequest(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getSender() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getOwnerLock() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getWithdrawalProof() {
      const start = 20;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new CKBMerkleProof(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getKvStateProof() {
      const start = 24;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getKvState() {
      const start = 28;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new KVPairVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getAccountCount() {
      const start = 32;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeCCWithdrawalWitness(value) {
    const buffers = [];
    buffers.push(SerializeRawL2Block(value.raw_l2block));
    buffers.push(SerializeWithdrawalRequest(value.withdrawal));
    buffers.push(SerializeScript(value.sender));
    buffers.push(SerializeScript(value.owner_lock));
    buffers.push(SerializeCKBMerkleProof(value.withdrawal_proof));
    buffers.push(SerializeBytes(value.kv_state_proof));
    buffers.push(SerializeKVPairVec(value.kv_state));
    buffers.push(SerializeUint32(value.account_count));
    return serializeTable(buffers);
  }

  class RollupSubmitBlock {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new L2Block(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Byte32Vec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    }

    getBlock() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new L2Block(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getRevertedBlockHashes() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getRevertedBlockProof() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeRollupSubmitBlock(value) {
    const buffers = [];
    buffers.push(SerializeL2Block(value.block));
    buffers.push(SerializeByte32Vec(value.reverted_block_hashes));
    buffers.push(SerializeBytes(value.reverted_block_proof));
    return serializeTable(buffers);
  }

  class RollupEnterChallenge {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new ChallengeWitness(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    }

    getWitness() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new ChallengeWitness(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeRollupEnterChallenge(value) {
    const buffers = [];
    buffers.push(SerializeChallengeWitness(value.witness));
    return serializeTable(buffers);
  }

  class RollupCancelChallenge {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      verifyAndExtractOffsets(this.view, 0, true);
    }

  }

  function SerializeRollupCancelChallenge(value) {
    const buffers = [];
    return serializeTable(buffers);
  }

  class RollupRevert {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new RawL2BlockVec(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new RawL2Block(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
    }

    getRevertedBlocks() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new RawL2BlockVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getBlockProof() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getRevertedBlockProof() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getNewTipBlock() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new RawL2Block(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeRollupRevert(value) {
    const buffers = [];
    buffers.push(SerializeRawL2BlockVec(value.reverted_blocks));
    buffers.push(SerializeBytes(value.block_proof));
    buffers.push(SerializeBytes(value.reverted_block_proof));
    buffers.push(SerializeRawL2Block(value.new_tip_block));
    return serializeTable(buffers);
  }

  class RollupAction {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        assertDataLength(this.view.byteLength, ">4");
      }
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        new RollupSubmitBlock(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      case 1:
        new RollupEnterChallenge(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      case 2:
        new RollupCancelChallenge(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      case 3:
        new RollupRevert(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }

    unionType() {
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        return "RollupSubmitBlock";
      case 1:
        return "RollupEnterChallenge";
      case 2:
        return "RollupCancelChallenge";
      case 3:
        return "RollupRevert";
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }

    value() {
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        return new RollupSubmitBlock(this.view.buffer.slice(4), { validate: false });
      case 1:
        return new RollupEnterChallenge(this.view.buffer.slice(4), { validate: false });
      case 2:
        return new RollupCancelChallenge(this.view.buffer.slice(4), { validate: false });
      case 3:
        return new RollupRevert(this.view.buffer.slice(4), { validate: false });
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }
  }

  function SerializeRollupAction(value) {
    switch (value.type) {
    case "RollupSubmitBlock":
      {
        const itemBuffer = SerializeRollupSubmitBlock(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 0, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    case "RollupEnterChallenge":
      {
        const itemBuffer = SerializeRollupEnterChallenge(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 1, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    case "RollupCancelChallenge":
      {
        const itemBuffer = SerializeRollupCancelChallenge(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 2, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    case "RollupRevert":
      {
        const itemBuffer = SerializeRollupRevert(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 3, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    default:
      throw new Error(`Invalid type: ${value.type}`);
    }

  }

  class Byte20 {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, 20);
    }

    indexAt(i) {
      return this.view.getUint8(i);
    }

    raw() {
      return this.view.buffer;
    }

    static size() {
      return 20;
    }
  }

  function SerializeByte20(value) {
    const buffer = assertArrayBuffer(value);
    assertDataLength(buffer.byteLength, 20);
    return buffer;
  }

  class ETHAddrRegArgs {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        assertDataLength(this.view.byteLength, ">4");
      }
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        new EthToGw(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      case 1:
        new GwToEth(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      case 2:
        new SetMapping(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      case 3:
        new BatchSetMapping(this.view.buffer.slice(4), { validate: false }).validate();
        break;
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }

    unionType() {
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        return "EthToGw";
      case 1:
        return "GwToEth";
      case 2:
        return "SetMapping";
      case 3:
        return "BatchSetMapping";
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }

    value() {
      const t = this.view.getUint32(0, true);
      switch (t) {
      case 0:
        return new EthToGw(this.view.buffer.slice(4), { validate: false });
      case 1:
        return new GwToEth(this.view.buffer.slice(4), { validate: false });
      case 2:
        return new SetMapping(this.view.buffer.slice(4), { validate: false });
      case 3:
        return new BatchSetMapping(this.view.buffer.slice(4), { validate: false });
      default:
        throw new Error(`Invalid type: ${t}`);
      }
    }
  }

  function SerializeETHAddrRegArgs(value) {
    switch (value.type) {
    case "EthToGw":
      {
        const itemBuffer = SerializeEthToGw(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 0, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    case "GwToEth":
      {
        const itemBuffer = SerializeGwToEth(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 1, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    case "SetMapping":
      {
        const itemBuffer = SerializeSetMapping(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 2, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    case "BatchSetMapping":
      {
        const itemBuffer = SerializeBatchSetMapping(value.value);
        const array = new Uint8Array(4 + itemBuffer.byteLength);
        const view = new DataView(array.buffer);
        view.setUint32(0, 3, true);
        array.set(new Uint8Array(itemBuffer), 4);
        return array.buffer;
      }
    default:
      throw new Error(`Invalid type: ${value.type}`);
    }

  }

  class EthToGw {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getEthAddress() {
      return new Byte20(this.view.buffer.slice(0, 0 + Byte20.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, EthToGw.size());
      this.getEthAddress().validate(compatible);
    }
    static size() {
      return 0 + Byte20.size();
    }
  }

  function SerializeEthToGw(value) {
    const array = new Uint8Array(0 + Byte20.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte20(value.eth_address)), 0);
    return array.buffer;
  }

  class GwToEth {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getGwScriptHash() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, GwToEth.size());
      this.getGwScriptHash().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size();
    }
  }

  function SerializeGwToEth(value) {
    const array = new Uint8Array(0 + Byte32.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.gw_script_hash)), 0);
    return array.buffer;
  }

  class SetMapping {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getGwScriptHash() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getFee() {
      return new Fee(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Fee.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, SetMapping.size());
      this.getGwScriptHash().validate(compatible);
      this.getFee().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + Fee.size();
    }
  }

  function SerializeSetMapping(value) {
    const array = new Uint8Array(0 + Byte32.size() + Fee.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.gw_script_hash)), 0);
    array.set(new Uint8Array(SerializeFee(value.fee)), 0 + Byte32.size());
    return array.buffer;
  }

  class BatchSetMapping {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Byte32Vec(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Fee(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    }

    getGwScriptHashes() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getFee() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Fee(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeBatchSetMapping(value) {
    const buffers = [];
    buffers.push(SerializeByte32Vec(value.gw_script_hashes));
    buffers.push(SerializeFee(value.fee));
    return serializeTable(buffers);
  }

  class Uint16 {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, 2);
    }

    indexAt(i) {
      return this.view.getUint8(i);
    }

    raw() {
      return this.view.buffer;
    }

    toBigEndianUint16() {
      return this.view.getUint16(0, false);
    }

    toLittleEndianUint16() {
      return this.view.getUint16(0, true);
    }

    static size() {
      return 2;
    }
  }

  function SerializeUint16(value) {
    const buffer = assertArrayBuffer(value);
    assertDataLength(buffer.byteLength, 2);
    return buffer;
  }

  class Uint32 {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, 4);
    }

    indexAt(i) {
      return this.view.getUint8(i);
    }

    raw() {
      return this.view.buffer;
    }

    toBigEndianUint32() {
      return this.view.getUint32(0, false);
    }

    toLittleEndianUint32() {
      return this.view.getUint32(0, true);
    }

    static size() {
      return 4;
    }
  }

  function SerializeUint32(value) {
    const buffer = assertArrayBuffer(value);
    assertDataLength(buffer.byteLength, 4);
    return buffer;
  }

  class Uint64 {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, 8);
    }

    indexAt(i) {
      return this.view.getUint8(i);
    }

    raw() {
      return this.view.buffer;
    }

    toBigEndianBigUint64() {
      return this.view.getBigUint64(0, false);
    }

    toLittleEndianBigUint64() {
      return this.view.getBigUint64(0, true);
    }

    static size() {
      return 8;
    }
  }

  function SerializeUint64(value) {
    const buffer = assertArrayBuffer(value);
    assertDataLength(buffer.byteLength, 8);
    return buffer;
  }

  class Uint128 {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, 16);
    }

    indexAt(i) {
      return this.view.getUint8(i);
    }

    raw() {
      return this.view.buffer;
    }

    static size() {
      return 16;
    }
  }

  function SerializeUint128(value) {
    const buffer = assertArrayBuffer(value);
    assertDataLength(buffer.byteLength, 16);
    return buffer;
  }

  class Byte32 {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, 32);
    }

    indexAt(i) {
      return this.view.getUint8(i);
    }

    raw() {
      return this.view.buffer;
    }

    static size() {
      return 32;
    }
  }

  function SerializeByte32(value) {
    const buffer = assertArrayBuffer(value);
    assertDataLength(buffer.byteLength, 32);
    return buffer;
  }

  class Uint256 {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, 32);
    }

    indexAt(i) {
      return this.view.getUint8(i);
    }

    raw() {
      return this.view.buffer;
    }

    static size() {
      return 32;
    }
  }

  function SerializeUint256(value) {
    const buffer = assertArrayBuffer(value);
    assertDataLength(buffer.byteLength, 32);
    return buffer;
  }

  class Bytes {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        dataLengthError(this.view.byteLength, ">4");
      }
      const requiredByteLength = this.length() + 4;
      assertDataLength(this.view.byteLength, requiredByteLength);
    }

    raw() {
      return this.view.buffer.slice(4);
    }

    indexAt(i) {
      return this.view.getUint8(4 + i);
    }

    length() {
      return this.view.getUint32(0, true);
    }
  }

  function SerializeBytes(value) {
    const item = assertArrayBuffer(value);
    const array = new Uint8Array(4 + item.byteLength);
    (new DataView(array.buffer)).setUint32(0, item.byteLength, true);
    array.set(new Uint8Array(item), 4);
    return array.buffer;
  }

  class BytesOpt {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.hasValue()) {
        this.value().validate(compatible);
      }
    }

    value() {
      return new Bytes(this.view.buffer, { validate: false });
    }

    hasValue() {
      return this.view.byteLength > 0;
    }
  }

  function SerializeBytesOpt(value) {
    if (value) {
      return SerializeBytes(value);
    } else {
      return new ArrayBuffer(0);
    }
  }

  class BytesVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      for (let i = 0; i < offsets.length - 1; i++) {
        new Bytes(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
      }
    }

    length() {
      if (this.view.byteLength < 8) {
        return 0;
      } else {
        return this.view.getUint32(4, true) / 4 - 1;
      }
    }

    indexAt(i) {
      const start = 4 + i * 4;
      const offset = this.view.getUint32(start, true);
      let offset_end = this.view.byteLength;
      if (i + 1 < this.length()) {
        offset_end = this.view.getUint32(start + 4, true);
      }
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeBytesVec(value) {
    return serializeTable(value.map(item => SerializeBytes(item)));
  }

  class Byte32Vec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        dataLengthError(this.view.byteLength, ">4");
      }
      const requiredByteLength = this.length() * Byte32.size() + 4;
      assertDataLength(this.view.byteLength, requiredByteLength);
      for (let i = 0; i < 0; i++) {
        const item = this.indexAt(i);
        item.validate(compatible);
      }
    }

    indexAt(i) {
      return new Byte32(this.view.buffer.slice(4 + i * Byte32.size(), 4 + (i + 1) * Byte32.size()), { validate: false });
    }

    length() {
      return this.view.getUint32(0, true);
    }
  }

  function SerializeByte32Vec(value) {
    const array = new Uint8Array(4 + Byte32.size() * value.length);
    (new DataView(array.buffer)).setUint32(0, value.length, true);
    for (let i = 0; i < value.length; i++) {
      const itemBuffer = SerializeByte32(value[i]);
      array.set(new Uint8Array(itemBuffer), 4 + i * Byte32.size());
    }
    return array.buffer;
  }

  class ScriptOpt {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.hasValue()) {
        this.value().validate(compatible);
      }
    }

    value() {
      return new Script(this.view.buffer, { validate: false });
    }

    hasValue() {
      return this.view.byteLength > 0;
    }
  }

  function SerializeScriptOpt(value) {
    if (value) {
      return SerializeScript(value);
    } else {
      return new ArrayBuffer(0);
    }
  }

  class ProposalShortId {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, 10);
    }

    indexAt(i) {
      return this.view.getUint8(i);
    }

    raw() {
      return this.view.buffer;
    }

    static size() {
      return 10;
    }
  }

  function SerializeProposalShortId(value) {
    const buffer = assertArrayBuffer(value);
    assertDataLength(buffer.byteLength, 10);
    return buffer;
  }

  class UncleBlockVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      for (let i = 0; i < offsets.length - 1; i++) {
        new UncleBlock(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
      }
    }

    length() {
      if (this.view.byteLength < 8) {
        return 0;
      } else {
        return this.view.getUint32(4, true) / 4 - 1;
      }
    }

    indexAt(i) {
      const start = 4 + i * 4;
      const offset = this.view.getUint32(start, true);
      let offset_end = this.view.byteLength;
      if (i + 1 < this.length()) {
        offset_end = this.view.getUint32(start + 4, true);
      }
      return new UncleBlock(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeUncleBlockVec(value) {
    return serializeTable(value.map(item => SerializeUncleBlock(item)));
  }

  class TransactionVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      for (let i = 0; i < offsets.length - 1; i++) {
        new Transaction(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
      }
    }

    length() {
      if (this.view.byteLength < 8) {
        return 0;
      } else {
        return this.view.getUint32(4, true) / 4 - 1;
      }
    }

    indexAt(i) {
      const start = 4 + i * 4;
      const offset = this.view.getUint32(start, true);
      let offset_end = this.view.byteLength;
      if (i + 1 < this.length()) {
        offset_end = this.view.getUint32(start + 4, true);
      }
      return new Transaction(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeTransactionVec(value) {
    return serializeTable(value.map(item => SerializeTransaction(item)));
  }

  class ProposalShortIdVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        dataLengthError(this.view.byteLength, ">4");
      }
      const requiredByteLength = this.length() * ProposalShortId.size() + 4;
      assertDataLength(this.view.byteLength, requiredByteLength);
      for (let i = 0; i < 0; i++) {
        const item = this.indexAt(i);
        item.validate(compatible);
      }
    }

    indexAt(i) {
      return new ProposalShortId(this.view.buffer.slice(4 + i * ProposalShortId.size(), 4 + (i + 1) * ProposalShortId.size()), { validate: false });
    }

    length() {
      return this.view.getUint32(0, true);
    }
  }

  function SerializeProposalShortIdVec(value) {
    const array = new Uint8Array(4 + ProposalShortId.size() * value.length);
    (new DataView(array.buffer)).setUint32(0, value.length, true);
    for (let i = 0; i < value.length; i++) {
      const itemBuffer = SerializeProposalShortId(value[i]);
      array.set(new Uint8Array(itemBuffer), 4 + i * ProposalShortId.size());
    }
    return array.buffer;
  }

  class CellDepVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        dataLengthError(this.view.byteLength, ">4");
      }
      const requiredByteLength = this.length() * CellDep.size() + 4;
      assertDataLength(this.view.byteLength, requiredByteLength);
      for (let i = 0; i < 0; i++) {
        const item = this.indexAt(i);
        item.validate(compatible);
      }
    }

    indexAt(i) {
      return new CellDep(this.view.buffer.slice(4 + i * CellDep.size(), 4 + (i + 1) * CellDep.size()), { validate: false });
    }

    length() {
      return this.view.getUint32(0, true);
    }
  }

  function SerializeCellDepVec(value) {
    const array = new Uint8Array(4 + CellDep.size() * value.length);
    (new DataView(array.buffer)).setUint32(0, value.length, true);
    for (let i = 0; i < value.length; i++) {
      const itemBuffer = SerializeCellDep(value[i]);
      array.set(new Uint8Array(itemBuffer), 4 + i * CellDep.size());
    }
    return array.buffer;
  }

  class CellInputVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      if (this.view.byteLength < 4) {
        dataLengthError(this.view.byteLength, ">4");
      }
      const requiredByteLength = this.length() * CellInput.size() + 4;
      assertDataLength(this.view.byteLength, requiredByteLength);
      for (let i = 0; i < 0; i++) {
        const item = this.indexAt(i);
        item.validate(compatible);
      }
    }

    indexAt(i) {
      return new CellInput(this.view.buffer.slice(4 + i * CellInput.size(), 4 + (i + 1) * CellInput.size()), { validate: false });
    }

    length() {
      return this.view.getUint32(0, true);
    }
  }

  function SerializeCellInputVec(value) {
    const array = new Uint8Array(4 + CellInput.size() * value.length);
    (new DataView(array.buffer)).setUint32(0, value.length, true);
    for (let i = 0; i < value.length; i++) {
      const itemBuffer = SerializeCellInput(value[i]);
      array.set(new Uint8Array(itemBuffer), 4 + i * CellInput.size());
    }
    return array.buffer;
  }

  class CellOutputVec {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      for (let i = 0; i < offsets.length - 1; i++) {
        new CellOutput(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
      }
    }

    length() {
      if (this.view.byteLength < 8) {
        return 0;
      } else {
        return this.view.getUint32(4, true) / 4 - 1;
      }
    }

    indexAt(i) {
      const start = 4 + i * 4;
      const offset = this.view.getUint32(start, true);
      let offset_end = this.view.byteLength;
      if (i + 1 < this.length()) {
        offset_end = this.view.getUint32(start + 4, true);
      }
      return new CellOutput(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeCellOutputVec(value) {
    return serializeTable(value.map(item => SerializeCellOutput(item)));
  }

  class Script {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Byte32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      if (offsets[2] - offsets[1] !== 1) {
        throw new Error(`Invalid offset for hash_type: ${offsets[1]} - ${offsets[2]}`)
      }
      new Bytes(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    }

    getCodeHash() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getHashType() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new DataView(this.view.buffer.slice(offset, offset_end)).getUint8(0);
    }

    getArgs() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeScript(value) {
    const buffers = [];
    buffers.push(SerializeByte32(value.code_hash));
    const hashTypeView = new DataView(new ArrayBuffer(1));
    hashTypeView.setUint8(0, value.hash_type);
    buffers.push(hashTypeView.buffer);
    buffers.push(SerializeBytes(value.args));
    return serializeTable(buffers);
  }

  class OutPoint {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getTxHash() {
      return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
    }

    getIndex() {
      return new Uint32(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Uint32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, OutPoint.size());
      this.getTxHash().validate(compatible);
      this.getIndex().validate(compatible);
    }
    static size() {
      return 0 + Byte32.size() + Uint32.size();
    }
  }

  function SerializeOutPoint(value) {
    const array = new Uint8Array(0 + Byte32.size() + Uint32.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeByte32(value.tx_hash)), 0);
    array.set(new Uint8Array(SerializeUint32(value.index)), 0 + Byte32.size());
    return array.buffer;
  }

  class CellInput {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getSince() {
      return new Uint64(this.view.buffer.slice(0, 0 + Uint64.size()), { validate: false });
    }

    getPreviousOutput() {
      return new OutPoint(this.view.buffer.slice(0 + Uint64.size(), 0 + Uint64.size() + OutPoint.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, CellInput.size());
      this.getSince().validate(compatible);
      this.getPreviousOutput().validate(compatible);
    }
    static size() {
      return 0 + Uint64.size() + OutPoint.size();
    }
  }

  function SerializeCellInput(value) {
    const array = new Uint8Array(0 + Uint64.size() + OutPoint.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeUint64(value.since)), 0);
    array.set(new Uint8Array(SerializeOutPoint(value.previous_output)), 0 + Uint64.size());
    return array.buffer;
  }

  class CellOutput {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Uint64(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Script(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new ScriptOpt(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    }

    getCapacity() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getLock() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getType() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new ScriptOpt(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeCellOutput(value) {
    const buffers = [];
    buffers.push(SerializeUint64(value.capacity));
    buffers.push(SerializeScript(value.lock));
    buffers.push(SerializeScriptOpt(value.type_));
    return serializeTable(buffers);
  }

  class CellDep {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getOutPoint() {
      return new OutPoint(this.view.buffer.slice(0, 0 + OutPoint.size()), { validate: false });
    }

    getDepType() {
      return this.view.getUint8(0 + OutPoint.size());
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, CellDep.size());
      this.getOutPoint().validate(compatible);
    }
    static size() {
      return 0 + OutPoint.size() + 1;
    }
  }

  function SerializeCellDep(value) {
    const array = new Uint8Array(0 + OutPoint.size() + 1);
    const view = new DataView(array.buffer);
    array.set(new Uint8Array(SerializeOutPoint(value.out_point)), 0);
    view.setUint8(0 + OutPoint.size(), value.dep_type);
    return array.buffer;
  }

  class RawTransaction {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Uint32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new CellDepVec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new Byte32Vec(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new CellInputVec(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
      new CellOutputVec(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
      new BytesVec(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
    }

    getVersion() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getCellDeps() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new CellDepVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getHeaderDeps() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getInputs() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new CellInputVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getOutputs() {
      const start = 20;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new CellOutputVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getOutputsData() {
      const start = 24;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new BytesVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeRawTransaction(value) {
    const buffers = [];
    buffers.push(SerializeUint32(value.version));
    buffers.push(SerializeCellDepVec(value.cell_deps));
    buffers.push(SerializeByte32Vec(value.header_deps));
    buffers.push(SerializeCellInputVec(value.inputs));
    buffers.push(SerializeCellOutputVec(value.outputs));
    buffers.push(SerializeBytesVec(value.outputs_data));
    return serializeTable(buffers);
  }

  class Transaction {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new RawTransaction(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new BytesVec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    }

    getRaw() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new RawTransaction(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getWitnesses() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new BytesVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeTransaction(value) {
    const buffers = [];
    buffers.push(SerializeRawTransaction(value.raw));
    buffers.push(SerializeBytesVec(value.witnesses));
    return serializeTable(buffers);
  }

  class RawHeader {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getVersion() {
      return new Uint32(this.view.buffer.slice(0, 0 + Uint32.size()), { validate: false });
    }

    getCompactTarget() {
      return new Uint32(this.view.buffer.slice(0 + Uint32.size(), 0 + Uint32.size() + Uint32.size()), { validate: false });
    }

    getTimestamp() {
      return new Uint64(this.view.buffer.slice(0 + Uint32.size() + Uint32.size(), 0 + Uint32.size() + Uint32.size() + Uint64.size()), { validate: false });
    }

    getNumber() {
      return new Uint64(this.view.buffer.slice(0 + Uint32.size() + Uint32.size() + Uint64.size(), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size()), { validate: false });
    }

    getEpoch() {
      return new Uint64(this.view.buffer.slice(0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size(), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size()), { validate: false });
    }

    getParentHash() {
      return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size(), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size()), { validate: false });
    }

    getTransactionsRoot() {
      return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size(), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size()), { validate: false });
    }

    getProposalsHash() {
      return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size(), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size()), { validate: false });
    }

    getExtraHash() {
      return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size(), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size()), { validate: false });
    }

    getDao() {
      return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size(), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, RawHeader.size());
      this.getVersion().validate(compatible);
      this.getCompactTarget().validate(compatible);
      this.getTimestamp().validate(compatible);
      this.getNumber().validate(compatible);
      this.getEpoch().validate(compatible);
      this.getParentHash().validate(compatible);
      this.getTransactionsRoot().validate(compatible);
      this.getProposalsHash().validate(compatible);
      this.getExtraHash().validate(compatible);
      this.getDao().validate(compatible);
    }
    static size() {
      return 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size();
    }
  }

  function SerializeRawHeader(value) {
    const array = new Uint8Array(0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeUint32(value.version)), 0);
    array.set(new Uint8Array(SerializeUint32(value.compact_target)), 0 + Uint32.size());
    array.set(new Uint8Array(SerializeUint64(value.timestamp)), 0 + Uint32.size() + Uint32.size());
    array.set(new Uint8Array(SerializeUint64(value.number)), 0 + Uint32.size() + Uint32.size() + Uint64.size());
    array.set(new Uint8Array(SerializeUint64(value.epoch)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size());
    array.set(new Uint8Array(SerializeByte32(value.parent_hash)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size());
    array.set(new Uint8Array(SerializeByte32(value.transactions_root)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size());
    array.set(new Uint8Array(SerializeByte32(value.proposals_hash)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size());
    array.set(new Uint8Array(SerializeByte32(value.extra_hash)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size());
    array.set(new Uint8Array(SerializeByte32(value.dao)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size());
    return array.buffer;
  }

  class Header {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    getRaw() {
      return new RawHeader(this.view.buffer.slice(0, 0 + RawHeader.size()), { validate: false });
    }

    getNonce() {
      return new Uint128(this.view.buffer.slice(0 + RawHeader.size(), 0 + RawHeader.size() + Uint128.size()), { validate: false });
    }

    validate(compatible = false) {
      assertDataLength(this.view.byteLength, Header.size());
      this.getRaw().validate(compatible);
      this.getNonce().validate(compatible);
    }
    static size() {
      return 0 + RawHeader.size() + Uint128.size();
    }
  }

  function SerializeHeader(value) {
    const array = new Uint8Array(0 + RawHeader.size() + Uint128.size());
    new DataView(array.buffer);
    array.set(new Uint8Array(SerializeRawHeader(value.raw)), 0);
    array.set(new Uint8Array(SerializeUint128(value.nonce)), 0 + RawHeader.size());
    return array.buffer;
  }

  class UncleBlock {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Header(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new ProposalShortIdVec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    }

    getHeader() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Header(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getProposals() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new ProposalShortIdVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeUncleBlock(value) {
    const buffers = [];
    buffers.push(SerializeHeader(value.header));
    buffers.push(SerializeProposalShortIdVec(value.proposals));
    return serializeTable(buffers);
  }

  class Block {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Header(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new UncleBlockVec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new TransactionVec(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new ProposalShortIdVec(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
    }

    getHeader() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Header(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getUncles() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new UncleBlockVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getTransactions() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new TransactionVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getProposals() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new ProposalShortIdVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeBlock(value) {
    const buffers = [];
    buffers.push(SerializeHeader(value.header));
    buffers.push(SerializeUncleBlockVec(value.uncles));
    buffers.push(SerializeTransactionVec(value.transactions));
    buffers.push(SerializeProposalShortIdVec(value.proposals));
    return serializeTable(buffers);
  }

  class BlockV1 {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Header(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new UncleBlockVec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new TransactionVec(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
      new ProposalShortIdVec(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
    }

    getHeader() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Header(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getUncles() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new UncleBlockVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getTransactions() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new TransactionVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getProposals() {
      const start = 16;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new ProposalShortIdVec(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getExtension() {
      const start = 20;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeBlockV1(value) {
    const buffers = [];
    buffers.push(SerializeHeader(value.header));
    buffers.push(SerializeUncleBlockVec(value.uncles));
    buffers.push(SerializeTransactionVec(value.transactions));
    buffers.push(SerializeProposalShortIdVec(value.proposals));
    buffers.push(SerializeBytes(value.extension));
    return serializeTable(buffers);
  }

  class CellbaseWitness {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new Script(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new Bytes(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    }

    getLock() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getMessage() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeCellbaseWitness(value) {
    const buffers = [];
    buffers.push(SerializeScript(value.lock));
    buffers.push(SerializeBytes(value.message));
    return serializeTable(buffers);
  }

  class WitnessArgs {
    constructor(reader, { validate = true } = {}) {
      this.view = new DataView(assertArrayBuffer(reader));
      if (validate) {
        this.validate();
      }
    }

    validate(compatible = false) {
      const offsets = verifyAndExtractOffsets(this.view, 0, true);
      new BytesOpt(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
      new BytesOpt(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
      new BytesOpt(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    }

    getLock() {
      const start = 4;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new BytesOpt(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getInputType() {
      const start = 8;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.getUint32(start + 4, true);
      return new BytesOpt(this.view.buffer.slice(offset, offset_end), { validate: false });
    }

    getOutputType() {
      const start = 12;
      const offset = this.view.getUint32(start, true);
      const offset_end = this.view.byteLength;
      return new BytesOpt(this.view.buffer.slice(offset, offset_end), { validate: false });
    }
  }

  function SerializeWitnessArgs(value) {
    const buffers = [];
    buffers.push(SerializeBytesOpt(value.lock));
    buffers.push(SerializeBytesOpt(value.input_type));
    buffers.push(SerializeBytesOpt(value.output_type));
    return serializeTable(buffers);
  }

  exports.AccountMerkleState = AccountMerkleState;
  exports.AllowedTypeHash = AllowedTypeHash;
  exports.AllowedTypeHashVec = AllowedTypeHashVec;
  exports.BatchCreateEthAccounts = BatchCreateEthAccounts;
  exports.BatchSetMapping = BatchSetMapping;
  exports.Block = Block;
  exports.BlockHashEntry = BlockHashEntry;
  exports.BlockHashEntryVec = BlockHashEntryVec;
  exports.BlockInfo = BlockInfo;
  exports.BlockMerkleState = BlockMerkleState;
  exports.BlockV1 = BlockV1;
  exports.Byte20 = Byte20;
  exports.Byte32 = Byte32;
  exports.Byte32Vec = Byte32Vec;
  exports.Bytes = Bytes;
  exports.BytesOpt = BytesOpt;
  exports.BytesVec = BytesVec;
  exports.CCTransactionSignatureWitness = CCTransactionSignatureWitness;
  exports.CCTransactionWitness = CCTransactionWitness;
  exports.CCWithdrawalWitness = CCWithdrawalWitness;
  exports.CKBMerkleProof = CKBMerkleProof;
  exports.CellDep = CellDep;
  exports.CellDepVec = CellDepVec;
  exports.CellInput = CellInput;
  exports.CellInputVec = CellInputVec;
  exports.CellOutput = CellOutput;
  exports.CellOutputVec = CellOutputVec;
  exports.CellbaseWitness = CellbaseWitness;
  exports.ChallengeLockArgs = ChallengeLockArgs;
  exports.ChallengeTarget = ChallengeTarget;
  exports.ChallengeWitness = ChallengeWitness;
  exports.CreateAccount = CreateAccount;
  exports.CustodianLockArgs = CustodianLockArgs;
  exports.DepositLockArgs = DepositLockArgs;
  exports.DepositRequest = DepositRequest;
  exports.DepositRequestVec = DepositRequestVec;
  exports.ETHAddrRegArgs = ETHAddrRegArgs;
  exports.EthToGw = EthToGw;
  exports.Fee = Fee;
  exports.GlobalState = GlobalState;
  exports.GlobalStateV0 = GlobalStateV0;
  exports.GwToEth = GwToEth;
  exports.Header = Header;
  exports.KVPair = KVPair;
  exports.KVPairVec = KVPairVec;
  exports.L2Block = L2Block;
  exports.L2Transaction = L2Transaction;
  exports.L2TransactionVec = L2TransactionVec;
  exports.MetaContractArgs = MetaContractArgs;
  exports.OutPoint = OutPoint;
  exports.ProposalShortId = ProposalShortId;
  exports.ProposalShortIdVec = ProposalShortIdVec;
  exports.RawHeader = RawHeader;
  exports.RawL2Block = RawL2Block;
  exports.RawL2BlockVec = RawL2BlockVec;
  exports.RawL2Transaction = RawL2Transaction;
  exports.RawTransaction = RawTransaction;
  exports.RawWithdrawalRequest = RawWithdrawalRequest;
  exports.RollupAction = RollupAction;
  exports.RollupCancelChallenge = RollupCancelChallenge;
  exports.RollupConfig = RollupConfig;
  exports.RollupEnterChallenge = RollupEnterChallenge;
  exports.RollupRevert = RollupRevert;
  exports.RollupSubmitBlock = RollupSubmitBlock;
  exports.SUDTArgs = SUDTArgs;
  exports.SUDTQuery = SUDTQuery;
  exports.SUDTTransfer = SUDTTransfer;
  exports.Script = Script;
  exports.ScriptOpt = ScriptOpt;
  exports.ScriptVec = ScriptVec;
  exports.SerializeAccountMerkleState = SerializeAccountMerkleState;
  exports.SerializeAllowedTypeHash = SerializeAllowedTypeHash;
  exports.SerializeAllowedTypeHashVec = SerializeAllowedTypeHashVec;
  exports.SerializeBatchCreateEthAccounts = SerializeBatchCreateEthAccounts;
  exports.SerializeBatchSetMapping = SerializeBatchSetMapping;
  exports.SerializeBlock = SerializeBlock;
  exports.SerializeBlockHashEntry = SerializeBlockHashEntry;
  exports.SerializeBlockHashEntryVec = SerializeBlockHashEntryVec;
  exports.SerializeBlockInfo = SerializeBlockInfo;
  exports.SerializeBlockMerkleState = SerializeBlockMerkleState;
  exports.SerializeBlockV1 = SerializeBlockV1;
  exports.SerializeByte20 = SerializeByte20;
  exports.SerializeByte32 = SerializeByte32;
  exports.SerializeByte32Vec = SerializeByte32Vec;
  exports.SerializeBytes = SerializeBytes;
  exports.SerializeBytesOpt = SerializeBytesOpt;
  exports.SerializeBytesVec = SerializeBytesVec;
  exports.SerializeCCTransactionSignatureWitness = SerializeCCTransactionSignatureWitness;
  exports.SerializeCCTransactionWitness = SerializeCCTransactionWitness;
  exports.SerializeCCWithdrawalWitness = SerializeCCWithdrawalWitness;
  exports.SerializeCKBMerkleProof = SerializeCKBMerkleProof;
  exports.SerializeCellDep = SerializeCellDep;
  exports.SerializeCellDepVec = SerializeCellDepVec;
  exports.SerializeCellInput = SerializeCellInput;
  exports.SerializeCellInputVec = SerializeCellInputVec;
  exports.SerializeCellOutput = SerializeCellOutput;
  exports.SerializeCellOutputVec = SerializeCellOutputVec;
  exports.SerializeCellbaseWitness = SerializeCellbaseWitness;
  exports.SerializeChallengeLockArgs = SerializeChallengeLockArgs;
  exports.SerializeChallengeTarget = SerializeChallengeTarget;
  exports.SerializeChallengeWitness = SerializeChallengeWitness;
  exports.SerializeCreateAccount = SerializeCreateAccount;
  exports.SerializeCustodianLockArgs = SerializeCustodianLockArgs;
  exports.SerializeDepositLockArgs = SerializeDepositLockArgs;
  exports.SerializeDepositRequest = SerializeDepositRequest;
  exports.SerializeDepositRequestVec = SerializeDepositRequestVec;
  exports.SerializeETHAddrRegArgs = SerializeETHAddrRegArgs;
  exports.SerializeEthToGw = SerializeEthToGw;
  exports.SerializeFee = SerializeFee;
  exports.SerializeGlobalState = SerializeGlobalState;
  exports.SerializeGlobalStateV0 = SerializeGlobalStateV0;
  exports.SerializeGwToEth = SerializeGwToEth;
  exports.SerializeHeader = SerializeHeader;
  exports.SerializeKVPair = SerializeKVPair;
  exports.SerializeKVPairVec = SerializeKVPairVec;
  exports.SerializeL2Block = SerializeL2Block;
  exports.SerializeL2Transaction = SerializeL2Transaction;
  exports.SerializeL2TransactionVec = SerializeL2TransactionVec;
  exports.SerializeMetaContractArgs = SerializeMetaContractArgs;
  exports.SerializeOutPoint = SerializeOutPoint;
  exports.SerializeProposalShortId = SerializeProposalShortId;
  exports.SerializeProposalShortIdVec = SerializeProposalShortIdVec;
  exports.SerializeRawHeader = SerializeRawHeader;
  exports.SerializeRawL2Block = SerializeRawL2Block;
  exports.SerializeRawL2BlockVec = SerializeRawL2BlockVec;
  exports.SerializeRawL2Transaction = SerializeRawL2Transaction;
  exports.SerializeRawTransaction = SerializeRawTransaction;
  exports.SerializeRawWithdrawalRequest = SerializeRawWithdrawalRequest;
  exports.SerializeRollupAction = SerializeRollupAction;
  exports.SerializeRollupCancelChallenge = SerializeRollupCancelChallenge;
  exports.SerializeRollupConfig = SerializeRollupConfig;
  exports.SerializeRollupEnterChallenge = SerializeRollupEnterChallenge;
  exports.SerializeRollupRevert = SerializeRollupRevert;
  exports.SerializeRollupSubmitBlock = SerializeRollupSubmitBlock;
  exports.SerializeSUDTArgs = SerializeSUDTArgs;
  exports.SerializeSUDTQuery = SerializeSUDTQuery;
  exports.SerializeSUDTTransfer = SerializeSUDTTransfer;
  exports.SerializeScript = SerializeScript;
  exports.SerializeScriptOpt = SerializeScriptOpt;
  exports.SerializeScriptVec = SerializeScriptVec;
  exports.SerializeSetMapping = SerializeSetMapping;
  exports.SerializeStakeLockArgs = SerializeStakeLockArgs;
  exports.SerializeSubmitTransactions = SerializeSubmitTransactions;
  exports.SerializeSubmitWithdrawals = SerializeSubmitWithdrawals;
  exports.SerializeTransaction = SerializeTransaction;
  exports.SerializeTransactionVec = SerializeTransactionVec;
  exports.SerializeUint128 = SerializeUint128;
  exports.SerializeUint16 = SerializeUint16;
  exports.SerializeUint256 = SerializeUint256;
  exports.SerializeUint32 = SerializeUint32;
  exports.SerializeUint32Vec = SerializeUint32Vec;
  exports.SerializeUint64 = SerializeUint64;
  exports.SerializeUncleBlock = SerializeUncleBlock;
  exports.SerializeUncleBlockVec = SerializeUncleBlockVec;
  exports.SerializeUnlockCustodianViaRevertWitness = SerializeUnlockCustodianViaRevertWitness;
  exports.SerializeUnlockWithdrawalViaFinalize = SerializeUnlockWithdrawalViaFinalize;
  exports.SerializeUnlockWithdrawalViaRevert = SerializeUnlockWithdrawalViaRevert;
  exports.SerializeUnlockWithdrawalWitness = SerializeUnlockWithdrawalWitness;
  exports.SerializeWithdrawalLockArgs = SerializeWithdrawalLockArgs;
  exports.SerializeWithdrawalRequest = SerializeWithdrawalRequest;
  exports.SerializeWithdrawalRequestVec = SerializeWithdrawalRequestVec;
  exports.SerializeWitnessArgs = SerializeWitnessArgs;
  exports.SetMapping = SetMapping;
  exports.StakeLockArgs = StakeLockArgs;
  exports.SubmitTransactions = SubmitTransactions;
  exports.SubmitWithdrawals = SubmitWithdrawals;
  exports.Transaction = Transaction;
  exports.TransactionVec = TransactionVec;
  exports.Uint128 = Uint128;
  exports.Uint16 = Uint16;
  exports.Uint256 = Uint256;
  exports.Uint32 = Uint32;
  exports.Uint32Vec = Uint32Vec;
  exports.Uint64 = Uint64;
  exports.UncleBlock = UncleBlock;
  exports.UncleBlockVec = UncleBlockVec;
  exports.UnlockCustodianViaRevertWitness = UnlockCustodianViaRevertWitness;
  exports.UnlockWithdrawalViaFinalize = UnlockWithdrawalViaFinalize;
  exports.UnlockWithdrawalViaRevert = UnlockWithdrawalViaRevert;
  exports.UnlockWithdrawalWitness = UnlockWithdrawalWitness;
  exports.WithdrawalLockArgs = WithdrawalLockArgs;
  exports.WithdrawalRequest = WithdrawalRequest;
  exports.WithdrawalRequestVec = WithdrawalRequestVec;
  exports.WitnessArgs = WitnessArgs;

  Object.defineProperty(exports, '__esModule', { value: true });

}));
