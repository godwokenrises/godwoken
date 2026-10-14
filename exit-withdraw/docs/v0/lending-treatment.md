# V0 lending and debt policy

V0 uses the same economic principles as V1: account for debt within the originating pool, use verified pool reserves and protocol equity before ordinary-creditor loss, and never use unrelated assets to fill a gap. This does not import V1 pool IDs or its lending balance sheets into V0.

The reviewed V0 DB-only calculation has no separately approved Aave/Compound closeout basket analogous to V1 10659. Its supported principal adapters cover native balances, WCKB, pairs/farms, stable pools and IFO claims. A project treasury or unsupported receipt is not converted into a lender refund by analogy. Held source balances and their disposition are recorded in `refunds/source-reserves.csv`.

Daruma 36838 remains project treasury custody, not established DRM-holder pro-rata redemption. Do not treat its USDC balance as automatically payable to manager 35969. A new funded lending path would require an explicitly reviewed adapter and debt/authority checks; unknown funded residues must not silently acquire an EOA beneficiary.

[V0 implementation](../../src/refund/v0.rs) · [account decisions](account-treatment.md)
