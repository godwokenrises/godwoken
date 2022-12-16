import test from "ava";
import { JSONResponse, client } from "../www";
import * as errcode from "../../src/methods/error-code";

test("gw_get_storage_at", async (t) => {
  const res: JSONResponse = await client.request(t.title, []);
  t.truthy(res.error);
  t.is(res.error?.code, errcode.INVALID_PARAMS);
});

test("gw_get_nonce", async (t) => {
  const res: JSONResponse = await client.request(t.title, ["0xasdf"]);
  t.truthy(res.error);
  t.is(res.error?.code, errcode.INVALID_PARAMS);
});

test("gw_execute_raw_l2transaction", async (t) => {
  const res: JSONResponse = await client.request(t.title, [
    "0x5c00000014000000180000001c0000002000000004000000140000001300000038000000ffffff504f4c590000a0724e18090000000000000000000000000000000000000000000000000000000000000000000004000000d504ea1d",
  ]);
  t.truthy(res.error);
  // FIXME https://github.com/nervosnetwork/godwoken-web3/issues/256
  // t.is(res.error?.code, errcode.INVALID_PARAMS);
});
