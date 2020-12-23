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

export class Byte32Opt {
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
    return new Byte32(this.view.buffer, { validate: false });
  }

  hasValue() {
    return this.view.byteLength > 0;
  }
}

export function SerializeByte32Opt(value) {
  if (value) {
    return SerializeByte32(value);
  } else {
    return new ArrayBuffer(0);
  }
}

export class Byte20 {
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

export function SerializeByte20(value) {
  const buffer = assertArrayBuffer(value);
  assertDataLength(buffer.byteLength, 20);
  return buffer;
}

export class Signature {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, 65);
  }

  indexAt(i) {
    return this.view.getUint8(i);
  }

  raw() {
    return this.view.buffer;
  }

  static size() {
    return 65;
  }
}

export function SerializeSignature(value) {
  const buffer = assertArrayBuffer(value);
  assertDataLength(buffer.byteLength, 65);
  return buffer;
}

export class BlockMerkleState {
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

export function SerializeBlockMerkleState(value) {
  const array = new Uint8Array(0 + Byte32.size() + Uint64.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeByte32(value.merkle_root)), 0);
  array.set(new Uint8Array(SerializeUint64(value.count)), 0 + Byte32.size());
  return array.buffer;
}

export class AccountMerkleState {
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

export function SerializeAccountMerkleState(value) {
  const array = new Uint8Array(0 + Byte32.size() + Uint32.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeByte32(value.merkle_root)), 0);
  array.set(new Uint8Array(SerializeUint32(value.count)), 0 + Byte32.size());
  return array.buffer;
}

export class GlobalState {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new AccountMerkleState(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new BlockMerkleState(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new Byte32(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
    new Status(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
  }

  getAccount() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new AccountMerkleState(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getBlock() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new BlockMerkleState(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getRevertedBlockRoot() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getLastFinalizedBlockNumber() {
    const start = 16;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getStatus() {
    const start = 20;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Status(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeGlobalState(value) {
  const buffers = [];
  buffers.push(SerializeAccountMerkleState(value.account));
  buffers.push(SerializeBlockMerkleState(value.block));
  buffers.push(SerializeByte32(value.reverted_block_root));
  buffers.push(SerializeUint64(value.last_finalized_block_number));
  buffers.push(SerializeStatus(value.status));
  return serializeTable(buffers);
}

export class Status {
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
      new Running(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 1:
      new Reverting(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }

  unionType() {
    const t = this.view.getUint32(0, true);
    switch (t) {
    case 0:
      return "Running";
    case 1:
      return "Reverting";
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }

  value() {
    const t = this.view.getUint32(0, true);
    switch (t) {
    case 0:
      return new Running(this.view.buffer.slice(4), { validate: false });
    case 1:
      return new Reverting(this.view.buffer.slice(4), { validate: false });
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }
}

export function SerializeStatus(value) {
  switch (value.type) {
  case "Running":
    {
      const itemBuffer = SerializeRunning(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 0, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "Reverting":
    {
      const itemBuffer = SerializeReverting(value.value);
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

export class Running {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
  }

}

export function SerializeRunning(value) {
  const buffers = [];
  return serializeTable(buffers);
}

export class Reverting {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getNextBlockNumber() {
    return new Uint64(this.view.buffer.slice(0, 0 + Uint64.size()), { validate: false });
  }

  getChallengerId() {
    return new Uint32(this.view.buffer.slice(0 + Uint64.size(), 0 + Uint64.size() + Uint32.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, Reverting.size());
    this.getNextBlockNumber().validate(compatible);
    this.getChallengerId().validate(compatible);
  }
  static size() {
    return 0 + Uint64.size() + Uint32.size();
  }
}

export function SerializeReverting(value) {
  const array = new Uint8Array(0 + Uint64.size() + Uint32.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeUint64(value.next_block_number)), 0);
  array.set(new Uint8Array(SerializeUint32(value.challenger_id)), 0 + Uint64.size());
  return array.buffer;
}

export class RawL2Transaction {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Uint32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Uint32(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new Uint32(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    new Bytes(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
  }

  getFromId() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getToId() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getNonce() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getArgs() {
    const start = 16;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeRawL2Transaction(value) {
  const buffers = [];
  buffers.push(SerializeUint32(value.from_id));
  buffers.push(SerializeUint32(value.to_id));
  buffers.push(SerializeUint32(value.nonce));
  buffers.push(SerializeBytes(value.args));
  return serializeTable(buffers);
}

export class L2Transaction {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new RawL2Transaction(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Signature(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
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
    return new Signature(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeL2Transaction(value) {
  const buffers = [];
  buffers.push(SerializeRawL2Transaction(value.raw));
  buffers.push(SerializeSignature(value.signature));
  return serializeTable(buffers);
}

export class L2TransactionVec {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    for (let i = 0; i < len(offsets) - 1; i++) {
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

export function SerializeL2TransactionVec(value) {
  return serializeTable(value.map(item => SerializeL2Transaction(item)));
}

export class RawL2Block {
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
    new Byte32(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
    new AccountMerkleState(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
    new AccountMerkleState(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
    new SubmitTransactions(this.view.buffer.slice(offsets[6], offsets[7]), { validate: false }).validate();
    new Byte32(this.view.buffer.slice(offsets[7], offsets[8]), { validate: false }).validate();
  }

  getNumber() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getAggregatorId() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getStakeCellOwnerLockHash() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getTimestamp() {
    const start = 16;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getPrevAccount() {
    const start = 20;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new AccountMerkleState(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getPostAccount() {
    const start = 24;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new AccountMerkleState(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getSubmitTransactions() {
    const start = 28;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new SubmitTransactions(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getWithdrawalRequestsRoot() {
    const start = 32;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeRawL2Block(value) {
  const buffers = [];
  buffers.push(SerializeUint64(value.number));
  buffers.push(SerializeUint32(value.aggregator_id));
  buffers.push(SerializeByte32(value.stake_cell_owner_lock_hash));
  buffers.push(SerializeUint64(value.timestamp));
  buffers.push(SerializeAccountMerkleState(value.prev_account));
  buffers.push(SerializeAccountMerkleState(value.post_account));
  buffers.push(SerializeSubmitTransactions(value.submit_transactions));
  buffers.push(SerializeByte32(value.withdrawal_requests_root));
  return serializeTable(buffers);
}

export class L2Block {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new RawL2Block(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Signature(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new KVPairVec(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    new Bytes(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
    new L2TransactionVec(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
    new Bytes(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
    new WithdrawalRequestVec(this.view.buffer.slice(offsets[6], offsets[7]), { validate: false }).validate();
  }

  getRaw() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new RawL2Block(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getSignature() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Signature(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getKvState() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new KVPairVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getKvStateProof() {
    const start = 16;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getTransactions() {
    const start = 20;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new L2TransactionVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getBlockProof() {
    const start = 24;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getWithdrawalRequests() {
    const start = 28;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new WithdrawalRequestVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeL2Block(value) {
  const buffers = [];
  buffers.push(SerializeRawL2Block(value.raw));
  buffers.push(SerializeSignature(value.signature));
  buffers.push(SerializeKVPairVec(value.kv_state));
  buffers.push(SerializeBytes(value.kv_state_proof));
  buffers.push(SerializeL2TransactionVec(value.transactions));
  buffers.push(SerializeBytes(value.block_proof));
  buffers.push(SerializeWithdrawalRequestVec(value.withdrawal_requests));
  return serializeTable(buffers);
}

export class DepositionRequest {
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
    new Script(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    new Script(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
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

  getSudtScript() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getScript() {
    const start = 16;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeDepositionRequest(value) {
  const buffers = [];
  buffers.push(SerializeUint64(value.capacity));
  buffers.push(SerializeUint128(value.amount));
  buffers.push(SerializeScript(value.sudt_script));
  buffers.push(SerializeScript(value.script));
  return serializeTable(buffers);
}

export class RawWithdrawalRequest {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getNonce() {
    return new Uint32(this.view.buffer.slice(0, 0 + Uint32.size()), { validate: false });
  }

  getCapacity() {
    return new Uint64(this.view.buffer.slice(0 + Uint32.size(), 0 + Uint32.size() + Uint64.size()), { validate: false });
  }

  getAmount() {
    return new Uint128(this.view.buffer.slice(0 + Uint32.size() + Uint64.size(), 0 + Uint32.size() + Uint64.size() + Uint128.size()), { validate: false });
  }

  getSudtScriptHash() {
    return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint128.size(), 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size()), { validate: false });
  }

  getAccountScriptHash() {
    return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size(), 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size()), { validate: false });
  }

  getSellAmount() {
    return new Uint128(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size(), 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size()), { validate: false });
  }

  getSellCapacity() {
    return new Uint64(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size(), 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size() + Uint64.size()), { validate: false });
  }

  getOwnerLockHash() {
    return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size() + Uint64.size(), 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size()), { validate: false });
  }

  getPaymentLockHash() {
    return new Byte32(this.view.buffer.slice(0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size(), 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size() + Byte32.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, RawWithdrawalRequest.size());
    this.getNonce().validate(compatible);
    this.getCapacity().validate(compatible);
    this.getAmount().validate(compatible);
    this.getSudtScriptHash().validate(compatible);
    this.getAccountScriptHash().validate(compatible);
    this.getSellAmount().validate(compatible);
    this.getSellCapacity().validate(compatible);
    this.getOwnerLockHash().validate(compatible);
    this.getPaymentLockHash().validate(compatible);
  }
  static size() {
    return 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size() + Byte32.size();
  }
}

export function SerializeRawWithdrawalRequest(value) {
  const array = new Uint8Array(0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size() + Byte32.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeUint32(value.nonce)), 0);
  array.set(new Uint8Array(SerializeUint64(value.capacity)), 0 + Uint32.size());
  array.set(new Uint8Array(SerializeUint128(value.amount)), 0 + Uint32.size() + Uint64.size());
  array.set(new Uint8Array(SerializeByte32(value.sudt_script_hash)), 0 + Uint32.size() + Uint64.size() + Uint128.size());
  array.set(new Uint8Array(SerializeByte32(value.account_script_hash)), 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size());
  array.set(new Uint8Array(SerializeUint128(value.sell_amount)), 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size());
  array.set(new Uint8Array(SerializeUint64(value.sell_capacity)), 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size());
  array.set(new Uint8Array(SerializeByte32(value.owner_lock_hash)), 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size() + Uint64.size());
  array.set(new Uint8Array(SerializeByte32(value.payment_lock_hash)), 0 + Uint32.size() + Uint64.size() + Uint128.size() + Byte32.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size());
  return array.buffer;
}

export class WithdrawalRequestVec {
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
    const requiredByteLength = this.length() * WithdrawalRequest.size() + 4;
    assertDataLength(this.view.byteLength, requiredByteLength);
    for (let i = 0; i < 0; i++) {
      const item = this.indexAt(i);
      item.validate(compatible);
    }
  }

  indexAt(i) {
    return new WithdrawalRequest(this.view.buffer.slice(4 + i * WithdrawalRequest.size(), 4 + (i + 1) * WithdrawalRequest.size()), { validate: false });
  }

  length() {
    return this.view.getUint32(0, true);
  }
}

export function SerializeWithdrawalRequestVec(value) {
  const array = new Uint8Array(4 + WithdrawalRequest.size() * value.length);
  (new DataView(array.buffer)).setUint32(0, value.length, true);
  for (let i = 0; i < value.length; i++) {
    const itemBuffer = SerializeWithdrawalRequest(value[i]);
    array.set(new Uint8Array(itemBuffer), 4 + i * WithdrawalRequest.size());
  }
  return array.buffer;
}

export class WithdrawalRequest {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getRaw() {
    return new RawWithdrawalRequest(this.view.buffer.slice(0, 0 + RawWithdrawalRequest.size()), { validate: false });
  }

  getSignature() {
    return new Signature(this.view.buffer.slice(0 + RawWithdrawalRequest.size(), 0 + RawWithdrawalRequest.size() + Signature.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, WithdrawalRequest.size());
    this.getRaw().validate(compatible);
    this.getSignature().validate(compatible);
  }
  static size() {
    return 0 + RawWithdrawalRequest.size() + Signature.size();
  }
}

export function SerializeWithdrawalRequest(value) {
  const array = new Uint8Array(0 + RawWithdrawalRequest.size() + Signature.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeRawWithdrawalRequest(value.raw)), 0);
  array.set(new Uint8Array(SerializeSignature(value.signature)), 0 + RawWithdrawalRequest.size());
  return array.buffer;
}

export class SubmitTransactions {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Byte32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Uint32(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new Byte32Vec(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
  }

  getTxWitnessRoot() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getTxCount() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getCompactedPostRootList() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeSubmitTransactions(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.tx_witness_root));
  buffers.push(SerializeUint32(value.tx_count));
  buffers.push(SerializeByte32Vec(value.compacted_post_root_list));
  return serializeTable(buffers);
}

export class KVPair {
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
  }

  getK() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getV() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeKVPair(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.k));
  buffers.push(SerializeByte32(value.v));
  return serializeTable(buffers);
}

export class KVPairVec {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    for (let i = 0; i < len(offsets) - 1; i++) {
      new KVPair(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
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
    return new KVPair(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeKVPairVec(value) {
  return serializeTable(value.map(item => SerializeKVPair(item)));
}

export class BlockInfo {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getAggregatorId() {
    return new Uint32(this.view.buffer.slice(0, 0 + Uint32.size()), { validate: false });
  }

  getNumber() {
    return new Uint64(this.view.buffer.slice(0 + Uint32.size(), 0 + Uint32.size() + Uint64.size()), { validate: false });
  }

  getTimestamp() {
    return new Uint64(this.view.buffer.slice(0 + Uint32.size() + Uint64.size(), 0 + Uint32.size() + Uint64.size() + Uint64.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, BlockInfo.size());
    this.getAggregatorId().validate(compatible);
    this.getNumber().validate(compatible);
    this.getTimestamp().validate(compatible);
  }
  static size() {
    return 0 + Uint32.size() + Uint64.size() + Uint64.size();
  }
}

export function SerializeBlockInfo(value) {
  const array = new Uint8Array(0 + Uint32.size() + Uint64.size() + Uint64.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeUint32(value.aggregator_id)), 0);
  array.set(new Uint8Array(SerializeUint64(value.number)), 0 + Uint32.size());
  array.set(new Uint8Array(SerializeUint64(value.timestamp)), 0 + Uint32.size() + Uint64.size());
  return array.buffer;
}

export class DepositionLockArgs {
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
    const offset_end = this.view.byteLength;
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeDepositionLockArgs(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.owner_lock_hash));
  buffers.push(SerializeScript(value.layer2_lock));
  buffers.push(SerializeUint64(value.cancel_timeout));
  return serializeTable(buffers);
}

export class CustodianLockArgs {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new DepositionLockArgs(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Byte32(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
  }

  getDepositionLockArgs() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new DepositionLockArgs(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getDepositionBlockHash() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getDepositionBlockNumber() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeCustodianLockArgs(value) {
  const buffers = [];
  buffers.push(SerializeDepositionLockArgs(value.deposition_lock_args));
  buffers.push(SerializeByte32(value.deposition_block_hash));
  buffers.push(SerializeUint64(value.deposition_block_number));
  return serializeTable(buffers);
}

export class UnlockCustodianViaRevert {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Bytes(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Byte32(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
  }

  getBlockProof() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getDepositionLockHash() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeUnlockCustodianViaRevert(value) {
  const buffers = [];
  buffers.push(SerializeBytes(value.block_proof));
  buffers.push(SerializeByte32(value.deposition_lock_hash));
  return serializeTable(buffers);
}

export class WithdrawalLockArgs {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getDepositionBlockHash() {
    return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
  }

  getDepositionBlockNumber() {
    return new Uint64(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Uint64.size()), { validate: false });
  }

  getWithdrawalBlockHash() {
    return new Byte32(this.view.buffer.slice(0 + Byte32.size() + Uint64.size(), 0 + Byte32.size() + Uint64.size() + Byte32.size()), { validate: false });
  }

  getWithdrawalBlockNumber() {
    return new Uint64(this.view.buffer.slice(0 + Byte32.size() + Uint64.size() + Byte32.size(), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size()), { validate: false });
  }

  getSudtScriptHash() {
    return new Byte32(this.view.buffer.slice(0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size(), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size()), { validate: false });
  }

  getSellAmount() {
    return new Uint128(this.view.buffer.slice(0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size(), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size()), { validate: false });
  }

  getSellCapacity() {
    return new Uint64(this.view.buffer.slice(0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size(), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size() + Uint64.size()), { validate: false });
  }

  getOwnerLockHash() {
    return new Byte32(this.view.buffer.slice(0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size() + Uint64.size(), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size()), { validate: false });
  }

  getPaymentLockHash() {
    return new Byte32(this.view.buffer.slice(0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size(), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size() + Byte32.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, WithdrawalLockArgs.size());
    this.getDepositionBlockHash().validate(compatible);
    this.getDepositionBlockNumber().validate(compatible);
    this.getWithdrawalBlockHash().validate(compatible);
    this.getWithdrawalBlockNumber().validate(compatible);
    this.getSudtScriptHash().validate(compatible);
    this.getSellAmount().validate(compatible);
    this.getSellCapacity().validate(compatible);
    this.getOwnerLockHash().validate(compatible);
    this.getPaymentLockHash().validate(compatible);
  }
  static size() {
    return 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size() + Byte32.size();
  }
}

export function SerializeWithdrawalLockArgs(value) {
  const array = new Uint8Array(0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size() + Byte32.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeByte32(value.deposition_block_hash)), 0);
  array.set(new Uint8Array(SerializeUint64(value.deposition_block_number)), 0 + Byte32.size());
  array.set(new Uint8Array(SerializeByte32(value.withdrawal_block_hash)), 0 + Byte32.size() + Uint64.size());
  array.set(new Uint8Array(SerializeUint64(value.withdrawal_block_number)), 0 + Byte32.size() + Uint64.size() + Byte32.size());
  array.set(new Uint8Array(SerializeByte32(value.sudt_script_hash)), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size());
  array.set(new Uint8Array(SerializeUint128(value.sell_amount)), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size());
  array.set(new Uint8Array(SerializeUint64(value.sell_capacity)), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size());
  array.set(new Uint8Array(SerializeByte32(value.owner_lock_hash)), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size() + Uint64.size());
  array.set(new Uint8Array(SerializeByte32(value.payment_lock_hash)), 0 + Byte32.size() + Uint64.size() + Byte32.size() + Uint64.size() + Byte32.size() + Uint128.size() + Uint64.size() + Byte32.size());
  return array.buffer;
}

export class UnlockWithdrawal {
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
    case 2:
      new UnlockWithdrawalViaTrade(this.view.buffer.slice(4), { validate: false }).validate();
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
    case 2:
      return "UnlockWithdrawalViaTrade";
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
    case 2:
      return new UnlockWithdrawalViaTrade(this.view.buffer.slice(4), { validate: false });
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }
}

export function SerializeUnlockWithdrawal(value) {
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
  case "UnlockWithdrawalViaTrade":
    {
      const itemBuffer = SerializeUnlockWithdrawalViaTrade(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 2, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  default:
    throw new Error(`Invalid type: ${value.type}`);
  }

}

export class UnlockWithdrawalViaFinalize {
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

  getBlockProof() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeUnlockWithdrawalViaFinalize(value) {
  const buffers = [];
  buffers.push(SerializeBytes(value.block_proof));
  return serializeTable(buffers);
}

export class UnlockWithdrawalViaRevert {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Bytes(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Byte32(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
  }

  getBlockProof() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getCustodianLockHash() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeUnlockWithdrawalViaRevert(value) {
  const buffers = [];
  buffers.push(SerializeBytes(value.block_proof));
  buffers.push(SerializeByte32(value.custodian_lock_hash));
  return serializeTable(buffers);
}

export class UnlockWithdrawalViaTrade {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Script(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
  }

  getOwnerLock() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeUnlockWithdrawalViaTrade(value) {
  const buffers = [];
  buffers.push(SerializeScript(value.owner_lock));
  return serializeTable(buffers);
}

export class StakeLockArgs {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getOwnerLockHash() {
    return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
  }

  getSigningPubkeyHash() {
    return new Byte20(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Byte20.size()), { validate: false });
  }

  getStakeBlockNumber() {
    return new Uint64(this.view.buffer.slice(0 + Byte32.size() + Byte20.size(), 0 + Byte32.size() + Byte20.size() + Uint64.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, StakeLockArgs.size());
    this.getOwnerLockHash().validate(compatible);
    this.getSigningPubkeyHash().validate(compatible);
    this.getStakeBlockNumber().validate(compatible);
  }
  static size() {
    return 0 + Byte32.size() + Byte20.size() + Uint64.size();
  }
}

export function SerializeStakeLockArgs(value) {
  const array = new Uint8Array(0 + Byte32.size() + Byte20.size() + Uint64.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeByte32(value.owner_lock_hash)), 0);
  array.set(new Uint8Array(SerializeByte20(value.signing_pubkey_hash)), 0 + Byte32.size());
  array.set(new Uint8Array(SerializeUint64(value.stake_block_number)), 0 + Byte32.size() + Byte20.size());
  return array.buffer;
}

export class MetaContractArgs {
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
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }

  unionType() {
    const t = this.view.getUint32(0, true);
    switch (t) {
    case 0:
      return "CreateAccount";
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }

  value() {
    const t = this.view.getUint32(0, true);
    switch (t) {
    case 0:
      return new CreateAccount(this.view.buffer.slice(4), { validate: false });
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }
}

export function SerializeMetaContractArgs(value) {
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
  default:
    throw new Error(`Invalid type: ${value.type}`);
  }

}

export class CreateAccount {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Script(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
  }

  getScript() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Script(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeCreateAccount(value) {
  const buffers = [];
  buffers.push(SerializeScript(value.script));
  return serializeTable(buffers);
}

export class SUDTArgs {
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

export function SerializeSUDTArgs(value) {
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

export class SUDTQuery {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getAccountId() {
    return new Uint32(this.view.buffer.slice(0, 0 + Uint32.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, SUDTQuery.size());
    this.getAccountId().validate(compatible);
  }
  static size() {
    return 0 + Uint32.size();
  }
}

export function SerializeSUDTQuery(value) {
  const array = new Uint8Array(0 + Uint32.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeUint32(value.account_id)), 0);
  return array.buffer;
}

export class SUDTTransfer {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getTo() {
    return new Uint32(this.view.buffer.slice(0, 0 + Uint32.size()), { validate: false });
  }

  getAmount() {
    return new Uint128(this.view.buffer.slice(0 + Uint32.size(), 0 + Uint32.size() + Uint128.size()), { validate: false });
  }

  getFee() {
    return new Uint128(this.view.buffer.slice(0 + Uint32.size() + Uint128.size(), 0 + Uint32.size() + Uint128.size() + Uint128.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, SUDTTransfer.size());
    this.getTo().validate(compatible);
    this.getAmount().validate(compatible);
    this.getFee().validate(compatible);
  }
  static size() {
    return 0 + Uint32.size() + Uint128.size() + Uint128.size();
  }
}

export function SerializeSUDTTransfer(value) {
  const array = new Uint8Array(0 + Uint32.size() + Uint128.size() + Uint128.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeUint32(value.to)), 0);
  array.set(new Uint8Array(SerializeUint128(value.amount)), 0 + Uint32.size());
  array.set(new Uint8Array(SerializeUint128(value.fee)), 0 + Uint32.size() + Uint128.size());
  return array.buffer;
}

export class StartChallenge {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getBlockHash() {
    return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
  }

  getBlockNumber() {
    return new Uint64(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + Uint64.size()), { validate: false });
  }

  getTxIndex() {
    return new Uint32(this.view.buffer.slice(0 + Byte32.size() + Uint64.size(), 0 + Byte32.size() + Uint64.size() + Uint32.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, StartChallenge.size());
    this.getBlockHash().validate(compatible);
    this.getBlockNumber().validate(compatible);
    this.getTxIndex().validate(compatible);
  }
  static size() {
    return 0 + Byte32.size() + Uint64.size() + Uint32.size();
  }
}

export function SerializeStartChallenge(value) {
  const array = new Uint8Array(0 + Byte32.size() + Uint64.size() + Uint32.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeByte32(value.block_hash)), 0);
  array.set(new Uint8Array(SerializeUint64(value.block_number)), 0 + Byte32.size());
  array.set(new Uint8Array(SerializeUint32(value.tx_index)), 0 + Byte32.size() + Uint64.size());
  return array.buffer;
}

export class ScriptVec {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    for (let i = 0; i < len(offsets) - 1; i++) {
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

export function SerializeScriptVec(value) {
  return serializeTable(value.map(item => SerializeScript(item)));
}

export class CancelChallenge {
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
    new KVPairVec(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    new Bytes(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
    new ScriptVec(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
    new Byte32(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
    new Bytes(this.view.buffer.slice(offsets[6], offsets[7]), { validate: false }).validate();
    new Bytes(this.view.buffer.slice(offsets[7], offsets[8]), { validate: false }).validate();
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

  getKvState() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new KVPairVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getKvStateProof() {
    const start = 16;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getScripts() {
    const start = 20;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new ScriptVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getReturnDataHash() {
    const start = 24;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getTxProof() {
    const start = 28;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getBlockProof() {
    const start = 32;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeCancelChallenge(value) {
  const buffers = [];
  buffers.push(SerializeRawL2Block(value.raw_l2block));
  buffers.push(SerializeL2Transaction(value.l2tx));
  buffers.push(SerializeKVPairVec(value.kv_state));
  buffers.push(SerializeBytes(value.kv_state_proof));
  buffers.push(SerializeScriptVec(value.scripts));
  buffers.push(SerializeByte32(value.return_data_hash));
  buffers.push(SerializeBytes(value.tx_proof));
  buffers.push(SerializeBytes(value.block_proof));
  return serializeTable(buffers);
}

export class HeaderInfo {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getNumber() {
    return new Uint64(this.view.buffer.slice(0, 0 + Uint64.size()), { validate: false });
  }

  getBlockHash() {
    return new Byte32(this.view.buffer.slice(0 + Uint64.size(), 0 + Uint64.size() + Byte32.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, HeaderInfo.size());
    this.getNumber().validate(compatible);
    this.getBlockHash().validate(compatible);
  }
  static size() {
    return 0 + Uint64.size() + Byte32.size();
  }
}

export function SerializeHeaderInfo(value) {
  const array = new Uint8Array(0 + Uint64.size() + Byte32.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeUint64(value.number)), 0);
  array.set(new Uint8Array(SerializeByte32(value.block_hash)), 0 + Uint64.size());
  return array.buffer;
}

export class Uint32 {
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

export function SerializeUint32(value) {
  const buffer = assertArrayBuffer(value);
  assertDataLength(buffer.byteLength, 4);
  return buffer;
}

export class Uint64 {
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
    return this.view.getUint64(0, true);
  }

  static size() {
    return 8;
  }
}

export function SerializeUint64(value) {
  const buffer = assertArrayBuffer(value);
  assertDataLength(buffer.byteLength, 8);
  return buffer;
}

export class Uint128 {
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

export function SerializeUint128(value) {
  const buffer = assertArrayBuffer(value);
  assertDataLength(buffer.byteLength, 16);
  return buffer;
}

export class Byte32 {
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

export function SerializeByte32(value) {
  const buffer = assertArrayBuffer(value);
  assertDataLength(buffer.byteLength, 32);
  return buffer;
}

export class Uint256 {
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

export function SerializeUint256(value) {
  const buffer = assertArrayBuffer(value);
  assertDataLength(buffer.byteLength, 32);
  return buffer;
}

export class Bytes {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    if (this.view.byteLength < 4) {
      dataLengthError(this.view.byteLength, ">4")
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

export function SerializeBytes(value) {
  const item = assertArrayBuffer(value);
  const array = new Uint8Array(4 + item.byteLength);
  (new DataView(array.buffer)).setUint32(0, item.byteLength, true);
  array.set(new Uint8Array(item), 4);
  return array.buffer;
}

export class BytesOpt {
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

export function SerializeBytesOpt(value) {
  if (value) {
    return SerializeBytes(value);
  } else {
    return new ArrayBuffer(0);
  }
}

export class BytesVec {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    for (let i = 0; i < len(offsets) - 1; i++) {
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

export function SerializeBytesVec(value) {
  return serializeTable(value.map(item => SerializeBytes(item)));
}

export class Byte32Vec {
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

export function SerializeByte32Vec(value) {
  const array = new Uint8Array(4 + Byte32.size() * value.length);
  (new DataView(array.buffer)).setUint32(0, value.length, true);
  for (let i = 0; i < value.length; i++) {
    const itemBuffer = SerializeByte32(value[i]);
    array.set(new Uint8Array(itemBuffer), 4 + i * Byte32.size());
  }
  return array.buffer;
}

export class ScriptOpt {
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

export function SerializeScriptOpt(value) {
  if (value) {
    return SerializeScript(value);
  } else {
    return new ArrayBuffer(0);
  }
}

export class ProposalShortId {
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

export function SerializeProposalShortId(value) {
  const buffer = assertArrayBuffer(value);
  assertDataLength(buffer.byteLength, 10);
  return buffer;
}

export class UncleBlockVec {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    for (let i = 0; i < len(offsets) - 1; i++) {
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

export function SerializeUncleBlockVec(value) {
  return serializeTable(value.map(item => SerializeUncleBlock(item)));
}

export class TransactionVec {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    for (let i = 0; i < len(offsets) - 1; i++) {
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

export function SerializeTransactionVec(value) {
  return serializeTable(value.map(item => SerializeTransaction(item)));
}

export class ProposalShortIdVec {
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

export function SerializeProposalShortIdVec(value) {
  const array = new Uint8Array(4 + ProposalShortId.size() * value.length);
  (new DataView(array.buffer)).setUint32(0, value.length, true);
  for (let i = 0; i < value.length; i++) {
    const itemBuffer = SerializeProposalShortId(value[i]);
    array.set(new Uint8Array(itemBuffer), 4 + i * ProposalShortId.size());
  }
  return array.buffer;
}

export class CellDepVec {
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

export function SerializeCellDepVec(value) {
  const array = new Uint8Array(4 + CellDep.size() * value.length);
  (new DataView(array.buffer)).setUint32(0, value.length, true);
  for (let i = 0; i < value.length; i++) {
    const itemBuffer = SerializeCellDep(value[i]);
    array.set(new Uint8Array(itemBuffer), 4 + i * CellDep.size());
  }
  return array.buffer;
}

export class CellInputVec {
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

export function SerializeCellInputVec(value) {
  const array = new Uint8Array(4 + CellInput.size() * value.length);
  (new DataView(array.buffer)).setUint32(0, value.length, true);
  for (let i = 0; i < value.length; i++) {
    const itemBuffer = SerializeCellInput(value[i]);
    array.set(new Uint8Array(itemBuffer), 4 + i * CellInput.size());
  }
  return array.buffer;
}

export class CellOutputVec {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    for (let i = 0; i < len(offsets) - 1; i++) {
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

export function SerializeCellOutputVec(value) {
  return serializeTable(value.map(item => SerializeCellOutput(item)));
}

export class Script {
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

export function SerializeScript(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.code_hash));
  const hashTypeView = new DataView(new ArrayBuffer(1));
  hashTypeView.setUint8(0, value.hash_type);
  buffers.push(hashTypeView.buffer)
  buffers.push(SerializeBytes(value.args));
  return serializeTable(buffers);
}

export class OutPoint {
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

export function SerializeOutPoint(value) {
  const array = new Uint8Array(0 + Byte32.size() + Uint32.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeByte32(value.tx_hash)), 0);
  array.set(new Uint8Array(SerializeUint32(value.index)), 0 + Byte32.size());
  return array.buffer;
}

export class CellInput {
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

export function SerializeCellInput(value) {
  const array = new Uint8Array(0 + Uint64.size() + OutPoint.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeUint64(value.since)), 0);
  array.set(new Uint8Array(SerializeOutPoint(value.previous_output)), 0 + Uint64.size());
  return array.buffer;
}

export class CellOutput {
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

export function SerializeCellOutput(value) {
  const buffers = [];
  buffers.push(SerializeUint64(value.capacity));
  buffers.push(SerializeScript(value.lock));
  buffers.push(SerializeScriptOpt(value.type_));
  return serializeTable(buffers);
}

export class CellDep {
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

export function SerializeCellDep(value) {
  const array = new Uint8Array(0 + OutPoint.size() + 1);
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeOutPoint(value.out_point)), 0);
  view.setUint8(0 + OutPoint.size(), value.dep_type);
  return array.buffer;
}

export class RawTransaction {
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

export function SerializeRawTransaction(value) {
  const buffers = [];
  buffers.push(SerializeUint32(value.version));
  buffers.push(SerializeCellDepVec(value.cell_deps));
  buffers.push(SerializeByte32Vec(value.header_deps));
  buffers.push(SerializeCellInputVec(value.inputs));
  buffers.push(SerializeCellOutputVec(value.outputs));
  buffers.push(SerializeBytesVec(value.outputs_data));
  return serializeTable(buffers);
}

export class Transaction {
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

export function SerializeTransaction(value) {
  const buffers = [];
  buffers.push(SerializeRawTransaction(value.raw));
  buffers.push(SerializeBytesVec(value.witnesses));
  return serializeTable(buffers);
}

export class RawHeader {
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

  getUnclesHash() {
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
    this.getUnclesHash().validate(compatible);
    this.getDao().validate(compatible);
  }
  static size() {
    return 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size();
  }
}

export function SerializeRawHeader(value) {
  const array = new Uint8Array(0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeUint32(value.version)), 0);
  array.set(new Uint8Array(SerializeUint32(value.compact_target)), 0 + Uint32.size());
  array.set(new Uint8Array(SerializeUint64(value.timestamp)), 0 + Uint32.size() + Uint32.size());
  array.set(new Uint8Array(SerializeUint64(value.number)), 0 + Uint32.size() + Uint32.size() + Uint64.size());
  array.set(new Uint8Array(SerializeUint64(value.epoch)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size());
  array.set(new Uint8Array(SerializeByte32(value.parent_hash)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size());
  array.set(new Uint8Array(SerializeByte32(value.transactions_root)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size());
  array.set(new Uint8Array(SerializeByte32(value.proposals_hash)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size());
  array.set(new Uint8Array(SerializeByte32(value.uncles_hash)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size());
  array.set(new Uint8Array(SerializeByte32(value.dao)), 0 + Uint32.size() + Uint32.size() + Uint64.size() + Uint64.size() + Uint64.size() + Byte32.size() + Byte32.size() + Byte32.size() + Byte32.size());
  return array.buffer;
}

export class Header {
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

export function SerializeHeader(value) {
  const array = new Uint8Array(0 + RawHeader.size() + Uint128.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeRawHeader(value.raw)), 0);
  array.set(new Uint8Array(SerializeUint128(value.nonce)), 0 + RawHeader.size());
  return array.buffer;
}

export class UncleBlock {
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

export function SerializeUncleBlock(value) {
  const buffers = [];
  buffers.push(SerializeHeader(value.header));
  buffers.push(SerializeProposalShortIdVec(value.proposals));
  return serializeTable(buffers);
}

export class Block {
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

export function SerializeBlock(value) {
  const buffers = [];
  buffers.push(SerializeHeader(value.header));
  buffers.push(SerializeUncleBlockVec(value.uncles));
  buffers.push(SerializeTransactionVec(value.transactions));
  buffers.push(SerializeProposalShortIdVec(value.proposals));
  return serializeTable(buffers);
}

export class CellbaseWitness {
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

export function SerializeCellbaseWitness(value) {
  const buffers = [];
  buffers.push(SerializeScript(value.lock));
  buffers.push(SerializeBytes(value.message));
  return serializeTable(buffers);
}

export class WitnessArgs {
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

export function SerializeWitnessArgs(value) {
  const buffers = [];
  buffers.push(SerializeBytesOpt(value.lock));
  buffers.push(SerializeBytesOpt(value.input_type));
  buffers.push(SerializeBytesOpt(value.output_type));
  return serializeTable(buffers);
}

