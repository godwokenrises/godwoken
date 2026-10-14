# V0 asset treatment

## General treatment

Use exact asset identities, not symbols, to establish custody support. Unbacked assets cannot be paid from this pool; lack of backing alone does not prove how a token was issued. LP, farm, vault, wrapped and lending receipts are traced to underlying before the support check. Never pay both a receipt and its backing. Existing verified positions include embedded trading fees/compounding; separate unclaimed reward streams are excluded.

No blanket haircut or personal-account USD minimum applies. Pool-specific approved losses and research deferrals are explicit. Preserve non-applicant, excluded, locked and rounding balances. No cross-pool subsidy, arbitrary token swap, new exit fee or assumed external compensation. Capacity shortages delay execution without erasing entitlement. Network transaction fees remain separate.

The full per-source accounting is in [refunds/source-reserves.csv](refunds/source-reserves.csv); per-beneficiary provenance is in [refunds/claim-sources.csv](refunds/claim-sources.csv). Every source satisfies native input = refund entitlement + retained amount. Actual L1 payment additionally floors to native L1 units after beneficiary aggregation.

## V0 scope

- **All YOK is excluded**, including direct balances, conversion paths, LP underlying and fee rights. Other supported sides of a mixed pool remain independently eligible.
- KOI and NFT ownership are excluded from this round. Separate pending YOK/KOI/CKB rewards are not principal refunds.
- ETH owners only. TRON/unrecognized owners and unmapped destinations remain held. A v0 application short address is not itself the ETH signer.
- Verified WCKB/LP/farm/stable-pool principal is unwound by current shares. Fee-adjusted total supply and all locked/unidentified shares remain in denominators; do not renormalize known holders upward.
- Approved feeTo 16045 unminted protocol-fee rights are included for supported assets, separately from its existing holdings. These arise from the deployed pair fee formula, not invented new principal. Verify fee state and include the fee shares in the supply denominator.
- Seven IFOs **19202, 29696, 31272, 36023, 36024, 47794, 47796** allocate each pool's actual refundable holdings proportionately to verified unclaimed refunds. Their specific authorization does not permit deficits to be funded by another source. IFO 49401 is also checked, but has no blanket shortfall approval.
- Daruma 36838 project treasury is held; manager 35969 is not automatically the beneficial owner. System balances, sale proceeds without user refund rights, unregistered reserves and rounding remain reserved.

Implementation: [V0 adapter](../../src/refund/v0.rs), [storage identities](../../src/refund/policy/v0-storage.json), [asset identities](../../src/refund/policy/v0-assets.json). Rules are embedded; monetary inputs come only from DB.
