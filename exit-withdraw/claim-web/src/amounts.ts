import { parseUnits } from "ethers";
import type { Asset, Balance } from "./types";

// Never convert financial amounts through Number, rounding or floating point.
export function withdrawalAmounts(
  values: Record<number, string>,
  available: Balance[],
  assets: Asset[],
): Balance[] {
  const amounts = available
    .map((balance) => {
      const text = values[balance.asset_id]?.trim() || "0";
      const asset = assets.find((a) => a.asset_id === balance.asset_id);
      const decimals = asset?.l1_decimals ?? 0;
      if (
        !/^\d+(?:\.\d+)?$/.test(text) ||
        (text.split(".")[1]?.length ?? 0) > decimals
      )
        throw new Error(
          `Invalid amount for ${asset?.symbol ?? `asset #${balance.asset_id}`}. Use at most ${decimals} decimal places.`,
        );
      const n = parseUnits(text, decimals);
      if (n > BigInt(balance.raw))
        throw new Error(
          `Requested ${asset?.symbol ?? `asset #${balance.asset_id}`} exceeds remaining balance.`,
        );
      return { asset_id: balance.asset_id, raw: n.toString() };
    })
    .filter((a) => BigInt(a.raw) > 0n);
  if (!amounts.some((a) => a.asset_id === 1))
    throw new Error(
      "Enter the CKB to receive, including token-cell capacity. The network fee is paid in addition.",
    );
  return amounts;
}
