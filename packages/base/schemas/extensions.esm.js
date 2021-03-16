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

export class BoolOpt {
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
    return new Bool(this.view.buffer, { validate: false });
  }

  hasValue() {
    return this.view.byteLength > 0;
  }
}

export function SerializeBoolOpt(value) {
  if (value) {
    return SerializeBool(value);
  } else {
    return new ArrayBuffer(0);
  }
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

export class Bool {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, 1);
  }

  indexAt(i) {
    return this.view.getUint8(i);
  }

  raw() {
    return this.view.buffer;
  }

  static size() {
    return 1;
  }
}

export function SerializeBool(value) {
  const buffer = assertArrayBuffer(value);
  assertDataLength(buffer.byteLength, 1);
  return buffer;
}

export class BeUint32 {
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

export function SerializeBeUint32(value) {
  const buffer = assertArrayBuffer(value);
  assertDataLength(buffer.byteLength, 4);
  return buffer;
}

export class BeUint64 {
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

export function SerializeBeUint64(value) {
  const buffer = assertArrayBuffer(value);
  assertDataLength(buffer.byteLength, 8);
  return buffer;
}

export class Uint32Vec {
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

export function SerializeUint32Vec(value) {
  const array = new Uint8Array(4 + Uint32.size() * value.length);
  (new DataView(array.buffer)).setUint32(0, value.length, true);
  for (let i = 0; i < value.length; i++) {
    const itemBuffer = SerializeUint32(value[i]);
    array.set(new Uint8Array(itemBuffer), 4 + i * Uint32.size());
  }
  return array.buffer;
}

export class Uint64Vec {
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
    const requiredByteLength = this.length() * Uint64.size() + 4;
    assertDataLength(this.view.byteLength, requiredByteLength);
    for (let i = 0; i < 0; i++) {
      const item = this.indexAt(i);
      item.validate(compatible);
    }
  }

  indexAt(i) {
    return new Uint64(this.view.buffer.slice(4 + i * Uint64.size(), 4 + (i + 1) * Uint64.size()), { validate: false });
  }

  length() {
    return this.view.getUint32(0, true);
  }
}

export function SerializeUint64Vec(value) {
  const array = new Uint8Array(4 + Uint64.size() * value.length);
  (new DataView(array.buffer)).setUint32(0, value.length, true);
  for (let i = 0; i < value.length; i++) {
    const itemBuffer = SerializeUint64(value[i]);
    array.set(new Uint8Array(itemBuffer), 4 + i * Uint64.size());
  }
  return array.buffer;
}

export class CellOutputOpt {
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
    return new CellOutput(this.view.buffer, { validate: false });
  }

  hasValue() {
    return this.view.byteLength > 0;
  }
}

export function SerializeCellOutputOpt(value) {
  if (value) {
    return SerializeCellOutput(value);
  } else {
    return new ArrayBuffer(0);
  }
}

export class HeaderVec {
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
    const requiredByteLength = this.length() * Header.size() + 4;
    assertDataLength(this.view.byteLength, requiredByteLength);
    for (let i = 0; i < 0; i++) {
      const item = this.indexAt(i);
      item.validate(compatible);
    }
  }

  indexAt(i) {
    return new Header(this.view.buffer.slice(4 + i * Header.size(), 4 + (i + 1) * Header.size()), { validate: false });
  }

  length() {
    return this.view.getUint32(0, true);
  }
}

export function SerializeHeaderVec(value) {
  const array = new Uint8Array(4 + Header.size() * value.length);
  (new DataView(array.buffer)).setUint32(0, value.length, true);
  for (let i = 0; i < value.length; i++) {
    const itemBuffer = SerializeHeader(value[i]);
    array.set(new Uint8Array(itemBuffer), 4 + i * Header.size());
  }
  return array.buffer;
}

export class OutPointVec {
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
    const requiredByteLength = this.length() * OutPoint.size() + 4;
    assertDataLength(this.view.byteLength, requiredByteLength);
    for (let i = 0; i < 0; i++) {
      const item = this.indexAt(i);
      item.validate(compatible);
    }
  }

  indexAt(i) {
    return new OutPoint(this.view.buffer.slice(4 + i * OutPoint.size(), 4 + (i + 1) * OutPoint.size()), { validate: false });
  }

  length() {
    return this.view.getUint32(0, true);
  }
}

export function SerializeOutPointVec(value) {
  const array = new Uint8Array(4 + OutPoint.size() * value.length);
  (new DataView(array.buffer)).setUint32(0, value.length, true);
  for (let i = 0; i < value.length; i++) {
    const itemBuffer = SerializeOutPoint(value[i]);
    array.set(new Uint8Array(itemBuffer), 4 + i * OutPoint.size());
  }
  return array.buffer;
}

export class HeaderView {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Byte32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Header(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
  }

  getHash() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getData() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Header(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeHeaderView(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.hash));
  buffers.push(SerializeHeader(value.data));
  return serializeTable(buffers);
}

export class UncleBlockVecView {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Byte32Vec(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new UncleBlockVec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
  }

  getHashes() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getData() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new UncleBlockVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeUncleBlockVecView(value) {
  const buffers = [];
  buffers.push(SerializeByte32Vec(value.hashes));
  buffers.push(SerializeUncleBlockVec(value.data));
  return serializeTable(buffers);
}

export class TransactionView {
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
    new Transaction(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
  }

  getHash() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getWitnessHash() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getData() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Transaction(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeTransactionView(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.hash));
  buffers.push(SerializeByte32(value.witness_hash));
  buffers.push(SerializeTransaction(value.data));
  return serializeTable(buffers);
}

export class BlockExt {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Uint256(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    new Uint64Vec(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
    new BoolOpt(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
  }

  getTotalDifficulty() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint256(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getTotalUnclesCount() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getReceivedAt() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getTxsFees() {
    const start = 16;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getVerified() {
    const start = 20;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new BoolOpt(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeBlockExt(value) {
  const buffers = [];
  buffers.push(SerializeUint256(value.total_difficulty));
  buffers.push(SerializeUint64(value.total_uncles_count));
  buffers.push(SerializeUint64(value.received_at));
  buffers.push(SerializeUint64Vec(value.txs_fees));
  buffers.push(SerializeBoolOpt(value.verified));
  return serializeTable(buffers);
}

export class EpochExt {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Uint256(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Byte32(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new Uint32(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[6], offsets[7]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[7], offsets[8]), { validate: false }).validate();
  }

  getPreviousEpochHashRate() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint256(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getLastBlockHashInPreviousEpoch() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getCompactTarget() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getNumber() {
    const start = 16;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getBaseBlockReward() {
    const start = 20;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getRemainderReward() {
    const start = 24;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getStartNumber() {
    const start = 28;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getLength() {
    const start = 32;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeEpochExt(value) {
  const buffers = [];
  buffers.push(SerializeUint256(value.previous_epoch_hash_rate));
  buffers.push(SerializeByte32(value.last_block_hash_in_previous_epoch));
  buffers.push(SerializeUint32(value.compact_target));
  buffers.push(SerializeUint64(value.number));
  buffers.push(SerializeUint64(value.base_block_reward));
  buffers.push(SerializeUint64(value.remainder_reward));
  buffers.push(SerializeUint64(value.start_number));
  buffers.push(SerializeUint64(value.length));
  return serializeTable(buffers);
}

export class TransactionKey {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getBlockHash() {
    return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
  }

  getIndex() {
    return new BeUint32(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + BeUint32.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, this.size());
    this.getBlockHash().validate(compatible);
    this.getIndex().validate(compatible);
  }
  static size() {
    return 0 + Byte32.size() + BeUint32.size();
  }
}

export function SerializeTransactionKey(value) {
  const array = new Uint8Array(0 + Byte32.size() + BeUint32.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeByte32(value.block_hash)), 0);
  array.set(new Uint8Array(SerializeBeUint32(value.index)), 0 + Byte32.size());
  return array.buffer;
}

export class TransactionInfo {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Uint64(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new TransactionKey(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
  }

  getBlockNumber() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getBlockEpoch() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getKey() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new TransactionKey(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeTransactionInfo(value) {
  const buffers = [];
  buffers.push(SerializeUint64(value.block_number));
  buffers.push(SerializeUint64(value.block_epoch));
  buffers.push(SerializeTransactionKey(value.key));
  return serializeTable(buffers);
}

export class TransactionMeta {
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
    new Uint64(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    new Uint32(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
    new Bytes(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
    new Bool(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
  }

  getBlockHash() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getBlockNumber() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getEpochNumber() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getLen() {
    const start = 16;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getBits() {
    const start = 20;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getCellbase() {
    const start = 24;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Bool(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeTransactionMeta(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.block_hash));
  buffers.push(SerializeUint64(value.block_number));
  buffers.push(SerializeUint64(value.epoch_number));
  buffers.push(SerializeUint32(value.len));
  buffers.push(SerializeBytes(value.bits));
  buffers.push(SerializeBool(value.cellbase));
  return serializeTable(buffers);
}

export class TransactionPoint {
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
    new Uint32(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
  }

  getTxHash() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getBlockNumber() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getIndex() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeTransactionPoint(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.tx_hash));
  buffers.push(SerializeUint64(value.block_number));
  buffers.push(SerializeUint32(value.index));
  return serializeTable(buffers);
}

export class TransactionPointOpt {
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
    return new TransactionPoint(this.view.buffer, { validate: false });
  }

  hasValue() {
    return this.view.byteLength > 0;
  }
}

export function SerializeTransactionPointOpt(value) {
  if (value) {
    return SerializeTransactionPoint(value);
  } else {
    return new ArrayBuffer(0);
  }
}

export class LockHashCellOutput {
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
    new CellOutputOpt(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
  }

  getLockHash() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getBlockNumber() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getCellOutput() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new CellOutputOpt(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeLockHashCellOutput(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.lock_hash));
  buffers.push(SerializeUint64(value.block_number));
  buffers.push(SerializeCellOutputOpt(value.cell_output));
  return serializeTable(buffers);
}

export class LockHashIndex {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  getLockHash() {
    return new Byte32(this.view.buffer.slice(0, 0 + Byte32.size()), { validate: false });
  }

  getBlockNumber() {
    return new BeUint64(this.view.buffer.slice(0 + Byte32.size(), 0 + Byte32.size() + BeUint64.size()), { validate: false });
  }

  getTxHash() {
    return new Byte32(this.view.buffer.slice(0 + Byte32.size() + BeUint64.size(), 0 + Byte32.size() + BeUint64.size() + Byte32.size()), { validate: false });
  }

  getIndex() {
    return new BeUint32(this.view.buffer.slice(0 + Byte32.size() + BeUint64.size() + Byte32.size(), 0 + Byte32.size() + BeUint64.size() + Byte32.size() + BeUint32.size()), { validate: false });
  }

  validate(compatible = false) {
    assertDataLength(this.view.byteLength, this.size());
    this.getLockHash().validate(compatible);
    this.getBlockNumber().validate(compatible);
    this.getTxHash().validate(compatible);
    this.getIndex().validate(compatible);
  }
  static size() {
    return 0 + Byte32.size() + BeUint64.size() + Byte32.size() + BeUint32.size();
  }
}

export function SerializeLockHashIndex(value) {
  const array = new Uint8Array(0 + Byte32.size() + BeUint64.size() + Byte32.size() + BeUint32.size());
  const view = new DataView(array.buffer);
  array.set(new Uint8Array(SerializeByte32(value.lock_hash)), 0);
  array.set(new Uint8Array(SerializeBeUint64(value.block_number)), 0 + Byte32.size());
  array.set(new Uint8Array(SerializeByte32(value.tx_hash)), 0 + Byte32.size() + BeUint64.size());
  array.set(new Uint8Array(SerializeBeUint32(value.index)), 0 + Byte32.size() + BeUint64.size() + Byte32.size());
  return array.buffer;
}

export class LockHashIndexState {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Uint64(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Byte32(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
  }

  getBlockNumber() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getBlockHash() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeLockHashIndexState(value) {
  const buffers = [];
  buffers.push(SerializeUint64(value.block_number));
  buffers.push(SerializeByte32(value.block_hash));
  return serializeTable(buffers);
}

export class LiveCellOutput {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new CellOutput(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Uint64(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new Bool(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
  }

  getCellOutput() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new CellOutput(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getOutputDataLen() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getCellbase() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Bool(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeLiveCellOutput(value) {
  const buffers = [];
  buffers.push(SerializeCellOutput(value.cell_output));
  buffers.push(SerializeUint64(value.output_data_len));
  buffers.push(SerializeBool(value.cellbase));
  return serializeTable(buffers);
}

export class RelayMessage {
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
      new CompactBlock(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 1:
      new RelayTransactions(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 2:
      new RelayTransactionHashes(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 3:
      new GetRelayTransactions(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 4:
      new GetBlockTransactions(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 5:
      new BlockTransactions(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 6:
      new GetBlockProposal(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 7:
      new BlockProposal(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }

  unionType() {
    const t = this.view.getUint32(0, true);
    switch (t) {
    case 0:
      return "CompactBlock";
    case 1:
      return "RelayTransactions";
    case 2:
      return "RelayTransactionHashes";
    case 3:
      return "GetRelayTransactions";
    case 4:
      return "GetBlockTransactions";
    case 5:
      return "BlockTransactions";
    case 6:
      return "GetBlockProposal";
    case 7:
      return "BlockProposal";
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }

  value() {
    const t = this.view.getUint32(0, true);
    switch (t) {
    case 0:
      return new CompactBlock(this.view.buffer.slice(4), { validate: false });
    case 1:
      return new RelayTransactions(this.view.buffer.slice(4), { validate: false });
    case 2:
      return new RelayTransactionHashes(this.view.buffer.slice(4), { validate: false });
    case 3:
      return new GetRelayTransactions(this.view.buffer.slice(4), { validate: false });
    case 4:
      return new GetBlockTransactions(this.view.buffer.slice(4), { validate: false });
    case 5:
      return new BlockTransactions(this.view.buffer.slice(4), { validate: false });
    case 6:
      return new GetBlockProposal(this.view.buffer.slice(4), { validate: false });
    case 7:
      return new BlockProposal(this.view.buffer.slice(4), { validate: false });
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }
}

export function SerializeRelayMessage(value) {
  switch (value.type) {
  case "CompactBlock":
    {
      const itemBuffer = SerializeCompactBlock(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 0, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "RelayTransactions":
    {
      const itemBuffer = SerializeRelayTransactions(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 1, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "RelayTransactionHashes":
    {
      const itemBuffer = SerializeRelayTransactionHashes(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 2, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "GetRelayTransactions":
    {
      const itemBuffer = SerializeGetRelayTransactions(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 3, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "GetBlockTransactions":
    {
      const itemBuffer = SerializeGetBlockTransactions(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 4, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "BlockTransactions":
    {
      const itemBuffer = SerializeBlockTransactions(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 5, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "GetBlockProposal":
    {
      const itemBuffer = SerializeGetBlockProposal(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 6, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "BlockProposal":
    {
      const itemBuffer = SerializeBlockProposal(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 7, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  default:
    throw new Error(`Invalid type: ${value.type}`);
  }

}

export class CompactBlock {
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
    new IndexTransactionVec(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
    new Byte32Vec(this.view.buffer.slice(offsets[3], offsets[4]), { validate: false }).validate();
    new ProposalShortIdVec(this.view.buffer.slice(offsets[4], offsets[5]), { validate: false }).validate();
  }

  getHeader() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Header(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getShortIds() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new ProposalShortIdVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getPrefilledTransactions() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new IndexTransactionVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getUncles() {
    const start = 16;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getProposals() {
    const start = 20;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new ProposalShortIdVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeCompactBlock(value) {
  const buffers = [];
  buffers.push(SerializeHeader(value.header));
  buffers.push(SerializeProposalShortIdVec(value.short_ids));
  buffers.push(SerializeIndexTransactionVec(value.prefilled_transactions));
  buffers.push(SerializeByte32Vec(value.uncles));
  buffers.push(SerializeProposalShortIdVec(value.proposals));
  return serializeTable(buffers);
}

export class RelayTransaction {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Uint64(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Transaction(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
  }

  getCycles() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getTransaction() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Transaction(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeRelayTransaction(value) {
  const buffers = [];
  buffers.push(SerializeUint64(value.cycles));
  buffers.push(SerializeTransaction(value.transaction));
  return serializeTable(buffers);
}

export class RelayTransactionVec {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    for (let i = 0; i < len(offsets) - 1; i++) {
      new RelayTransaction(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
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
    return new RelayTransaction(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeRelayTransactionVec(value) {
  return serializeTable(value.map(item => SerializeRelayTransaction(item)));
}

export class RelayTransactions {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new RelayTransactionVec(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
  }

  getTransactions() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new RelayTransactionVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeRelayTransactions(value) {
  const buffers = [];
  buffers.push(SerializeRelayTransactionVec(value.transactions));
  return serializeTable(buffers);
}

export class RelayTransactionHashes {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Byte32Vec(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
  }

  getTxHashes() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeRelayTransactionHashes(value) {
  const buffers = [];
  buffers.push(SerializeByte32Vec(value.tx_hashes));
  return serializeTable(buffers);
}

export class GetRelayTransactions {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Byte32Vec(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
  }

  getTxHashes() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeGetRelayTransactions(value) {
  const buffers = [];
  buffers.push(SerializeByte32Vec(value.tx_hashes));
  return serializeTable(buffers);
}

export class GetBlockTransactions {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Byte32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Uint32Vec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new Uint32Vec(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
  }

  getBlockHash() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getIndexes() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getUncleIndexes() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Uint32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeGetBlockTransactions(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.block_hash));
  buffers.push(SerializeUint32Vec(value.indexes));
  buffers.push(SerializeUint32Vec(value.uncle_indexes));
  return serializeTable(buffers);
}

export class BlockTransactions {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Byte32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new TransactionVec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new UncleBlockVec(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
  }

  getBlockHash() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getTransactions() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new TransactionVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getUncles() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new UncleBlockVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeBlockTransactions(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.block_hash));
  buffers.push(SerializeTransactionVec(value.transactions));
  buffers.push(SerializeUncleBlockVec(value.uncles));
  return serializeTable(buffers);
}

export class GetBlockProposal {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Byte32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new ProposalShortIdVec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
  }

  getBlockHash() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getProposals() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new ProposalShortIdVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeGetBlockProposal(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.block_hash));
  buffers.push(SerializeProposalShortIdVec(value.proposals));
  return serializeTable(buffers);
}

export class BlockProposal {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new TransactionVec(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
  }

  getTransactions() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new TransactionVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeBlockProposal(value) {
  const buffers = [];
  buffers.push(SerializeTransactionVec(value.transactions));
  return serializeTable(buffers);
}

export class IndexTransaction {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Uint32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Transaction(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
  }

  getIndex() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getTransaction() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Transaction(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeIndexTransaction(value) {
  const buffers = [];
  buffers.push(SerializeUint32(value.index));
  buffers.push(SerializeTransaction(value.transaction));
  return serializeTable(buffers);
}

export class IndexTransactionVec {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    for (let i = 0; i < len(offsets) - 1; i++) {
      new IndexTransaction(this.view.buffer.slice(offsets[i], offsets[i + 1]), { validate: false }).validate();
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
    return new IndexTransaction(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeIndexTransactionVec(value) {
  return serializeTable(value.map(item => SerializeIndexTransaction(item)));
}

export class SyncMessage {
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
      new GetHeaders(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 1:
      new SendHeaders(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 2:
      new GetBlocks(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 3:
      new SendBlock(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 4:
      new SetFilter(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 5:
      new AddFilter(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 6:
      new ClearFilter(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 7:
      new FilteredBlock(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    case 8:
      new InIBD(this.view.buffer.slice(4), { validate: false }).validate();
      break;
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }

  unionType() {
    const t = this.view.getUint32(0, true);
    switch (t) {
    case 0:
      return "GetHeaders";
    case 1:
      return "SendHeaders";
    case 2:
      return "GetBlocks";
    case 3:
      return "SendBlock";
    case 4:
      return "SetFilter";
    case 5:
      return "AddFilter";
    case 6:
      return "ClearFilter";
    case 7:
      return "FilteredBlock";
    case 8:
      return "InIBD";
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }

  value() {
    const t = this.view.getUint32(0, true);
    switch (t) {
    case 0:
      return new GetHeaders(this.view.buffer.slice(4), { validate: false });
    case 1:
      return new SendHeaders(this.view.buffer.slice(4), { validate: false });
    case 2:
      return new GetBlocks(this.view.buffer.slice(4), { validate: false });
    case 3:
      return new SendBlock(this.view.buffer.slice(4), { validate: false });
    case 4:
      return new SetFilter(this.view.buffer.slice(4), { validate: false });
    case 5:
      return new AddFilter(this.view.buffer.slice(4), { validate: false });
    case 6:
      return new ClearFilter(this.view.buffer.slice(4), { validate: false });
    case 7:
      return new FilteredBlock(this.view.buffer.slice(4), { validate: false });
    case 8:
      return new InIBD(this.view.buffer.slice(4), { validate: false });
    default:
      throw new Error(`Invalid type: ${t}`);
    }
  }
}

export function SerializeSyncMessage(value) {
  switch (value.type) {
  case "GetHeaders":
    {
      const itemBuffer = SerializeGetHeaders(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 0, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "SendHeaders":
    {
      const itemBuffer = SerializeSendHeaders(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 1, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "GetBlocks":
    {
      const itemBuffer = SerializeGetBlocks(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 2, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "SendBlock":
    {
      const itemBuffer = SerializeSendBlock(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 3, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "SetFilter":
    {
      const itemBuffer = SerializeSetFilter(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 4, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "AddFilter":
    {
      const itemBuffer = SerializeAddFilter(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 5, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "ClearFilter":
    {
      const itemBuffer = SerializeClearFilter(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 6, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "FilteredBlock":
    {
      const itemBuffer = SerializeFilteredBlock(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 7, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  case "InIBD":
    {
      const itemBuffer = SerializeInIBD(value.value);
      const array = new Uint8Array(4 + itemBuffer.byteLength);
      const view = new DataView(array.buffer);
      view.setUint32(0, 8, true);
      array.set(new Uint8Array(itemBuffer), 4);
      return array.buffer;
    }
  default:
    throw new Error(`Invalid type: ${value.type}`);
  }

}

export class GetHeaders {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Byte32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Byte32Vec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
  }

  getHashStop() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Byte32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getBlockLocatorHashes() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeGetHeaders(value) {
  const buffers = [];
  buffers.push(SerializeByte32(value.hash_stop));
  buffers.push(SerializeByte32Vec(value.block_locator_hashes));
  return serializeTable(buffers);
}

export class GetBlocks {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Byte32Vec(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
  }

  getBlockHashes() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Byte32Vec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeGetBlocks(value) {
  const buffers = [];
  buffers.push(SerializeByte32Vec(value.block_hashes));
  return serializeTable(buffers);
}

export class SendHeaders {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new HeaderVec(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
  }

  getHeaders() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new HeaderVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeSendHeaders(value) {
  const buffers = [];
  buffers.push(SerializeHeaderVec(value.headers));
  return serializeTable(buffers);
}

export class SendBlock {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Block(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
  }

  getBlock() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Block(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeSendBlock(value) {
  const buffers = [];
  buffers.push(SerializeBlock(value.block));
  return serializeTable(buffers);
}

export class SetFilter {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Uint32(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new Bytes(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    if (offsets[3] - offsets[2] !== 1) {
      throw new Error(`Invalid offset for num_hashes: ${offsets[2]} - ${offsets[3]}`)
    }
  }

  getHashSeed() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getFilter() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getNumHashes() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new DataView(this.view.buffer.slice(offset, offset_end)).getUint8(0);
  }
}

export function SerializeSetFilter(value) {
  const buffers = [];
  buffers.push(SerializeUint32(value.hash_seed));
  buffers.push(SerializeBytes(value.filter));
  const numHashesView = new DataView(new ArrayBuffer(1));
  numHashesView.setUint8(0, value.num_hashes);
  buffers.push(numHashesView.buffer)
  return serializeTable(buffers);
}

export class AddFilter {
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

  getFilter() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeAddFilter(value) {
  const buffers = [];
  buffers.push(SerializeBytes(value.filter));
  return serializeTable(buffers);
}

export class ClearFilter {
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

export function SerializeClearFilter(value) {
  const buffers = [];
  return serializeTable(buffers);
}

export class FilteredBlock {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Header(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new TransactionVec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
    new MerkleProof(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
  }

  getHeader() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Header(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getTransactions() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new TransactionVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getProof() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new MerkleProof(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeFilteredBlock(value) {
  const buffers = [];
  buffers.push(SerializeHeader(value.header));
  buffers.push(SerializeTransactionVec(value.transactions));
  buffers.push(SerializeMerkleProof(value.proof));
  return serializeTable(buffers);
}

export class MerkleProof {
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

export function SerializeMerkleProof(value) {
  const buffers = [];
  buffers.push(SerializeUint32Vec(value.indices));
  buffers.push(SerializeByte32Vec(value.lemmas));
  return serializeTable(buffers);
}

export class InIBD {
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

export function SerializeInIBD(value) {
  const buffers = [];
  return serializeTable(buffers);
}

export class Time {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new Uint64(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
  }

  getTimestamp() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeTime(value) {
  const buffers = [];
  buffers.push(SerializeUint64(value.timestamp));
  return serializeTable(buffers);
}

export class RawAlert {
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
    new BytesOpt(this.view.buffer.slice(offsets[5], offsets[6]), { validate: false }).validate();
    new BytesOpt(this.view.buffer.slice(offsets[6], offsets[7]), { validate: false }).validate();
  }

  getNoticeUntil() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getId() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getCancel() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getPriority() {
    const start = 16;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint32(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getMessage() {
    const start = 20;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getMinVersion() {
    const start = 24;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new BytesOpt(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getMaxVersion() {
    const start = 28;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new BytesOpt(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeRawAlert(value) {
  const buffers = [];
  buffers.push(SerializeUint64(value.notice_until));
  buffers.push(SerializeUint32(value.id));
  buffers.push(SerializeUint32(value.cancel));
  buffers.push(SerializeUint32(value.priority));
  buffers.push(SerializeBytes(value.message));
  buffers.push(SerializeBytesOpt(value.min_version));
  buffers.push(SerializeBytesOpt(value.max_version));
  return serializeTable(buffers);
}

export class Alert {
  constructor(reader, { validate = true } = {}) {
    this.view = new DataView(assertArrayBuffer(reader));
    if (validate) {
      this.validate();
    }
  }

  validate(compatible = false) {
    const offsets = verifyAndExtractOffsets(this.view, 0, true);
    new RawAlert(this.view.buffer.slice(offsets[0], offsets[1]), { validate: false }).validate();
    new BytesVec(this.view.buffer.slice(offsets[1], offsets[2]), { validate: false }).validate();
  }

  getRaw() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new RawAlert(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getSignatures() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new BytesVec(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeAlert(value) {
  const buffers = [];
  buffers.push(SerializeRawAlert(value.raw));
  buffers.push(SerializeBytesVec(value.signatures));
  return serializeTable(buffers);
}

export class Identify {
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
    new Bytes(this.view.buffer.slice(offsets[2], offsets[3]), { validate: false }).validate();
  }

  getFlag() {
    const start = 4;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Uint64(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getName() {
    const start = 8;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.getUint32(start + 4, true);
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }

  getClientVersion() {
    const start = 12;
    const offset = this.view.getUint32(start, true);
    const offset_end = this.view.byteLength;
    return new Bytes(this.view.buffer.slice(offset, offset_end), { validate: false });
  }
}

export function SerializeIdentify(value) {
  const buffers = [];
  buffers.push(SerializeUint64(value.flag));
  buffers.push(SerializeBytes(value.name));
  buffers.push(SerializeBytes(value.client_version));
  return serializeTable(buffers);
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
    assertDataLength(this.view.byteLength, this.size());
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
    assertDataLength(this.view.byteLength, this.size());
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
    assertDataLength(this.view.byteLength, this.size());
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
    assertDataLength(this.view.byteLength, this.size());
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
    assertDataLength(this.view.byteLength, this.size());
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
