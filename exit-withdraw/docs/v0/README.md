# V0 current asset and refund overview

Regenerated from the original read-only DB and embedded rules on 14 October 2026. Exact snapshot/observation anchors are in [assets/manifest.json](assets/manifest.json). No deployment or payment was performed. The fresh complete report equals the previous final table; see [regeneration comparison](refunds/regeneration.json).

- Beneficiary/capacity entries: **19,303**; entries with positive payable assets: **19,303**. These are accounts, not necessarily distinct people.
- Capacity-blocked entries: **8,008**, requiring **544213.09829905 CKB** in the theoretical full-basket scenario.
- Entries with **no CKB entitlement**: **735**; requiring **122256 CKB** in total. Zero usable own CKB after shannon flooring: **735**. This differs from all capacity-blocked accounts. Exact totals are in [capacity-summary.json](refunds/capacity-summary.json).
- All planned payouts fit observed finalized custodian amounts and native source budgets. This does not hide recorded supply mismatches; see [backing.json](assets/backing.json).

## All assets

Rows include zero native definitions. Amounts below are display units, with exact identities and integers in [refund-reconciliation.csv](assets/refund-reconciliation.csv). Planned refunds include capacity-blocked rights; the remaining balance assumes **all** planned rights are eventually paid, not just the currently executable subset. Cells must retain enough occupied capacity at execution time.

| Asset (L2 ID) | Known L2 balances | Custodian | Planned refund | L2 remaining | Custodian remaining |
|---|---:|---:|---:|---:|---:|
| CKB (1) | 259757215.87278411 | 259757215.87278411 | 259644798.81781875 | 112417.05496536 | 112417.05496536 |
| ETH (19) | 207.956131817159791117 | 207.956131817159791117 | 207.956131817066031581 | 0.000000000093759536 | 0.000000000093759536 |
| USDT (172) | 23531.564144 | 23531.564144 | 23531.54343 | 0.020714 | 0.020714 |
| UNVERIFIED_METADATA (173) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (174) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (175) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (176) | 0 | 0 | 0 | 0 | 0 |
| DAI (177) | 13915.015464798029455125 | 13915.015464798029455125 | 13915.015464798009169453 | 0.000000000020285672 | 0.000000000020285672 |
| LINK (178) | 0 | 0 | 0 | 0 | 0 |
| BAND (179) | 0 | 0 | 0 | 0 | 0 |
| BAT (180) | 234.1 | 234.1 | 234.1 | 0 | 0 |
| SNX (181) | 0 | 0 | 0 | 0 | 0 |
| UNI (182) | 0 | 0 | 0 | 0 | 0 |
| YFI (183) | 0 | 0 | 0 | 0 | 0 |
| BUSD (184) | 0 | 0 | 0 | 0 | 0 |
| COMP (185) | 0 | 0 | 0 | 0 | 0 |
| MKR (186) | 0 | 0 | 0 | 0 | 0 |
| IOTX (187) | 0 | 0 | 0 | 0 | 0 |
| BEL (188) | 0 | 0 | 0 | 0 | 0 |
| PAX (189) | 0 | 0 | 0 | 0 | 0 |
| USDC (190) | 218532.477912 | 218532.477912 | 210031.213593 | 8501.264319 | 8501.264319 |
| SXP (191) | 0 | 0 | 0 | 0 | 0 |
| UNVERIFIED_METADATA (192) | 0 | 0 | 0 | 0 | 0 |
| SUSHI (193) | 0 | 0 | 0 | 0 | 0 |
| AAVE (194) | 0 | 0 | 0 | 0 | 0 |
| 1INCH (195) | 0 | 0 | 0 | 0 | 0 |
| LTO (196) | 0 | 0 | 0 | 0 | 0 |
| MATIC (197) | 0 | 0 | 0 | 0 | 0 |
| BNT (198) | 0 | 0 | 0 | 0 | 0 |
| BZRX (199) | 0 | 0 | 0 | 0 | 0 |
| MDT (200) | 0 | 0 | 0 | 0 | 0 |
| PNT (201) | 0 | 0 | 0 | 0 | 0 |
| PERL (202) | 0 | 0 | 0 | 0 | 0 |
| BAL (203) | 0 | 0 | 0 | 0 | 0 |
| BLZ (204) | 0 | 0 | 0 | 0 | 0 |
| COTI (205) | 0 | 0 | 0 | 0 | 0 |
| TRX (206) | 0 | 0 | 0 | 0 | 0 |
| TUSD (207) | 0 | 0 | 0 | 0 | 0 |
| dCKB (249) | 379306.92967565 | 379306.92967565 | 379306.92731519 | 0.00236046 | 0.00236046 |
| TAI (253) | 2048.55742017 | 2048.55742017 | 2048.55644638 | 0.00097379 | 0.00097379 |
| UNVERIFIED_METADATA (30551) | 0 | 0 | 0 | 0 | 0 |
| BNB/bsc (30634) | 705.838148698589834329 | 705.838148698589834329 | 705.838148698549973656 | 0.000000000039860673 | 0.000000000039860673 |
| UNVERIFIED_METADATA (30664) | 0 | 0 | 0 | 0 | 0 |
| BUSD/bsc (30667) | 707.54759916 | 707.54759916 | 707.547599159999999999 | 0.000000000000000001 | 0.000000000000000001 |
| USDT/bsc (30668) | 423.754196144168137613 | 423.754196144168137613 | 423.754196142059278804 | 0.000000002108858809 | 0.000000002108858809 |
| ETH/bsc (30669) | 0.261351211109313721 | 0.261351211109313721 | 0.261351211109313708 | 0.000000000000000013 | 0.000000000000000013 |
| BTCB/bsc (30670) | 0.010582844834226587 | 0.010582844834226587 | 0.010582844724046387 | 0.0000000001101802 | 0.0000000001101802 |
| WBNB/bsc (30671) | 0.156112411024188669 | 0.156112411024188669 | 0.156112410324492961 | 0.000000000699695708 | 0.000000000699695708 |
| USDC/bsc (30672) | 36810.26386626606781735 | 36810.26386626606781735 | 36810.263866264816940962 | 0.000000001250876388 | 0.000000001250876388 |
| SFP/bsc (30673) | 0 | 0 | 0 | 0 | 0 |
| iBFR/bsc (37688) | 67.2 | 67.2 | 67.2 | 0 | 0 |
| WBTC/eth (49361) | 0.04893924 | 0.04893924 | 0.04893869 | 0.00000055 | 0.00000055 |
| YOK (49721) | 134427780.094408200118671748 | 134427780.094408200118671748 | 0 | 134427780.094408200118671748 | 134427780.094408200118671748 |
| KOI (49787) | 108988084032.923953898964599683 | 108988084032.923953898964599683 | 0 | 108988084032.923953898964599683 | 108988084032.923953898964599683 |

## Detailed records

- [L2 accounts](assets/l2-accounts.csv), [L2 balances](assets/l2-balances.csv), [L2 asset totals](assets/l2-assets.csv), complete account identities and raw observation in [l2-report.json.gz](assets/l2-report.json.gz).
- [Additional unregistered balances](assets/unregistered-balances.csv) discovered by the full DB calculation; these are held, not silently assigned to EOAs. V0 includes them in source totals; V1 keeps them separate from its registered-source census.
- [Custodian cells](assets/custodian-cells.csv), [asset totals](assets/custodian-assets.csv), raw capture in [l1-report.json.gz](assets/l1-report.json.gz). CKB includes capacity occupied by token cells.
- [All beneficiary/asset rows](refunds/accounts.csv), [source claims](refunds/claim-sources.csv), [source partitions and remainders](refunds/source-reserves.csv), [capacity per account](refunds/capacity.csv), [authority identities](refunds/authorities.json), [control refund rows](refunds/control-account-assets.csv), [control direct native holdings](assets/control-native-balances.csv), [complete report](refunds/report.json.gz).
- [Asset policy](asset-treatment.md), [special accounts](account-treatment.md), [lending](lending-treatment.md), [identity/units/deployment](protocol-details.md).

SHA256SUMS files bind published bytes. Decompress report.json.gz into a separate release directory before CLI inspection/export. The archived compression does not modify generator JSON; original uncompressed hashes are in regeneration.json. Synthetic test fixture roots are not production release roots. Choose and publish the deployment ID/profile/plan together following the [Ops runbook](../ops-refund-runbook.md).

Historical schema labels such as IMPLEMENTATION_PENDING or separate decision required are preserved in deterministic output for comparison; consult current authority and treatment documents for actual implementation and already-approved holds. They are not a claim that the tested controller path is unimplemented or a request to reopen every held account.
