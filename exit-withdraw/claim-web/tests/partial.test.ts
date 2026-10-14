import { describe, it, expect } from "vitest";
import fixture from "./partial-claim-fixture.json";
import type { BuildRequest, Config, Quote } from "../src/types";
import {
  verifyQuote,
  verifyWalletSignature,
  transferTokens,
  digest,
} from "../src/verify";
import { withdrawalAmounts } from "../src/amounts";
const f = fixture as unknown as {
  request: BuildRequest;
  quote: Quote;
  config: Config;
  signature: string;
  second_request: BuildRequest;
  second_quote: Quote;
  second_signature: string;
};

describe("Partial withdrawal across two real Rust/VM transactions", () => {
  it("verifies first withdrawal and same-owner change, then the remaining withdrawal", () => {
    verifyQuote(f.quote, f.request, f.config);
    verifyWalletSignature(
      f.quote.signing_message,
      f.signature,
      f.request.address,
    );
    verifyQuote(f.second_quote, f.second_request, f.config);
    verifyWalletSignature(
      f.second_quote.signing_message,
      f.second_signature,
      f.request.address,
    );
    const sent = transferTokens(f.quote, f.config);
    expect(sent).toHaveLength(1);
    expect(sent[0].raw).toBe("50000000"); // Change must not appear as recipient assets.
    expect(
      transferTokens(f.second_quote, f.config).find(
        (a) => a.asset.asset_id === sent[0].asset.asset_id,
      )?.raw,
    ).toBe("73456789");
  });
  it("rejects second withdrawal exceeding the remaining token balance by one unit", () => {
    const r = structuredClone(f.second_request);
    r.amounts = [
      { asset_id: 1, raw: f.second_quote.received_capacity_shannons },
      { asset_id: f.request.amounts![1].asset_id, raw: "73456790" },
    ];
    expect(() => verifyQuote(f.second_quote, r, f.config)).toThrow(
      /exceeds remaining balance/,
    );
  });
  it("rejects change redirected to a different ETH address even with a recomputed digest", () => {
    const q = structuredClone(f.quote);
    q.transaction.outputs.find(
      (o) => o.lock.args !== q.destination_lock.args,
    )!.lock.args = "0x01" + "22".repeat(20) + "00";
    const d = digest(q.transaction);
    q.transaction_hash = d.hash;
    q.signing_message = d.message;
    expect(() => verifyQuote(q, f.request, f.config)).toThrow(/change owner/);
  });
  it("rejects a server sending all tokens when the user requested only part", () => {
    const r = structuredClone(f.second_request);
    r.amounts = [
      { asset_id: 1, raw: f.second_quote.received_capacity_shannons },
      { asset_id: f.request.amounts![1].asset_id, raw: "1" },
    ];
    expect(() => verifyQuote(f.second_quote, r, f.config)).toThrow(
      /differs from recipient/,
    );
  });
  it("rejects a summary that counts change as received CKB", () => {
    const q = structuredClone(f.quote);
    q.received_capacity_shannons = q.transaction.outputs
      .reduce((n, o) => n + BigInt(o.capacity), 0n)
      .toString();
    expect(() => verifyQuote(q, f.request, f.config)).toThrow(
      /Capacity summary/,
    );
  });
});

describe("Exact partial amount entry", () => {
  const balances = [
    { asset_id: 1, raw: "80000000000" },
    { asset_id: 36, raw: "73456789" },
  ];
  it("keeps exact smallest units and accepts the remaining token balance", () => {
    expect(
      withdrawalAmounts(
        { 1: "600", 36: "0.73456789" },
        balances,
        f.config.assets,
      ),
    ).toEqual([
      { asset_id: 1, raw: "60000000000" },
      { asset_id: 36, raw: "73456789" },
    ]);
  });
  it.each(["0.73456790", "1", "9007199254740993"])(
    "rejects token overdraw %s before preparing/signing",
    (value) => {
      expect(() =>
        withdrawalAmounts({ 1: "600", 36: value }, balances, f.config.assets),
      ).toThrow(/exceeds remaining balance/);
    },
  );
  it.each(["-1", "1e-8", "0.000000001", "+1", "Infinity"])(
    "rejects invalid or too precise amount %s without rounding",
    (value) => {
      expect(() =>
        withdrawalAmounts({ 1: "600", 36: value }, balances, f.config.assets),
      ).toThrow(/Invalid amount/);
    },
  );
  it("requires CKB for receiving cells and rejects CKB overdraw", () => {
    expect(() =>
      withdrawalAmounts({ 36: "0.1" }, balances, f.config.assets),
    ).toThrow(/Enter the CKB/);
    expect(() =>
      withdrawalAmounts({ 1: "801" }, balances, f.config.assets),
    ).toThrow(/exceeds remaining balance/);
  });
});
