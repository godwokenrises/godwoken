import type { Asset, Inventory } from "./types";

// Snapshot dispositions explain assets; they must never increase payable amounts.
export function accountAssets(assets: Asset[], inventory: Inventory) {
  const ids = new Set([
    ...inventory.entitlements.map((row) => row.asset_id),
    ...inventory.available.map((row) => row.asset_id),
    ...inventory.explanations.map((row) => row.asset_id),
  ]);
  return [...ids]
    .sort((a, b) => a - b)
    .map((id) => {
      const explanations = inventory.explanations.filter(
        (row) => row.asset_id === id,
      );
      const metadata = assets.find((asset) => asset.asset_id === id);
      return {
        asset_id: id,
        symbol:
          metadata?.symbol ??
          explanations.find((row) => row.symbol)?.symbol ??
          null,
        decimals: metadata?.decimals ?? null,
        l1_decimals: metadata?.l1_decimals ?? null,
        entitlement:
          inventory.entitlements.find((row) => row.asset_id === id)?.raw ?? "0",
        available:
          inventory.available.find((row) => row.asset_id === id)?.raw ?? "0",
        explanations,
      };
    });
}
