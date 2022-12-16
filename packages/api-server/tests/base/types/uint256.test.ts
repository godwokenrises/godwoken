import test from "ava";
import { Uint256 } from "../../../src/base/types/uint";

const ERROR_MESSAGE = "value to small or too big";
const value = BigInt(
  "26959946667150639794667015087019630673637144422540572481103610249316"
);
const hex = "0x100000000000000000000000000000000000000000000000000000064";
const littleEndian =
  "0x6400000000000000000000000000000000000000000000000000000001000000";

test("Uint256 constructor", (t) => {
  t.is(new Uint256(value).getValue(), value);
});

test("Uint256 too big", (t) => {
  t.throws(() => new Uint256(2n ** 256n), undefined, ERROR_MESSAGE);
});

test("Uint256 too small", (t) => {
  t.throws(() => new Uint256(-1n), undefined, ERROR_MESSAGE);
});

test("Uint256 toHex", (t) => {
  t.is(new Uint256(value).toHex(), hex);
});

test("Uint256 fromHex", (t) => {
  t.is(Uint256.fromHex(hex).getValue(), value);
});

test("Uint256 toLittleEndian", (t) => {
  t.is(new Uint256(value).toLittleEndian(), littleEndian);
});

test("Uint256 fromLittleEndian", (t) => {
  t.is(Uint256.fromLittleEndian(littleEndian).getValue(), value);
});
