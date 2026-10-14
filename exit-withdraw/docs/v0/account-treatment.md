# V0 special accounts and beneficiary treatment

## Approved original authorities

These are separately authenticated authorities, not a rule that every administrator owns user funds. Signer sets are freshly read and checked during DB generation; [authorities.json](refunds/authorities.json) is authoritative. Personal EOA balances are never duplicated as contract rights. The plan retains group identities for multisignature accounts; reviewed sole-ETH-owner exits may be attributed to that owner by the approved generator.

| Account | Authority type | Refund threshold | Authenticated owners (account ID: ETH address) |
|---|---|---|---|
| 16045 | signer-wallet | 2 of 3 distinct | 18802: `0x7e2cd50ff305780450df7d8a02b6bd2b4878c3d8`; 16058: `0x27837f6036d94b107e203e535d991afa544d0b3d`; 18: `0x666666488c608edec79d6f85094e6e514a67064f` |
| 36620 | classic | 2 of 3 distinct | 36622: `0x2dc726a69d6d3495eccb7b5666bccc6153589277`; 36623: `0x6d6a162dbbfa724ca953509c0a6cec18ef8abceb`; 36625: `0x19337d112e9a494986221237e4b7c2fa38f2b567` |
| 36630 | classic | 2 of 3 distinct | 36622: `0x2dc726a69d6d3495eccb7b5666bccc6153589277`; 36623: `0x6d6a162dbbfa724ca953509c0a6cec18ef8abceb`; 36625: `0x19337d112e9a494986221237e4b7c2fa38f2b567` |
| 38316 | safe | 2 of 3 distinct | 38636: `0xc207a5dc49771a71367bd3a97fe58aaf9c5dfdae`; 13: `0x9c023610d438de45b8d53358de663233ce752f77`; 38429: `0xbb91644f26b075bda47a13682dad006eb9d70867` |
| 48851 | signer-wallet | 2 of 3 distinct | 48852: `0xbc350b51742711c962ffe8df9584124e9015f32f`; 48853: `0xff164b51ffecde88198af33fe579925e91c28da5`; 44091: `0xdf6ab9f6daf25dbd31513a6ccc441ccfb210a53c` |

For signer wallets whose old logic allowed caller and cosigner to coincide, the approved exit requires **two different owners**. Safe 2-of-4 and 3-of-6 policies are not reduced. Controller proposals use real ETH OmniLock input signatures over one common transaction and a common destination. Authorization cells return unchanged. Only a root-committed payable right can be used; governance or a project admin cannot add held funds to the payout.

## Deferred control/project accounts

| Account | Recorded authority | Treatment and reason |
|---|---|---|
| 36838 | manager: 0x72e0c4199049f6143afb808104f804f31ca30540 | Retain project custody; controller identity does not establish a generic user redemption right. |

## Receipt and protocol accounts

| Account/group | Beneficiary rule | Retained component |
|---|---|---|
| WCKB and Yokai pairs/farms | Authenticate receipt supply and holders; unwrap current principal once to supported native assets | Unknown/locked supply, floors and excluded YOK/KOI |
| Stable pools 38602, 38606, 48394, 48666, 48669, 49392 | Proportional recorded coin balances; LP supply must reconcile | Rounding and any non-user backing |
| Staking 48397, 48667, 48670, 49393 | User principal must sum to LP actually held; then follow LP underlying | Separate reward streams |
| IFO 19202, 29696, 31272, 36023, 36024, 47794, 47796 | Only unharvested refunds, exact deployed formula then approved pool-local pro rata | Accepted sale proceeds, harvested rights, floor dust |
| IFO 49401 | No unharvested user refunds at the reviewed snapshot | Residual/project LP |
| Vaults 2325, 8788, 9782, 23992 | Reviewed positions are excluded old-YOK staking; no eligible non-YOK LP route established | Whole-token YOK scope exclusion |
| Treasury 36838 | No approved DRM-holder redemption; manager 35969 alone does not create beneficial entitlement | Project treasury, including 4,000 USDC; reviewed debt zero |
| FeeTo 16045 | Supported pending protocol-fee shares are approved in addition to actual holdings | V0 YOK fee side still excluded |

Every remaining source is itemized, with the reason and amount, in [source-reserves.csv](refunds/source-reserves.csv). The rows include system balances and unmapped short-address reserves rather than silently treating them as EOAs.

## Evidence and reopening

The compact [control register](evidence/control-review.json) preserves identities/roles for included and held accounts. Current refund authority is the freshly generated register, not a historical project label. Bytecode/storage evidence is retained alongside it; the generator independently rechecks payable authorities via [authority.rs](../../src/refund/authority.rs). Held rights may be revisited with evidence and an approved scope change; they are not unrestricted project surplus.

## Reading control amounts

[Control refund rows](refunds/control-account-assets.csv) show the planned eligible rights; a zero refund does not mean the account holds no assets. [Direct native holdings](assets/control-native-balances.csv) separately show balances for reviewed control accounts. Those native holdings exclude indirect LP/vault/lending receipts; their final eligible or retained backing is traced in claim-sources.csv, source-reserves.csv and, for V1, retained-lots.json. Do not add direct holdings to already inclusive refund entitlements.
