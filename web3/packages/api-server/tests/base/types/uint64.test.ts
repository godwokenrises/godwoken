import test from "ava";
import { Uint64 } from "../../../src/base/types/uint";

const ERROR_MESSAGE = "value to small or too big";
const value = BigInt("4294967396");
const hex = "0x100000064";
const littleEndian = "0x6400000001000000";

test("Uint64 constructor", (t) => {
  t.is(new Uint64(value).getValue(), value);
});

test("Uint64 too big", (t) => {
  t.throws(() => new Uint64(2n ** 64n), undefined, ERROR_MESSAGE);
});

test("Uint64 too small", (t) => {
  t.throws(() => new Uint64(-1n), undefined, ERROR_MESSAGE);
});

test("Uint64 toHex", (t) => {
  t.is(new Uint64(value).toHex(), hex);
});

test("Uint64 fromHex", (t) => {
  t.is(Uint64.fromHex(hex).getValue(), value);
});

test("Uint64 toLittleEndian", (t) => {
  t.is(new Uint64(value).toLittleEndian(), littleEndian);
});

test("Uint64 fromLittleEndian", (t) => {
  t.is(Uint64.fromLittleEndian(littleEndian).getValue(), value);
});
