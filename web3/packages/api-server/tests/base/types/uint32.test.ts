import test from "ava";
import { Uint32 } from "../../../src/base/types/uint";

const ERROR_MESSAGE = "value to small or too big";
const value = 100;
const hex = "0x64";
const littleEndian = "0x64000000";

test("Uint32 constructor", (t) => {
  t.is(new Uint32(value).getValue(), value);
});

test("Uint32 too big", (t) => {
  t.throws(() => new Uint32(2 ** 32), undefined, ERROR_MESSAGE);
});

test("Uint32 too small", (t) => {
  t.throws(() => new Uint32(-1), undefined, ERROR_MESSAGE);
});

test("Uint32 toHex", (t) => {
  t.is(new Uint32(value).toHex(), hex);
});

test("Uint32 fromHex", (t) => {
  t.is(Uint32.fromHex(hex).getValue(), value);
});

test("Uint32 toLittleEndian", (t) => {
  t.is(new Uint32(value).toLittleEndian(), littleEndian);
});

test("Uint32 fromLittleEndian", (t) => {
  t.is(Uint32.fromLittleEndian(littleEndian).getValue(), value);
});
