import { Price } from "../../src/base/gas-price";
import web3Utils from "web3-utils";
import test from "ava";

test("gas price by ckb price", (t) => {
  const ckbPrices = [
    "0.00189",
    "0.00199",
    "0.002",
    "0.0021",
    "0.00211",
    "0.00221",
    "0.00231",
    "0.00289",
    "0.00299",
    "0.003",
    "0.0031",
    "0.0032",
    "0.0033",
    "0.0034",
    "0.0035",
    "0.0036",
    "0.0037",
    "0.00389",
    "0.00489",
    "0.00589",
    "0.00689",
    "0.00789",
    "0.00889",
    "0.00989",
    "0.01",
    "0.04",
  ];
  for (const p of ckbPrices) {
    console.log(
      `ckb: $${p}, gasPrice: ${toPCKB(
        Price.from(p).toGasPrice()
      )} pCKB, minGasPrice: ${toPCKB(Price.from(p).toMinGasPrice())} pCKB`
    );
  }
  const gasPrices = ckbPrices.map((p) => Price.from(p).toGasPrice());
  const gasPriceSorted = gasPrices.sort((a, b) => +(b - a).toString());
  t.deepEqual(gasPrices, gasPriceSorted);
});

function toPCKB(wei: bigint) {
  return web3Utils.fromWei(wei.toString(10), "ether");
}
