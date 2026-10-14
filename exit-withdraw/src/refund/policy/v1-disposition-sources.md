# V1 embedded refund policy provenance

This catalog freezes already approved decisions at account root
`0x5aedd0572ffb113b8b2ee3c850b1e56aede782a2eb0be41fe4b5f3033495caee`.
It contains identities and decisions, not balances, share weights, debts, prices,
or previous refund amounts. Monetary values are reconstructed from the DB.

- `v1_disposition.rs`: the 62 account IDs under `LOW_VALUE` are the approved
  whole-portfolio screen in `docs/v1/account-treatment.md`.
  Only otherwise unresolved residues are held. Known principal routes continue
  first. The program does not pretend this is a current USD valuation.
- The eight research exclusions include the explicit 12394 exception and NFT
  deferrals. 12697's unregistered outgoing LP recipient has its own low-value
  hold; unrelated sender balances remain eligible.
- WBTC account 121's self-held WBTC is pending manual claim verification.
  Account 5826's unclaimed CKB rewards and 3172's idle YOK rewards are excluded.
  See the confirmed policy decisions in the same resolution document.
- Sale custody 4466, 12228 and 12229 remains held. Later approved owner exits
  4573, 5793, 4075 and 11784 are separately authenticated in `authority.rs`.
- Controls 4831, 5713, 10288 and 10317 remain held. 10317's CKB is a retained
  low-value residue; no controller is inferred as a beneficiary.
- Zero and dead addresses are locked/retained even if registered as ETH EOAs.
  Refer to `docs/v1/asset-treatment.md` and the wrapper reviews.
- Relayers 11736 and 12028 remain deferred; the rational wrapper calculator
  preserves their fractions rather than reallocating them to others.
- V1 YOK principal is eligible; pending bonuses are not. KOI is outside scope.
  Native asset ID/type-hash pairs in `v1-assets.json` are checked against DB.
- Lending uses the approved pool-local reserve-first, treasury-second,
  ordinary-equity-last waterfall. Old 4757 and tiny 3635/10285 remain excluded.
  Four later wrapper rights are CKB-only; 11560/11567 retain all supported assets.

`v1-assets.json` is display/identity metadata only. V1 CKB has 18 L2 decimals,
converted to 8 L1 decimals after aggregating all claims for an account. No L1
custody quantity or observation is embedded in refund generation.

Unreviewed positive residuals cause an error, including unfamiliar unregistered
addresses. A new snapshot or new disposition needs reviewed policy changes.
