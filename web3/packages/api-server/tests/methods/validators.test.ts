import test from "ava";

import { validators } from "../../src/methods/validator";
import { InvalidParamsError } from "../../src/methods/error";

test("validators.rawTransaction's s with leading zeros", (t) => {
  const originRawTx =
    "0xf8680485174876e8008302f1c8940000000000000000000000000000000000000000808083022df4a0aa4567c44b378929018ebd3100716f288dd17f616ff9418d7ef0341f3aef4ca0a00013d2f7290ba5aff8911b28871c8e6b624117a711dea4f1484e0dc9d587d60a";

  const params = [originRawTx];

  const result = validators.rawTransaction(params, 0);

  t.true(result instanceof InvalidParamsError);
  t.is(
    result?.message,
    "invalid argument 0: rlp: non-canonical integer (leading zero bytes) for s, receive: 0x0013d2f7290ba5aff8911b28871c8e6b624117a711dea4f1484e0dc9d587d60a"
  );
});

test("validators.rawTranaction's r with leading zeros", (t) => {
  const originRawTx =
    "0xf8680485174876e8008302f45b940000000000000000000000000000000000000000808083022df4a000330af14634c7d18125258707de6b115ff990743d5c5fac35089e54ea4dfd60a01a9186f9b620063219709aadb6857f88702aa03d94850199c80d39ec090897a2";

  const params = [originRawTx];

  const result = validators.rawTransaction(params, 0);

  t.true(result instanceof InvalidParamsError);
  t.is(
    result?.message,
    "invalid argument 0: rlp: non-canonical integer (leading zero bytes) for r, receive: 0x00330af14634c7d18125258707de6b115ff990743d5c5fac35089e54ea4dfd60"
  );
});

test("validators.rawTransaction valid", (t) => {
  const originRawTx =
    "0xf8670485174876e8008302f45b940000000000000000000000000000000000000000808083022df49f330af14634c7d18125258707de6b115ff990743d5c5fac35089e54ea4dfd60a01a9186f9b620063219709aadb6857f88702aa03d94850199c80d39ec090897a2";

  const params = [originRawTx];

  const result = validators.rawTransaction(params, 0);

  t.is(result, undefined);
});
