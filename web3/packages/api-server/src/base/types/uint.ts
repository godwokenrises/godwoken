import { HexNumber, HexString } from "@ckb-lumos/base";

export function toHexNumber(num: number | bigint): HexNumber {
  return "0x" + num.toString(16);
}

export class Uint32 {
  private value: number;

  public static MIN = 0;
  public static MAX = 2 ** 32 - 1;

  constructor(value: number) {
    if (typeof value !== "number") {
      throw new Error("Uint32 value must be a number!");
    }
    if (value < Uint32.MIN || value > Uint32.MAX) {
      throw new Error("value to small or too big");
    }
    this.value = value;
  }

  public getValue(): number {
    return this.value;
  }

  public toHex(): HexNumber {
    return toHexNumber(this.value);
  }

  public static fromHex(value: HexNumber): Uint32 {
    assertHexNumber("Uint32.fromHex args", value);
    return new Uint32(+value);
  }

  public toLittleEndian(): HexString {
    const buf = Buffer.alloc(4);
    buf.writeUInt32LE(this.value);
    return `0x${buf.toString("hex")}`;
  }

  public static fromLittleEndian(hex: HexString): Uint32 {
    assertHexNumber("Uint32.fromLittleEndian args", hex);
    if (hex.length !== 10 || !hex.startsWith("0x")) {
      throw new Error(`little endian hex format error`);
    }
    const buf = Buffer.from(hex.slice(2), "hex");
    const num = buf.readUInt32LE();
    return new Uint32(num);
  }
}

export class Uint64 {
  private value: bigint;

  public static MIN = 0;
  public static MAX = 2n ** 64n - 1n;

  constructor(value: bigint) {
    if (typeof value !== "bigint") {
      throw new Error("Uint64 value must be a bigint!");
    }
    if (value < Uint64.MIN || value > Uint64.MAX) {
      throw new Error("value to small or too big");
    }
    this.value = value;
  }

  public getValue(): bigint {
    return this.value;
  }

  public toHex(): HexNumber {
    return toHexNumber(this.value);
  }

  public static fromHex(value: HexNumber): Uint64 {
    assertHexNumber("Uint64.fromHex args", value);
    return new Uint64(BigInt(value));
  }

  public toLittleEndian(): HexString {
    const buf = Buffer.alloc(8);
    buf.writeBigUInt64LE(this.value);
    return `0x${buf.toString("hex")}`;
  }

  public static fromLittleEndian(hex: HexNumber): Uint64 {
    assertHexNumber("Uint64.fromLittleEndian args", hex);
    if (hex.length !== 18 || !hex.startsWith("0x")) {
      throw new Error(`little endian hex format error`);
    }
    const buf = Buffer.from(hex.slice(2), "hex");
    const num = buf.readBigUInt64LE();
    return new Uint64(num);
  }
}

export class Uint128 {
  private value: bigint;

  public static MIN: bigint = 0n;
  public static MAX: bigint = 2n ** 128n - 1n;

  constructor(value: bigint) {
    if (typeof value !== "bigint") {
      throw new Error("Uint128 value must be a bigint!");
    }
    if (value < Uint128.MIN || value > Uint128.MAX) {
      throw new Error("value to small or too big");
    }
    this.value = value;
  }

  public getValue(): bigint {
    return this.value;
  }

  public toHex(): HexNumber {
    return toHexNumber(this.value);
  }

  public static fromHex(value: HexNumber): Uint128 {
    assertHexNumber("Uint128.fromHex args", value);
    return new Uint128(BigInt(value));
  }

  public toLittleEndian(): HexString {
    const buf = Buffer.alloc(16);
    buf.writeBigUInt64LE(this.value & BigInt("0xFFFFFFFFFFFFFFFF"), 0);
    buf.writeBigUInt64LE(this.value >> BigInt(64), 8);
    return "0x" + buf.toString("hex");
  }

  public static fromLittleEndian(hex: HexNumber): Uint128 {
    if (hex.length !== 34 || !hex.startsWith("0x")) {
      throw new Error(`little endian hex format error`);
    }
    const buf = Buffer.from(hex.slice(2, 34), "hex");
    const num = (buf.readBigUInt64LE(8) << BigInt(64)) + buf.readBigUInt64LE(0);
    return new Uint128(num);
  }
}

export class Uint256 {
  private value: bigint;

  public static MIN: bigint = 0n;
  public static MAX: bigint = 2n ** 256n - 1n;

  constructor(value: bigint) {
    if (typeof value !== "bigint") {
      throw new Error("Uint256 value must be a bigint!");
    }
    if (value < Uint256.MIN || value > Uint256.MAX) {
      throw new Error("value to small or too big");
    }
    this.value = value;
  }

  public getValue(): bigint {
    return this.value;
  }

  public toHex(): HexNumber {
    return toHexNumber(this.value);
  }

  public static fromHex(value: HexNumber): Uint256 {
    assertHexNumber("Uint256.fromHex args", value);
    return new Uint256(BigInt(value));
  }

  public toLittleEndian(): HexString {
    const u64Max = BigInt("0xFFFFFFFFFFFFFFFF");
    const buf = Buffer.alloc(32);
    buf.writeBigUInt64LE(this.value & u64Max, 0);
    buf.writeBigUInt64LE((this.value >> 64n) & u64Max, 8);
    buf.writeBigUInt64LE((this.value >> 128n) & u64Max, 16);
    buf.writeBigUInt64LE((this.value >> 192n) & u64Max, 24);
    return "0x" + buf.toString("hex");
  }

  public static fromLittleEndian(hex: HexNumber): Uint256 {
    if (hex.length !== 66 || !hex.startsWith("0x")) {
      throw new Error(`little endian hex format error`);
    }
    const buf = Buffer.from(hex.slice(2, 66), "hex");
    const num =
      buf.readBigUInt64LE(0) +
      (buf.readBigUInt64LE(8) << 64n) +
      (buf.readBigUInt64LE(16) << 128n) +
      (buf.readBigUInt64LE(24) << 192n);
    return new Uint256(num);
  }
}

function assertHexNumber(debugPath: string, str: string) {
  if (!/^0x(0|[0-9a-fA-F]+)$/.test(str)) {
    throw new Error(`${debugPath} must be a hex number!`);
  }
}
