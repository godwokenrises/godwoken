# Godwoken refunds

Current documentation for **mainnet V0 and V1**, kept separate throughout calculation, authorization and payment. Tables are calculations, not evidence that funds have been paid.

- [V0 assets, refunds and decisions](v0/README.md)
- [V1 assets, refunds and decisions](v1/README.md)
- [Operator runbook](ops-refund-runbook.md): reproduce, query, upgrade, initialize and pay applicants; run the wallet portal.
- [Contract design](contract-design.md): commitments, authority, conservation and replay protection.
- [Validation](validation.md): executed tests, reproduction and limitations.

Each version contains `assets/` (fresh L2 and L1 observations and reconciliation) and `refunds/` (fresh DB-only complete allocation). Read each manifest for exact snapshot and executable hashes. Raw integer fields are authoritative; display decimals do not change allocation.

Return verified eligible current positions backed by the selected custody assets. Trace receipts to underlying once, account for debt, preserve exclusions and non-applicants' rights, and charge no new exit fee. Custodian remainder is not free surplus. Both V0 and V1 exclude all YOK and KOI in this round. Prioritize material supported assets; preserve the approved small-value holds and CKB-only paths. See each version's treatment documents for all exceptions.

The full table is published for independent verification; only applicants selected by Ops are paid. One root initialization precedes subsequent batches. Approved deferrals count as completed treatment decisions, not extinguished rights. Reopening held claims requires a new reviewed policy/release; this validator does not accept arbitrary later epochs.

Machine connection details and operating paths belong in the separate machine inventory. `docs/notes/` is ignored private working material, not release documentation.
