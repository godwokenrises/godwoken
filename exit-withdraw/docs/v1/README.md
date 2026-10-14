# V1 current asset and refund overview

Regenerated from the original read-only DB and embedded rules on 14 October 2026 after excluding all YOK. L1/L2 inventory observations below retain their recorded capture times. Exact snapshot/observation anchors are in [assets/manifest.json](assets/manifest.json). No deployment or payment was performed. All non-YOK monetary entitlements and source partitions match the prior table. Capacity estimates improve when YOK outputs are removed. See [policy comparison](refunds/policy-comparison.json) and [regeneration metadata](refunds/regeneration.json).

- Beneficiary/capacity entries: **10,670**; entries with positive payable assets: **9,392**. These are accounts, not necessarily distinct people.
- Capacity-blocked entries: **6,661**, requiring **415612.68628253 CKB** in the theoretical full-basket scenario.
- Entries with **no CKB entitlement**: **26**; requiring **4032 CKB** in total. Zero usable own CKB after shannon flooring: **1,354**. This differs from all capacity-blocked accounts. Exact totals are in [capacity-summary.json](refunds/capacity-summary.json).
- All planned payouts fit observed finalized custodian amounts and native source budgets. This does not hide recorded supply mismatches; see [backing.json](assets/backing.json).

## All assets

Rows include zero native definitions. Amounts below are display units, with exact identities and integers in [refund-reconciliation.csv](assets/refund-reconciliation.csv). Planned refunds include capacity-blocked rights; the remaining balance assumes **all** planned rights are eventually paid, not just the currently executable subset. Cells must retain enough occupied capacity at execution time.

| Asset (L2 ID) | Known L2 balances | Custodian | Planned refund | L2 remaining | Custodian remaining |
|---|---:|---:|---:|---:|---:|
| CKB (1) | 427309108.90283169 | 427309108.90283169 | 426641430.31208614 | 667678.59074555 | 667678.59074555 |
| dCKB (34) | 4102124.4978582 | 4102124.4978582 | 4101824.4978582 | 300 | 300 |
| TAI (36) | 395.27933548 | 395.27933548 | 395.27933548 | 0 | 0 |
| ETH (38) | 114.876402178719076576 | 114.876402178719076576 | 114.856401874525649334 | 0.020000304193427242 | 0.020000304193427242 |
| USDT (40) | 165963.82823 | 165963.82823 | 165510.945217 | 452.883013 | 452.883013 |
| DAI (42) | 100.842214593455767964 | 100.842214593455767964 | 99.739356584949061267 | 1.102858008506706697 | 1.102858008506706697 |
| UNVERIFIED_METADATA (44) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (46) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (48) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (50) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (52) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (54) | 0 | 0 | 0 | 0 | 0 |
| BUSD (56) | 74.18 | 74.18 | 73.845976123580531094 | 0.334023876419468906 | 0.334023876419468906 |
| UNVERIFIED_METADATA (58) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (60) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (62) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (64) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (66) | 0 | 0 | 0 | 0 | 0 |
| USDC (68) | 339650.505408 | 339650.505408 | 337613.64221 | 2036.863198 | 2036.863198 |
| UNVERIFIED_METADATA (70) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (72) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (74) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (76) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (78) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (80) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (82) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (84) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (86) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (88) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (90) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (92) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (94) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (96) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (98) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (100) | 0 | 0 | 0 | 0 | 0 |
| BNB/bsc (102) | 38.725017937614102499 | 38.725017937614102499 | 37.907445798378944678 | 0.817572139235157821 | 0.817572139235157821 |
| BUSD/bsc (104) | 628.188193682696278913 | 628.188193682696278913 | 628.188193626530003744 | 0.000000056166275169 | 0.000000056166275169 |
| USDT/bsc (106) | 2121.602254832319079804 | 2121.602254832319079804 | 2121.602254831637889693 | 0.000000000681190111 | 0.000000000681190111 |
| UNVERIFIED_METADATA (108) | 0 | 0 | 0 | 0 | 0 |
| BTCB/bsc (110) | 0.0377870567282726 | 0.0377870567282726 | 0.037784409760391092 | 0.000002646967881508 | 0.000002646967881508 |
| UNVERIFIED_METADATA (112) | 0 | 0 | 0 | 0 | 0 |
| USDC/bsc (114) | 753.205354772713678866 | 753.205354772713678866 | 753.205354654207895911 | 0.000000118505782955 | 0.000000118505782955 |
| UNVERIFIED_METADATA (116) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (118) | 0 | 0 | 0 | 0 | 0 |
| WBTC/eth (120) | 4.78291988 | 4.78291988 | 4.77931387 | 0.00360601 | 0.00360601 |
| YOK (128) | 832021351.884016067451474621 | 832021351.884016067451474621 | 0 | 832021351.884016067451474621 | 832021351.884016067451474621 |
| COOP (161) | 1475.521 | 1475.521 | 1470.521 | 5 | 5 |
| KOI (1570) | 281429376421.935767110930893051 | 281429376421.935767110930893051 | 0 | 281429376421.935767110930893051 | 281429376421.935767110930893051 |

## Detailed records

- [L2 accounts](assets/l2-accounts.csv), [L2 balances](assets/l2-balances.csv), [L2 asset totals](assets/l2-assets.csv), complete account identities and raw observation in [l2-report.json.gz](assets/l2-report.json.gz).
- [Additional unregistered balances](assets/unregistered-balances.csv) discovered by the full DB calculation; these are held, not silently assigned to EOAs. V0 includes them in source totals; V1 keeps them separate from its registered-source census.
- [Custodian cells](assets/custodian-cells.csv), [asset totals](assets/custodian-assets.csv), raw capture in [l1-report.json.gz](assets/l1-report.json.gz). CKB includes capacity occupied by token cells.
- [All beneficiary/asset rows](refunds/accounts.csv), [source claims](refunds/claim-sources.csv), [source partitions and remainders](refunds/source-reserves.csv), [capacity per account](refunds/capacity.csv), [authority identities](refunds/authorities.json), [control refund rows](refunds/control-account-assets.csv), [control direct native holdings](assets/control-native-balances.csv), [complete report](refunds/report.json.gz).
- [Asset policy](asset-treatment.md), [special accounts](account-treatment.md), [lending](lending-treatment.md), [identity/units/deployment](protocol-details.md).

SHA256SUMS files bind published bytes. Decompress report.json.gz into a separate release directory before CLI inspection/export. The archived compression does not modify generator JSON; original uncompressed hashes are in regeneration.json. Synthetic test fixture roots are not production release roots. Choose and publish the deployment ID/profile/plan together following the [Ops runbook](../ops-refund-runbook.md).

Historical schema labels such as IMPLEMENTATION_PENDING or separate decision required are preserved in deterministic output for comparison; consult current authority and treatment documents for actual implementation and already-approved holds. They are not a claim that the tested controller path is unimplemented or a request to reopen every held account.

## Recorded supply discrepancy

The native recorded CKB total supply exceeds the known registered plus history-discovered balances. Those known balances, not an invented beneficiary for the difference, determine source-backed refund claims. `refund-reconciliation.csv` exposes both; `backing.json` intentionally reports the mismatch. Additional unregistered balances are held and included in this overview's known L2 total, separately from the registered-source CSV. The previously deferred supply discrepancy is not resolved by this documentation refresh.
