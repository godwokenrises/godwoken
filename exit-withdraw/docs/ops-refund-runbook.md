# Godwoken V0 / V1 refund operations

Updated 14 October 2026. This runbook describes the implemented refund release,
validator upgrade, root initialization, payout and wallet-claim workflows. No mainnet
upgrade or payout has been executed by these tests. Use separate directories,
plans, profiles, deployments and proposal queues for **V0 and V1**. The portal
selects V1 by default and processes only the explicitly selected version.

## What is authorized

The complete generated `report.json` is the canonical refund table. The CSVs
are readable views. Its amounts follow the approved protocol closeout rules;
held/excluded assets do not become payable through an administrator signature.
EOA allocations pay an exact standard mainnet ETH OmniLock. Reviewed control
accounts retain the committed distinct-owner threshold. Approved single-owner contract exits retain a separate contract allocation with
a 1-of-1 descriptor for the verified owner. They are not merged into, or paid a
second time through, that owner's personal EOA allocation.

The contract commits the source, reserve and claim trees through settlement
metadata in the rollup's account SMT. The updated **account root remains in the
existing rollup cell**. One settlement state root commits immutable account-page plans and cumulative paid
records and source/asset budgets prevent double payment. Each transition verifies
both old and new SMT roots. GlobalState encoding is preserved for compatibility
with the existing custodian scripts. The settlement state root must not simply replace the
account root: these are different commitments.

The upgraded V0/V1 binaries accept refund lifecycle actions only. They reject
ordinary block submission, deposits through block execution, challenges,
reverts and legacy direct-balance exits. Initialization commits the approved unified allocation root and asset budgets in one
transaction. There is no per-account registration stage. Only Begin (once) and
Pay are accepted; the action enum contains only Begin and Pay.

There are three different authorities:

| Action | Required authority |
|---|---|
| Replace the TYPE_ID validator code cell | Its existing CKB multisig, authenticated by the exact config preimage |
| Initialize roots/pay refunds | Pinned rollup governance; V0 bootstrap additionally uses the two existing PoA producers and retires the helper locks |
| Spend EOA payout / authorize control payout | The ETH owner / committed distinct ETH controller threshold |

V0's previously observed code-upgrade lock is 3-of-5. Never substitute the
2-of-3 *beneficiary* policy for code-upgrade governance. Both versions' current
code cells must be resolved live; historical outpoints are not execution inputs.

## 1. Build and reproduce the table

Use Rust 1.94.0 and the checked-in lockfile. From `exit-withdraw/`:

```sh
cargo +1.94.0 build --locked --release --bin gw-withdraw
# Define CLI as the absolute path to the resulting executable.
CLI=/path/to/gw-withdraw
"$CLI" refund generate --protocol v1 --db /path/to/v1-db --output-dir ./release-v1/tables
"$CLI" refund generate --protocol v0 --db /path/to/v0-db --output-dir ./release-v0/tables
```

DBs open read-only. Generation uses the DB and embedded reviewed rules, not an
old refund table. Outputs must be new and outside the DB. Preserve the output
checksums. Original DB paths and host operating notes belong in the machine
inventory, not this repository. Mainnet-sized generation can take tens of minutes.

Inspect all assets, or select a beneficiary:

```sh
"$CLI" refund inspect --protocol v1 --report ./release-v1/tables/report.json \
  --account-id 263 --output ./release-v1/account-263.json
"$CLI" refund inspect --protocol v0 --report ./release-v0/tables/report.json \
  --eth-address 0xYOUR_ETH_ADDRESS --output ./release-v0/account.json
```

Inspection reports theoretical rights and capacity information. It does not
claim the rights are still unpaid on L1; `settlement status` supplies live paid
state later. Amounts are exact integer strings. V0 CKB uses 8 decimals; V1 L2 CKB
uses 18 decimals and converts to L1 shannons at 10^10 L2 units per shannon.

Query backing independently:

```sh
"$CLI" l2-db --protocol v1 --db /path/to/v1-db --output ./release-v1/l2.json
"$CLI" l1-custodian --protocol v1 --output ./release-v1/l1.json
"$CLI" check-backing --l2-report ./release-v1/l2.json \
  --l1-report ./release-v1/l1.json --output ./release-v1/backing.json
jq '.observation' ./release-v1/l1.json > ./release-v1/l1-snapshot.json
```

Repeat with `--protocol v0`. The backing command deliberately retains its review
exit code for the documented supply/census limitations. Read the report; an exit
code alone is not a payout approval. Profile creation and transaction preparation
separately require the unpaid allocation to fit finalized custodian assets,
retained-cell capacity and native state.

## 2. Freeze the release and publish its profile

Choose a fresh 32-byte deployment ID and record it in the release record. Do not
use IDs from synthetic tests. Export with the exact generated JSON and DB:

```sh
"$CLI" refund export-plan --protocol v1 --db /path/to/v1-db \
  --report ./release-v1/tables/report.json --deployment-id 0xFRESH_32_BYTES \
  --output ./release-v1/plan.json
"$CLI" settlement --protocol v1 verify-plan --plan ./release-v1/plan.json
```

Export authenticates source balances and identities against the DB and checks
all source partitions and beneficiary totals. It merges duplicate explanatory
routes only when source, beneficiary and asset are identical. It includes the
report checksum and commits the exact controller descriptor (rollup, account,
threshold and sorted distinct ETH owners) for every payable multisig account.
Full-table acceptance results are in [the current validation report](validation.md).

For V1:

```sh
"$CLI" settlement --protocol v1 create-profile --plan ./release-v1/plan.json \
  --l1-snapshot ./release-v1/l1-snapshot.json --multisig-config 0xREFUND_GOVERNANCE_CONFIG \
  --fee-shannons 100000 --confirmations 24 --output-dir ./release-v1/profile
```

For V0, use `--protocol v0`, `--authority-lock authority-lock.json` (the approved
standard CKB single-signature refund authority), and
`--bootstrap-auxiliary auxiliary.json` (the two current PoA helper cells and two
producer cells). These are exact live `Cell` objects, not signer addresses.
The CLI validates the pinned PoA bootstrap shape. Both producers must authorize
the bootstrap. This is separate from the code cell's 3-of-5 upgrade.

Publish `profile.bin` as cell data using externally funded CKB. It needs no
special type script; its exact data hash is pinned into the validator. An ordinary
`ckb-cli tx` transfer can use `tx add-output --to-data-path profile.bin`, together
with the appropriate funding input, destination and occupied capacity. Sign and
confirm this publication with its funding wallet. Record the resulting outpoint
and data hash. Never fund code/profile publication from user custodians.

Build the selected production validator:

```sh
GW_WITHDRAW_CLI="$CLI" LLVM_STRIP=/path/to/llvm-strip \
  bash scripts/build-settlement-validator.sh ./release-v1/profile ./release-v1/validator v1
# V0: replace both directories and the final argument with v0.
```

The build pins `profile.hash` and strips ELF symbols to avoid locking CKB for
debug/symbol data. Preserve the binary and release metadata together. Calculate
its CKB-personalized data hash and fill `validator_data_hash` in a copy of
`deployment.template.json`; SHA256 is an archive checksum, not the script data
hash. Fill pinned code/profile/config dependency outpoints, exact data hashes,
bootstrap placeholders and live externally funded governance sponsor inputs.
The validator dependency will be the code-upgrade transaction's output 0.

Publish **report + CSV views + policy explanations + plan + profile + binary +
hashes/build recipe** together so another party can reproduce the table, roots
and rules. Editing any approved amounts or authorities requires a new reviewed
release; do not patch a signed transaction's allocation witnesses.

## 3. Build and multisign the code upgrade

Create `upgrade-request.json`:

```json
{
  "protocol": "v1",
  "code_cell": {"tx_hash":"0xCURRENT_CODE_TX", "index":"0x0"},
  "old_data_hash":"0xCURRENT_CODE_DATA_HASH",
  "multisig_config":"0xEXACT_EXISTING_CODE_MULTISIG_PREIMAGE",
  "funding_inputs":[{"tx_hash":"0xEXTERNAL_FUNDING_TX", "index":"0x0"}],
  "dependencies":[
    {"out_point":{"tx_hash":"0xMULTISIG_CODE_TX","index":"0x0"},"dep_type":"code","data_hash":"0xPINNED_HASH"},
    {"out_point":{"tx_hash":"0xSECP_DATA_TX","index":"0x0"},"dep_type":"code","data_hash":"0xPINNED_HASH"}
  ],
  "fee_shannons":1000000
}
```

The values above are placeholders. Fetch and hash the actual mainnet cells. Use
explicit code dependencies, including the secp data cell. Funding cells must be
plain CKB under the *same existing code multisig*. The tool preserves the code
TYPE_ID and lock, checks the old/new data hashes, computes occupied capacity,
returns change and enforces the minimum 1000-shannon/KB fee baseline.

```sh
"$CLI" upgrade build --request upgrade-request.json --deployment deployment.json \
  --binary ./release-v1/validator --output-dir ./upgrade-unsigned
```

Output: `skeleton.json`, raw `transaction.json`, and `signing-request.json`.
These raw transactions are **not** `ckb-cli tx` helper files. Use the installed
and tested `ckb-cli 1.16.0` raw-digest signing interface:

```sh
# Each member independently reviews the upgrade skeleton, deployment and binary.
# ckb-cli signs only the supplied digest; it does not verify the request or transaction.
# On the signing station, rebuild the upgrade from those reviewed inputs and compare
# its skeleton and signing-request with these copies before copying the digest.
ckb-cli --local-only --output-format json util sign-message \
  --message 0xSIGNING_DIGEST_FROM_REQUEST --recoverable \
  --from-account YOUR_CKB_CLI_ACCOUNT > signer-result.json
"$CLI" settlement --protocol v1 import-signature \
  --request ./upgrade-unsigned/signing-request.json --lock-hash 0xLOCK_HASH \
  --ckb-cli-output signer-result.json --output signer-response.json
"$CLI" upgrade assemble-signatures --skeleton ./upgrade-unsigned/skeleton.json \
  --signatures signer-1.json signer-2.json signer-3.json --output upgrade-signed.json
"$CLI" upgrade verify-signed --skeleton ./upgrade-unsigned/skeleton.json \
  --deployment deployment.json --transaction upgrade-signed.json --output upgrade-verified.json
```

Supply exactly the required distinct signatures, including any require-first-N
members. The example's number of filenames is illustrative; use the actual
config. `--privkey-path` is also supported by ckb-cli for an offline signing
station. Never use `sign-data` for these digests: it hashes the supplied data again.

Verification rebuilds the upgrade from live cells, checks that the signed raw
transaction is unchanged, and requests actual script execution with a 70M-cycle
limit. It does not broadcast. Submit only the exact verified JSON, for example
with CKB `send_transaction` and the `passthrough` output validator via your normal
RPC tool. Save the transaction hash before submission; after a lost response,
query that hash before retrying. Wait for the configured confirmations, then
record output 0 as the new validator dependency in the deployment manifest.

## 4. Initialize roots once, then refund only applicants

Use `settlement build-next` once without recipients:

```sh
"$CLI" settlement --protocol v1 build-next --db /path/to/v1-db \
  --plan plan.json --deployment deployment.json --output-dir step-001
```

This single Begin transaction commits the release profile (one initial state root,
manifest and policy), activates its epoch record and initializes per-asset
budgets. It does not register every source, reserve or beneficiary. After Begin
is confirmed, only requested payments are submitted. Each payment carries
membership proofs for its claim, source and INITIAL lot. Source/lot counters are
created on first payment and updated thereafter; all unpaid accounts stay off-chain.

The CLI reconstructs confirmed settlement history and verifies the current rollup
root before every step. Keep the original DB and complete plan unchanged.

For each step, review the deployment and complete skeleton. The optional
`gw-withdraw` offline signer requires both alongside the request and refuses a
request whose transaction hash, signing group, message or digest differs from
what the skeleton and deployment produce:

```sh
"$CLI" settlement --protocol v1 sign-message \
  --request step-001/signing-request.json --deployment deployment.json \
  --skeleton step-001/skeleton.json --lock-hash 0xLOCK_HASH \
  --private-key-file /path/to/offline-key --output response-1.json
```

For a capacity-funded Pay, the offline signer must also receive its own reviewed
`--capacity-funding capacity-funding.json`. It rechecks the actual transaction
against that account and maximum before reading the private key. If using
`ckb-cli`, make the same comparison outside `ckb-cli` before handing it a digest.

For a `ckb-cli` signer, independently run `settlement signing-requests` on the
signing station using the reviewed deployment and skeleton, compare the complete
result to the supplied `signing-request.json`, then copy the locally verified
digest into `ckb-cli`. `ckb-cli` cannot perform this check itself. Import/assemble
signatures and run:

```sh
"$CLI" settlement --protocol v1 assemble-signatures --deployment deployment.json \
  --skeleton step-001/skeleton.json --signatures response-1.json response-2.json \
  --output step-001/signed.json
"$CLI" settlement --protocol v1 verify-signed --db /path/to/v1-db \
  --plan plan.json --deployment deployment.json --transaction step-001/signed.json \
  --output step-001/verified.json
```

V1 governance uses its configured CKB multisig; V0 uses the pinned single-signature
CKB authority after bootstrap. V0 producer ETH approvals use the listed
`ethereum_personal_sign` payload as **32 bytes**, not the ASCII hex text. ETH
wallets add the Ethereum prefix themselves. ckb-cli's `sign-message` signs the
already prefixed `signing_digest`; MetaMask signs `message` using `personal_sign`.

After confirmation, use `next-deployment-after-confirmation.json` for the new
sponsor outpoint and build the next step. Do not advance this file on a mere
broadcast acknowledgment. Current state is always reconstructed from chain;
local step files do not authorize replay or bypass paid counters.

After this one initialization is confirmed, create `addresses.json` containing a JSON array of ETH owners:

```sh
"$CLI" settlement --protocol v1 build-next --db /path/to/v1-db \
  --plan plan.json --deployment deployment.json --addresses addresses.json --output-dir payout-001
```

The result pays standard ETH OmniLock cells, with exact sUDT types/data and
sufficient occupied CKB. It updates L2 sources, custodian balances and paid state
together. Sign/verify/submit using the same workflow. Users subsequently sign an
L1 OmniLock transfer in the portal to their chosen valid CKB mainnet address.

## 5. Control-account refunds and partial batches

A control payout goes directly to the jointly approved CKB destination. Each
participating ETH controller supplies **one live, plain mainnet ETH OmniLock
cell**; all these cells are returned byte-for-byte unchanged. They are proof of
authority, not a subsidy or a refund fee. A plain ETH OmniLock cell occupies at
least 63 CKB. Controllers can use suitable existing cells or fund their own
beforehand. Duplicate owners do not count twice; 2-of-4 and 3-of-6 remain intact.

Create `controls.json`:

```json
[
  {
    "account_id":263,
    "destination":"ckb1...",
    "authorization_inputs":[
      {"tx_hash":"0xOWNER_ONE_LIVE_CELL", "index":"0x0"},
      {"tx_hash":"0xOWNER_TWO_LIVE_CELL", "index":"0x0"}
    ]
  }
]
```

```sh
"$CLI" settlement --protocol v1 build-next --db /path/to/v1-db \
  --plan plan.json --deployment deployment.json --controls controls.json --output-dir control-001
```

Each consumed controller lock needs a signature; choose the intended threshold
subset before building. Any change to destination, amount, inputs, fee or paid
state requires rebuilding and collecting fresh signatures. Governance alone
cannot redirect a control refund. A project account without a committed payable
claim cannot enter this flow by presenting its administrator's key.

For partial payouts or beneficiaries with more than 64 source claims, first run
`settlement status` with DB/plan/deployment and `--output status.json`. Its `claims`
array lists source, asset, account, cap, paid amount, remaining amount and claim
key. Select up to 64 records in `payments.json`:

```json
[{"claim_key":"0xCLAIM_KEY", "amount_raw":"POSITIVE_L2_INTEGER"}]
```

Pass `--payments payments.json` to `build-next`, and `--controls controls.json`
when applicable. Do not combine it with `--addresses`. The builder checks each
remaining cap and duplicate key. Combined V1 CKB must be a whole number of L1
shannons. The default address/control selection takes all remaining claims and
refuses an oversized batch rather than truncating it.

If a payout needs more CKB than the beneficiary's payable native amount provides
for occupied cell capacity, the builder reports the required top-up and stops.
For a release built with the reviewed capacity-funding validator, provide a
`capacity-funding.json` file:

```json
{"account_lock_hash":"0xGOVERNANCE_LOCK_SCRIPT_HASH", "max_spend_ckb":"20.00000000"}
```

The lock hash must match the governance lock in the pinned profile. This Pay
version accepts only that account as an external capacity funder; its live plain
CKB inputs come from `deployment.json`'s `sponsor_inputs`. The maximum is the
account's **total** spend for this transaction, including the configured fee.
Use `--capacity-funding capacity-funding.json` with `build-next`. The builder
adds only the minimum missing payout capacity, rejects a spend above the maximum,
and returns the remainder to the governance lock. Review
`skeleton.json` → `allocations.capacity_funding` and each recipient's
`capacity_top_up_ckb` / `capacity_top_up_shannons`; the same summary appears in `progress.json` and
on standard output. `verify-signed` independently lists the actual top-up, fee,
and total governance spend before broadcast. After confirmation, compare the
confirmed transaction with that verified hash and use the recorded sponsor
change output index/outpoint for the next batch. `next-deployment-after-confirmation.json`
uses this exact output for Pay transactions.
After the configured confirmation depth, `settlement status` independently
replays confirmed Pay transactions and lists each funding result under
`confirmed_capacity_funding` with its transaction hash.
The builder also prints `future_capacity_warnings` when the selected partial
payment leaves less native CKB than the remaining claims would need for their
minimum future payout cells at the current destination. Review this warning
before requesting signatures and plan another funded batch if needed.

Capacity funding changes validator consensus rules. The fixed audited binary
cannot accept these transactions: build, review, and bind a new validator binary
and profile before using this option. Keep enough unpaid CKB for later token
batches, or plan their own funded payouts. Authorization cells cannot be silently
spent as top-up.

## 6. Run the web portal

Export the checksummed table and plan for each version:

```sh
"$CLI" settlement --protocol v1 export-portal --plan plan.json \
  --tables ./release-v1/tables --output-dir ./portal-v1
npm ci --prefix claim-web
npm run build --prefix claim-web
"$CLI" claim-serve --tables ./portal-v1 --plan ./portal-v1/plan.json \
  --deployment deployment-v1.json --v0-tables ./portal-v0 --v0-plan ./portal-v0/plan.json \
  --v0-deployment deployment-v0.json --dependencies claim-dependencies.json \
  --web-dir ./claim-web/dist --control-proposals ./control-proposals \
  --listen 127.0.0.1:8787 --public-origin https://YOUR_REFUND_DOMAIN \
  --body-timeout-secs 15
```

Use the same explicit pinned mainnet OmniLock/sUDT/secp-data dependencies as the
release. Configure your HTTPS reverse proxy and exact public origin. Start
without `--enable-broadcast` for review; add it when operating the refund service.
`--body-timeout-secs` limits the time spent reading a JSON body (1–120 seconds;
default 15). A body read timeout returns HTTP 408 and closes that connection.
The underlying client socket also has an idle I/O timeout of at most 3 seconds.
At the reverse proxy, enable request buffering, a request body timeout, and
connection limits; these controls also cover slow headers and requests that
stall before the application receives a complete body.
The service never holds signing keys. Omitting the V0 options disables V0 instead
of falling back to V1. Personal cells must have unmixed provenance in the selected
release; arbitrary cells under the same OmniLock are not silently included.

Each web proposal must pay exactly one control recipient. For a control proposal, collect only Ops/governance signatures offline, then:

```sh
"$CLI" settlement --protocol v1 publish-control-proposal --plan plan.json \
  --deployment deployment.json --skeleton control-001/skeleton.json \
  --signatures ops-1.json ops-2.json \
  --output ./control-proposals/v1/0xTRANSACTION_HASH.json
```

The contract tab displays the reviewed owners/threshold and every reported asset.
Controllers select the account and load the proposal's transaction hash, review
the outputs and sign with MetaMask or another ETH wallet. Each signer downloads
an approval JSON; co-signers exchange/import these files against the **same**
proposal. Duplicate, outsider and different-transaction approvals are rejected.
The browser derives the OmniLock signing message from the actual transaction and
checks that its authorization cell is returned intact. The server rechecks live
inputs, verifies all signatures, executes all scripts, then submits the exact tx.

After a partial control refund, a new proposal is required for the remaining
rights. The old rollup input has been consumed. A lost submission response keeps
the original hash visible; check its status before another attempt. Protocol,
account-mode or wallet changes discard the active signing context. EOA partial
OmniLock transfers preserve change under the same owner; over-remaining spends
are rejected by the actual lock/type scripts, not just the UI.

## Verification and release evidence

See [the current validation report](validation.md) for exact suites, real
script cycle measurements, full-table DB checks and browser scope. Synthetic
fixtures use public test keys; no production signature or mainnet broadcast is
part of acceptance. Generated acceptance plan IDs are disposable test IDs.


## Continuing after a completed batch

Keep the original DB and plan. Reconstruct confirmed history and verify the live root before every next batch, using fresh sponsor outpoints. See [contract design](contract-design.md) for cold-process rehearsal. Do not build competing transactions spending the same rollup cell.

## Report format and independent queries

`refund generate` takes the DB as its only external calculation input. Code/layout identities, approved exclusions and attribution rules are embedded in the CLI. Unknown roots, changed authority, incomplete receipt supply or unreviewed funded residues fail closed. V1 uses authenticated offline EVM getters and frozen on-chain prices; it makes no RPC calls during generation.

Canonical JSON is `report.json`; CSVs are views. Outputs include accounts, capacity, claim sources, source reserves, asset totals and authority records. V1 also includes retained lots and supplementary unregistered native balances. Historical tables are comparison inputs only, never generation inputs.

`l2-db` lists native registered balances and asset definitions; it does not itself enumerate every nonstandard destination or protocol receipt. `l1-custodian` captures current cells independently, or accepts `--snapshot` for explicitly historical inspection. Live captures are non-atomic single-provider observations. `check-backing` returns 1 on insufficient backing/supply mismatch, 2 for review with census limitations, and 3 on operational errors; consult the report rather than treating every nonzero status as capture failure.

The generation-time capacity estimate is 63 CKB per plain output or 144 CKB per nonzero sUDT output, excluding network fees. It is not an exact quote for arbitrary controller destinations. Actual transaction building enforces actual occupied capacity. Capacity-blocked rights are included in theoretical totals, not represented as immediately executable payouts.

## V3 state-format upgrade

The current source uses one settlement `state_root` and V3 plans/profiles/actions. Only this format is supported; no compatibility or migration workflow is provided. Regenerate the complete plan/profile, independently verify its static account-page root and rebuild the pinned validator. Existing deployment/governance and multisig upgrade requirements remain unchanged.

64 records / 20 recipients are format ceilings. Verify the entire signed transaction against the actual node cycles policy and split a batch when necessary. Keep a recipient's CKB and token payments together to fund occupied capacity. See [batch measurements](validation.md#batch-size-and-cycles) for reproducible benchmarks and observed limits.

V3 keys and values both use V3 commitment domains. Plan/profile/action/native-payload/signing-skeleton versions are 3; V0 profile bytes use the `GWV0SET3` marker. Do not mix separately generated release artifacts. Status reports expose the current `state_root` and paid-claim count; registration-phase counters no longer exist.
