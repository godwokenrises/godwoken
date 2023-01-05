import test from "ava";
import { Uint128 } from "../../../src/base/types/uint";

const ERROR_MESSAGE = "value to small or too big";
const value = BigInt("79228162514264337593543950436");
const hex = "0x1000000000000000000000064";
const littleEndian = "0x64000000000000000000000001000000";

test("Uint128 constructor", (t) => {
  t.is(new Uint128(value).getValue(), value);
});

test("Uint128 too big", (t) => {
  t.throws(() => new Uint128(2n ** 128n), undefined, ERROR_MESSAGE);
});

test("Uint128 too small", (t) => {
  t.throws(() => new Uint128(-1n), undefined, ERROR_MESSAGE);
});

test("Uint128 toHex", (t) => {
  t.is(new Uint128(value).toHex(), hex);
});

test("Uint128 fromHex", (t) => {
  t.is(Uint128.fromHex(hex).getValue(), value);
});

test("Uint128 toLittleEndian", (t) => {
  t.is(new Uint128(value).toLittleEndian(), littleEndian);
});

test("Uint128 fromLittleEndian", (t) => {
  t.is(Uint128.fromLittleEndian(littleEndian).getValue(), value);
});
