# Approved V1 lending refund policy and complete loss waterfall

## Latest decision

The user approved proportionate refunds after same-pool debt netting and first-loss protocol buffers. **10659 is approved for this allocation policy, not whole-pool exclusion.** Pools **3635 and 10285** are ignored because their remaining creditor net equity is very small. Old pool **4757** remains excluded under the earlier separate decision. No general low-value cutoff for all pools or personal accounts is introduced.

This is the current accounting policy. It supersedes the pending-decision status of 10659 and the historical whole-pool lending hold as a policy decision. The current DB-only generator integrates reviewed beneficiary paths. Actual payment still requires CKB capacity, a published release and live custody checks. Nothing has been paid on chain.

## Refund order and terminology

1. **Net each borrower's same-pool deposits/collateral against their debt**, including the indexed contractual amounts in the frozen scenario. Negative-net accounts have no pool payout; any uncovered borrowing remains visible in the debt register.
2. **Use verified same-pool protocol buffers first.** Explicit reserves absorb the deficit first in this calculation, followed by verified protocol treasury receipt equity. Protocol aTokens are already-counted claims: writing them down cannot also add their value to cash. The user's fee/reserve terminology refers to these verified lending buffers, not unrelated NFT royalties (which remain deferred), outside project funds or other pools.
3. **Apply the two specifically approved low-equity pool exclusions.** 3635 and 10285 pay zero and retain all balances and debts. Their counterfactual buffer deductions are shown for comparison, not represented as an executed write-off.
4. **Allocate residual loss to eligible positive net creditors of each remaining pool proportionately**, and distribute the actual supported asset basket. The ratio is calculated after protocol buffers. A USDC depositor may receive some CKB or another supported pool asset. No external repayment, token sale, liquidation bonus or exit fee is assumed.
5. **Preserve excluded rights, unexplained balances and rounding residues.** Excluded contract rights and unapproved attribution fractions remain held; approved nested beneficiaries are resolved by the current adapters. Users must provide any missing CKB capacity; liquidity value is not capacity.

“Refundable net equity” below means the calculated native-asset basket after losses and native-unit floors, not an already executable transfer or a fresh dollar promise. The basis remains V1 block 2,851,258 and the approved frozen price scenario (asset price timestamps 2025-11-04).

## Pool balance sheets

Displayed quote figures are truncated to six decimals for readability; CSV/JSON retain full precision and native quantities. Borrower debt is owed **to** the pool; gross deposit liability is owed **by** the pool. Positive net creditor equity is calculated holder by holder and excludes the first-loss protocol treasury. These are distinct figures and should not be added as independent assets.

| Pool | Actual cash | Borrower debt receivable | Gross deposit liability | Non-treasury positive net equity |
|---|---:|---:|---:|---:|
| 3635 | 3.192634 | 399.421316 | 20.309070 | 17.209055 |
| 4828 | 9.431591 | 1.041713 | 10.473304 | 9.431591 |
| 5710 | 2.792980 | 1.057072 | 3.850051 | 2.792971 |
| 5790 | 1976.462674 | 176.414511 | 2119.949579 | 1971.450585 |
| 10201 | 0.064417 | 0.242332 | 0.306749 | 0.064417 |
| 10285 | 0.136343 | 0.630451 | 0.766776 | 0.300635 |
| 10314 | 0.661810 | 0.951570 | 1.613376 | 0.661806 |
| 10659 | 1346327.808437 | 503109.934781 | 1847203.294555 | 1345626.540712 |

## Buffer deductions and remaining refunds

| Pool | Explicit reserve before deduction | Reserve absorbs | Treasury equity before deduction | Treasury absorbs | Residual user loss before rounding | Policy refundable equity after native rounding | Policy recovery before rounding |
|---|---:|---:|---:|---:|---:|---:|---:|
| 3635 (ignored) | 0.000000 | 0.000000 | 0.048540 | 0.048540 | 14.147917 | 0.000000 | 0% |
| 4828 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 9.431590 | 100.000000% |
| 5710 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 2.792969 | 100.000000% |
| 5790 | 32.927604 | 27.915517 | 0.000000 | 0.000000 | 0.000000 | 1971.450555 | 100.000000% |
| 10201 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.064417 | 100.000000% |
| 10285 (ignored) | 0.000017 | 0.000017 | 0.000000 | 0.000000 | 0.164292 | 0.000000 | 0% |
| 10314 | 0.000003 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.661804 | 100.000000% |
| 10659 | 0.000000 | 0.000000 | 2599.469821 | 2599.469821 | 1533.185210 | 1344093.342654 | 99.886061% |

For ignored pools, the deductions/loss columns are counterfactual diagnostics from the preserved trial; actual planned payment is zero and their full source balances remain held. Tiny sub-millionth quote amounts may display as zero; see exact values in pools.csv. The table also records `ordinary_shortfall_including_rounding_quote`, separating pre-rounding financial loss from final native-unit allocation shortfall.

Main Aave **10659**: verified treasury equity absorbs **2599.46982168267310593165727629** quote units. Remaining user loss is **1533.18521004451513244351772371** before rounding. The calculated non-treasury refundable basket is **1344093.342654520400069701437465605** quote units; native-unit rounding raises total user reduction to **1533.198058352620421465588736135**. Recovery is **99.886061%** before rounding. This is not a request to contribute 1,533 USDC.

Pool **5790**: explicit reserves of **32.92760496530947688611087779** absorb **27.915517228078224424595304285**. Positive net creditor value is fully covered before rounding. The residual user reduction of **0.000029982070655311446567625** is rounding. Treasury positions 10652 and 3628 were identified through fixed-block getters and mint/deposit history; see the evidence and reserve-first calculation.

Unexplained positive accounting components remain excluded from available buffers. Main Aave's **1,906.203803 USDC** and **328.472644 USDT** components are not treated as confirmed protocol income. No unrelated treasury is used to erase this remaining uncertainty.


## Reproduction and exact records

The DB-only generator recalculates indexed balances, debt, frozen prices, protocol buffers and source-constrained distributions. [Pool calculation code](../../src/refund/v1_closeout.rs) and [lending getters](../../src/refund/v1_lending.rs) are authoritative. Final beneficiary/source lots and retained reasons are in `refunds/`; current policy worksheets in `evidence/lending/` preserve the exact reviewed balance sheet and debt/loss explanation. The worksheets are supporting evidence, never inputs to `refund generate`. Their pool-level allocation precedes nested beneficiary exclusions, so their recipient totals must not be substituted for the final refund table. Contract look-through and later CKB-only exclusions are recorded separately.

Old pool 4757's administrative liquidity receipt is not simply removed to increase other creditors' coverage. The entire pool remains excluded by explicit decision. No balance or debt is deleted by documenting exclusion. No V1 pool ID or haircut is transferred to V0 automatically.

### Reading the supporting worksheets

- [pools.csv](evidence/lending/pools.csv): cash, debt receivable, deposit liability, reserves, treasury write-down, user loss and recovery.
- [markets.csv](evidence/lending/markets.csv): the same accounting per native asset.
- [accounts.csv](evidence/lending/accounts.csv): holder deposits, debt and net position.
- [allocation-lots.csv](evidence/lending/allocation-lots.csv): pool-level allocation before final nested-beneficiary/scope treatment.
- [source-balances.csv](evidence/lending/source-balances.csv): source conservation and protected components.

These are the final approved frozen-policy supporting worksheets, not a new alternative scenario and not evidence of executed liquidation. Use the freshly generated full report for actual beneficiary entitlements. Its source accounting includes the subsequent owner and wrapper decisions.
