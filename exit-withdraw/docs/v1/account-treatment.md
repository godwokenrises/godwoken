# V1 special accounts and beneficiary treatment

## Approved original authorities

These are separately authenticated authorities, not a rule that every administrator owns user funds. Signer sets are freshly read and checked during DB generation; [authorities.json](refunds/authorities.json) is authoritative. Personal EOA balances are never duplicated as contract rights. The plan retains separate contract identities for both multisignature accounts and reviewed sole-owner exits; a sole owner authorizes its 1-of-1 contract allocation.

| Account | Authority type | Refund threshold | Authenticated owners (account ID: ETH address) |
|---|---|---|---|
| 225 | signer-wallet | 2 of 3 distinct | unregistered: `0x7e2cd50ff305780450df7d8a02b6bd2b4878c3d8`; unregistered: `0x27837f6036d94b107e203e535d991afa544d0b3d`; 122: `0x666666488c608edec79d6f85094e6e514a67064f` |
| 253 | signer-wallet | 2 of 3 distinct | unregistered: `0x7e2cd50ff305780450df7d8a02b6bd2b4878c3d8`; unregistered: `0x27837f6036d94b107e203e535d991afa544d0b3d`; 122: `0x666666488c608edec79d6f85094e6e514a67064f` |
| 263 | signer-wallet | 2 of 3 distinct | unregistered: `0x7e2cd50ff305780450df7d8a02b6bd2b4878c3d8`; unregistered: `0x27837f6036d94b107e203e535d991afa544d0b3d`; 122: `0x666666488c608edec79d6f85094e6e514a67064f` |
| 3943 | safe | 2 of 4 distinct | 6852: `0x6538d3d3737b653bc7ca4f1061e6e0a1dfd8f3af`; 4746: `0x2661a4786ea12b7edfd3f5c08ff617702360a993`; 4808: `0xd5f7c76eeba710023e620f8770f6d8c4acdd1452`; 130: `0x9c023610d438de45b8d53358de663233ce752f77` |
| 10506 | safe | 3 of 6 distinct | 11305: `0xcb3c4f5f817604f3c8b5c7e1ecab480279c449d0`; 11293: `0xa14d06ee65f19ea64548ca2effcc26925823827d`; 12032: `0x2072e365400d42ae5b4c4145e0a98d65fbb526e1`; 8091: `0xe7d5869fe1955f2500987b9eccff0a9452c164cf`; 6852: `0x6538d3d3737b653bc7ca4f1061e6e0a1dfd8f3af`; 4746: `0x2661a4786ea12b7edfd3f5c08ff617702360a993` |
| 11784 | single-owner | 1 of 1 distinct | 11402: `0x9b8d70b41c0b1e69b739f60151f2d14127ecc165` |
| 4573 | single-owner | 1 of 1 distinct | 122: `0x666666488c608edec79d6f85094e6e514a67064f` |
| 5793 | single-owner | 1 of 1 distinct | 5662: `0x00c69071a6ad8a331d70d2f5cca5c09efc355ea4` |
| 4075 | fixed-recipient-executor | 1 of 1 distinct | 3559: `0xb156c425f325d750968702d4bd4226573edc3aed` |

For signer wallets whose old logic allowed caller and cosigner to coincide, the approved exit requires **two different owners**. Safe 2-of-4 and 3-of-6 policies are not reduced. Controller proposals use real ETH OmniLock input signatures over one common transaction and a common destination. Authorization cells return unchanged. Only a root-committed payable right can be used; governance or a project admin cannot add held funds to the payout.

## Deferred control/project accounts

| Account | Recorded authority | Treatment and reason |
|---|---|---|
| 4466 | Owner: 0x666666488c608edec79d6f85094e6e514a67064f | Completed sale/project custody held; any independently proven user refund is separately accounted. |
| 4831 | owner: 0x65698468f12a25885602b9f925b0f5e09e4857e9 | Retain project custody; controller identity does not establish a generic user redemption right. |
| 5707 | owner: 0x00c69071a6ad8a331d70d2f5cca5c09efc355ea4 | YOK remains retained under the whole-token V1 exclusion; preserve its balance for reconciliation, no refund. |
| 5713 | owner: 0xcaebe8353b11b23546ad51ec7a51bf3f673e47dc | Retain project custody; controller identity does not establish a generic user redemption right. |
| 10288 | owner: 0xcaebe8353b11b23546ad51ec7a51bf3f673e47dc | Retain project custody; controller identity does not establish a generic user redemption right. |
| 10317 | owner: 0xcaebe8353b11b23546ad51ec7a51bf3f673e47dc | Retain project custody and low-value residual rights; no payout to owner by default. |
| 12228 | Owner: 0xa9bd87fbdd609bfb170e99da2bf3c8a23760ead2 | Completed sale/project custody held; any independently proven user refund is separately accounted. |
| 12229 | Owner: 0xa9bd87fbdd609bfb170e99da2bf3c8a23760ead2 | Completed sale/project custody held; any independently proven user refund is separately accounted. |

## Owner-specific approvals

| Source | Approved control | Rule |
|---|---|---|
| 11784 | Sole owner 11402 | Owner-only withdrawal authority; operator 11805 is not a cosigner. This does not settle historical funders' economic disputes. |
| Tanuki 4573 | Owner 122 | Approved original owner withdrawal of completed-sale proceeds; supersedes its historical project-funds hold. |
| Treasury 5793 | Owner 5662 | Approved original owner exit, with its lending-derived rights computed after pool closeout. |
| Executor 4075 | Fixed controller 3559 | Original two enabled executors are 3559 and 3551; this is not a 2-of-2 wallet. Original withdrawal always pays controller 3559. Only 3559 receives the approved exit authority; 3551 cannot redirect. |

## Receipt and protocol accounts

| Account/group | Beneficiary rule | Retained component |
|---|---|---|
| Yokai LP and farms, including 265 | Trace exact current LP principal through actual backing and fee-adjusted supply | Locked shares, rounding, unapproved pending fee rights and separate rewards |
| Brainiac Vault/strategy pairs, including Vault 3171 / strategy 3172 | Verify reciprocal pointers, implementation, holder supply and idle LP + staked principal; allocate by current vault shares | Empty-share vault 4223 residue; strategy reward path is separate |
| Stable LP and staking | Verify coin identities, LP supply, user principal and source budgets | Admin/unsupported/rounding reserves |
| IFO 5731, 5732, 11852, 11853 | Exact deployed unharvested-refund formula, then only these four pools' approved pro-rata actual-asset allocation | Non-refund balance and floors |
| Static aToken wrappers 11560 / 11567 | Current net lending basket by authenticated static share; recursively trace Vault/BPT holders using exact rational fractions | Locked, deferred and rounding shares; proxy upgrade admin is not the beneficiary |
| Balancer Vault 10711 and BPT pools | BPT is a pool-share receipt, not a new custodian asset; partition Vault holdings by pool and internal account, then trace beneficial shares | Unbooked-interest fraction and excluded/unknown beneficiaries; no double-counting of Vault backing |
| Wrappers 10735, 10737, 11740 and extra 10711 rights | Approved incremental CKB-only allocation after lending closeout and receipt look-through | Newly traced non-CKB components remain held |
| DAO/guild and custom custody | Read deployed withdrawable balance/share rules; trace the verified entitled member, not simply the admin | Locked, unsupported or unverified residual rights |
| Marketplace 4572 | Separate verified refundable balances from creator royalties | Royalties remain deferred |
| WBTC 121 | Original transfer source known, but no automatic redemption right | Manual claim review for 0.0035 WBTC |
| 5826 / 3172 | Keep independently verified non-YOK principal routes | Unclaimed CKB / idle YOK bonuses excluded |
| 11736 / 12028 | Relayer ownership not inferred from custody | Whole-portfolio low-value deferral |
| 12394 and unregistered LP case linked to 12697 | Explicit low-value exceptions | Retain scoped assets; do not exclude unrelated 12697 positions |

The full generated [retained-lots.json](refunds/retained-lots.json) contains every held lot, reason, path and native amount; [source-reserves.csv](refunds/source-reserves.csv) partitions each backing source. Supplementary unregistered native balances are separately enumerated in `refunds/report.json.gz` and the asset views.

### Approved unresolved-residue account lists

These lists apply only after proven principal routes. They are not a rule to seize or discard the entire account, nor a fresh USD valuation.

Approved USD-100 research deferrals: 9, 10, 12, 16, 18, 19, 33, 133, 166, 168, 169, 228, 230, 231, 1792, 1811, 2070, 2076, 2315, 2319, 2357, 3174, 3176, 3178, 3180, 3182, 3335, 3463, 3477, 4224, 4299, 4361, 5731, 5830, 5832, 6367, 7789, 9947, 10098, 10099, 10103, 10108, 10110, 10111, 10934, 11064, 11066, 11068, 11070, 11072, 11119, 11120, 11222, 11484, 11580, 11909, 11957, 12145, 12147, 12282, 12562, 13044.

Other approved research-scope/NFT cases: 7667, 9469, 9427, 11992, 11353, 12043, 987, 12394. Additional asset-specific exceptions and unmapped identities are explicit in [v1_disposition.rs](../../src/refund/v1_disposition.rs).

## Evidence and reopening

The compact [control register](evidence/control-review.json) preserves identities/roles for included and held accounts. Current refund authority is the freshly generated register, not a historical project label. Bytecode/storage evidence is retained alongside it; the generator independently rechecks payable authorities via [authority.rs](../../src/refund/authority.rs). Held rights may be revisited with evidence and an approved scope change; they are not unrestricted project surplus.

## Reading control amounts

[Control refund rows](refunds/control-account-assets.csv) show the planned eligible rights; a zero refund does not mean the account holds no assets. [Direct native holdings](assets/control-native-balances.csv) separately show balances for reviewed control accounts. Those native holdings exclude indirect LP/vault/lending receipts; their final eligible or retained backing is traced in claim-sources.csv, source-reserves.csv and, for V1, retained-lots.json. Do not add direct holdings to already inclusive refund entitlements.
