# V1 asset treatment

## General treatment

Use exact asset identities, not symbols, to establish custody support. Unbacked assets cannot be paid from this pool; lack of backing alone does not prove how a token was issued. LP, farm, vault, wrapped and lending receipts are traced to underlying before the support check. Never pay both a receipt and its backing. Existing verified positions include embedded trading fees/compounding; separate unclaimed reward streams are excluded.

No blanket haircut or personal-account USD minimum applies. Pool-specific approved losses and research deferrals are explicit. Preserve non-applicant, excluded, locked and rounding balances. No cross-pool subsidy, arbitrary token swap, new exit fee or assumed external compensation. Capacity shortages delay execution without erasing entitlement. Network transaction fees remain separate.

The full per-source accounting is in [refunds/source-reserves.csv](refunds/source-reserves.csv); per-beneficiary provenance is in [refunds/claim-sources.csv](refunds/claim-sources.csv). Every source satisfies native input = refund entitlement + retained amount. Actual L1 payment additionally floors to native L1 units after beneficiary aggregation.

## V1 scope and exceptions

- **All YOK is excluded**, including direct balances, LP/farm/vault underlying and protocol-fee rights. This matches V0 and is a value/scope decision, not a YOK custody shortfall. Supported non-YOK sides of mixed positions remain independently refundable. KOI and NFT ownership remain outside this round.
- Pending CKB rewards from 5826 (14,008.13568 CKB at the frozen state) and idle YOK reward holdings of strategy 3172 stay held. Their separately verified non-YOK principal is not excluded.
- WBTC wrapper 121's 0.0035 self-held WBTC stays reserved pending an application and purpose/entitlement verification by original sender 3878. A known transfer sender alone is not a redemption right.
- The four specifically approved IFO shortfalls (5731, 5732, 11852, 11853) are allocated proportionately within their own actual asset budgets. No general IFO or token haircut is implied. Pool identities are in the embedded storage recipes and claim provenance.
- Marketplace 4572 creator royalties remain deferred, including proved monetary claims. NFT deferral does not silently erase separately verified monetary rights.
- Low-value decisions concern whole supported direct-plus-indirect contract portfolios, not just CKB, and only unresolved residues. The approved threshold was USD 100 at the reviewed scenario, with explicit exceptions 12394 and the outgoing unregistered LP case linked to 12697. Unrelated balances of 12697 are not excluded.
- The newly traced rights through 10735, 10737, 11740 and Vault 10711 are **CKB-only**. Their incremental ETH/USDT/USDC/WBTC remain held; those assets are not globally excluded. Unbooked Vault-interest attribution remains reserved. Proven routes through 11560/11567 retain all eligible assets.
- Zero/dead addresses, relayers 11736/12028, locked shares, protocol reservations without approved beneficiaries and unknown destinations remain held. Never infer abandonment.
- History-discovered unregistered native balances remain separate from the registered census. Their current balances are authenticated; history is used to discover keys, not as an alternative balance source. See the regenerated reconciliation for their custody effect and the remaining recorded CKB-supply discrepancy.

Implementation: [V1 adapters](../../src/refund/v1.rs), [explicit dispositions](../../src/refund/v1_disposition.rs), [storage identities](../../src/refund/policy/v1-storage.json), [asset identities](../../src/refund/policy/v1-assets.json). Approved low-value IDs are frozen decisions, not hardcoded historical monetary balances or current-price screening.

## Policy alignment on 14 October 2026

Prioritize material assets without expanding the approved research and cell-capacity costs for small positions. This revision changes only V1 YOK eligibility. Small pending protocol-fee rights remain held; the already approved material V0 fee rights remain eligible for non-YOK assets. Existing CKB-only allocations, low-value account decisions, lending losses, rewards and authority rules remain unchanged. Removing YOK outputs can reduce CKB top-ups and change capacity-ready status without changing any non-YOK monetary entitlement.
