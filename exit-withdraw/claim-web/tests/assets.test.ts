import { describe, expect, it } from "vitest";
import { accountAssets } from "../src/assets";
import type { Asset, Explanation, Inventory } from "../src/types";

const asset = (id: number): Asset => ({
  asset_id: id,
  symbol: `Asset ${id}`,
  decimals: 18,
  l1_decimals: 8,
  type_hash: "0x",
});
const explanation = (id: number, status: string): Explanation => ({
  asset_id: id,
  symbol: null,
  calculated_entitlement: "123.000000000000000001",
  status,
  reason: "Policy disposition",
  protocols: ["Direct wallet balance"],
});
const inventory = (data: Partial<Inventory>): Inventory => ({
  address: "",
  omnilock_address: "",
  cells: [],
  entitlements: [],
  available: [],
  explanations: [],
  more_batches: false,
  unsupported_cells: 0,
  confirming_cells: 0,
  confirmations_required: 24,
  observed_block: 0,
  ...data,
});

describe("complete account asset listing", () => {
  it("lists funded, unfunded, held and excluded assets without promoting snapshot rights to payable balances", () => {
    const rows = accountAssets(
      [1, 2, 3, 4, 5].map(asset),
      inventory({
        entitlements: [
          { asset_id: 1, raw: "1000000000000000001" },
          { asset_id: 2, raw: "20" },
        ],
        available: [{ asset_id: 1, raw: "100000000" }],
        explanations: [explanation(3, "DEFERRED"), explanation(4, "EXCLUDED")],
      }),
    );
    expect(rows.map((row) => row.asset_id)).toEqual([1, 2, 3, 4]);
    expect(rows[0].entitlement).toBe("1000000000000000001");
    expect(rows[0].available).toBe("100000000");
    expect(rows[1].available).toBe("0");
    for (const row of rows.slice(2)) {
      expect(row.entitlement).toBe("0");
      expect(row.available).toBe("0");
      expect(row.explanations[0].calculated_entitlement).toBe(
        "123.000000000000000001",
      );
    }
  });
  it("retains assets absent from the catalogue and all explanations without duplicate asset rows", () => {
    const records = [explanation(99, "HELD"), explanation(99, "EXCLUDED")];
    const rows = accountAssets(
      [],
      inventory({
        available: [{ asset_id: 99, raw: "900719925474099300000" }],
        explanations: records,
      }),
    );
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({
      asset_id: 99,
      symbol: null,
      decimals: null,
      l1_decimals: null,
      available: "900719925474099300000",
      explanations: records,
    });
  });
  it("does not list unrelated catalogue assets for an empty account", () => {
    expect(accountAssets([asset(1)], inventory({}))).toEqual([]);
  });
});
