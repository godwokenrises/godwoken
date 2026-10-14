import { describe, it, expect } from "vitest";
import fixture from "./claim-fixture.json";
import type { BuildRequest, Config, Quote } from "../src/types";
import {
  digest,
  verifyQuote,
  verifyWalletSignature,
  destination,
} from "../src/verify";
const f = fixture as unknown as {
  request: BuildRequest;
  config: Config;
  quote: Quote;
  signature: string;
};
describe("Independent browser checks against a Rust-generated, CKB-VM-executed transaction", () => {
  it("reproduces the exact raw hash, signing digest, fee and ETH recovery", () => {
    expect(verifyQuote(f.quote, f.request, f.config)).toBe(f.quote);
    expect(digest(f.quote.transaction).message).toBe(f.quote.signing_message);
    expect(() =>
      verifyWalletSignature(
        f.quote.signing_message,
        f.signature,
        f.request.address,
      ),
    ).not.toThrow();
  });
  it("compares objects by fields, not JSON property order", () => {
    const q = structuredClone(f.quote);
    q.transaction.cell_deps = q.transaction.cell_deps.map((d) => ({
      out_point: { tx_hash: d.out_point.tx_hash, index: d.out_point.index },
      dep_type: d.dep_type,
    }));
    expect(() => verifyQuote(q, f.request, f.config)).not.toThrow();
  });
  it.each([
    [
      "destination",
      (q: Quote) => {
        q.transaction.outputs[0].lock.args = "0x" + "33".repeat(20);
      },
    ],
    [
      "amount",
      (q: Quote) => {
        q.transaction.outputs_data[0] = "0x" + "00".repeat(16);
      },
    ],
    [
      "fee",
      (q: Quote) => {
        q.transaction.outputs[0].capacity = "0x1";
      },
    ],
    [
      "owner",
      (q: Quote) => {
        q.input_cells[0].output.lock.args = "0x01" + "33".repeat(20) + "00";
      },
    ],
    [
      "lock suffix",
      (q: Quote) => {
        q.input_cells[0].output.lock.args += "00";
      },
    ],
    [
      "sUDT",
      (q: Quote) => {
        q.transaction.outputs[0].type!.args = "0x" + "33".repeat(32);
      },
    ],
    [
      "duplicate input",
      (q: Quote) => {
        q.transaction.inputs[1] = q.transaction.inputs[0];
      },
    ],
    [
      "extra output",
      (q: Quote) => {
        q.transaction.outputs.push(q.transaction.outputs[0]);
        q.transaction.outputs_data.push(q.transaction.outputs_data[0]);
      },
    ],
    [
      "missing output",
      (q: Quote) => {
        q.transaction.outputs.pop();
        q.transaction.outputs_data.pop();
      },
    ],
    [
      "signing hash",
      (q: Quote) => {
        q.signing_message = "0x" + "00".repeat(32);
      },
    ],
    [
      "dependency",
      (q: Quote) => {
        q.transaction.cell_deps.pop();
      },
    ],
    [
      "extra witness",
      (q: Quote) => {
        q.transaction.witnesses.push("0x");
      },
    ],
    [
      "amount summary",
      (q: Quote) => {
        q.received_capacity_shannons = "999";
      },
    ],
    [
      "fee summary",
      (q: Quote) => {
        q.fee_shannons = "0";
      },
    ],
  ] as const)("rejects a changed %s", (_name, mutate) => {
    const q = structuredClone(f.quote);
    mutate(q);
    expect(() => verifyQuote(q, f.request, f.config)).toThrow();
  });
  it("rejects invalid destinations and signer mismatches", () => {
    for (const bad of [
      "ckt1bad",
      "0x" + "11".repeat(20),
      f.request.destination + "q",
    ])
      expect(() => destination(bad)).toThrow();
    expect(() =>
      verifyWalletSignature(
        f.quote.signing_message,
        f.signature,
        "0x" + "11".repeat(20),
      ),
    ).toThrow();
  });
});
