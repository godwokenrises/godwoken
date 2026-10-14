# Root-only acceptance — 14 October 2026

The current lifecycle is **Begin → Pay → Pay …** for both V0 and V1. No full-table
registration transactions are required. Monetary tables and their source/lot/claim
roots were not changed. See [design](contract-design.md) and
[Ops instructions](ops-refund-runbook.md).

## Executed checks

- V1 regression: 13 passing cases, including an actual CLI subprocess lifecycle
  `Begin, Pay, Pay`. The original DB stays unchanged; each command replays history.
- V0 regression: 11 passing cases. Dedicated actual-VM adversarial test also passed,
  including missing/forged source, lot, claim and mutable-state proofs, legacy
  operation rejection, stale paid state, excessive second payment and wrong outputs.
- Two dedicated controller VM tests passed with stripped binaries, including
  repeated payouts and onward OmniLock spends. Maximum observed controller payout:
  **31,715,934 cycles** (V1); both versions remain below 70M.
- Actual ckb-cli multisig upgrade tests: two passed.
- Frontend: 47 unit tests and production build passed; V0/V1 controller browser
  flows passed using freshly generated VM-accepted root-only transaction fixtures.
  Includes protocol/tab isolation, approval exchange, rejected signing, duplicate
  submission guard and mobile layout. Real HTTP/API rehearsal is part of the V1
  controller test and uses a local VM-backed RPC.

Regression counts include environment-selected artifact/control cases; dedicated
runs above supply the corresponding binaries rather than assuming a skipped case
executed VM code. The optional real-DB VM test below is reported separately.

## Original V0 DB, cold CLI processes

The original V0 DB was opened read-only in a separate process for each operation.
Each invocation verified all 770,736 native leaves before operating. The complete
current plan contains 24,483 claims, but only two selected EOA applicants were paid.
The plan, DB and source baseline remained unchanged; no SMT-node cache or trusted
local paid-counter file was reused.

1. Initialize once: history 0 → 1.
2. First applicant, partial payment: history 1 → 2.
3. Restart; pay its remaining rights: history 2 → 3.
4. Restart; repeated request for that applicant is rejected.
5. Restart; pay a distinct new applicant: history 3 → 4.

All roots linked correctly; all L2-source/custodian coverage checks passed.
[Cold-process evidence](../tests/fixtures/root-only/acceptance.json) contains exact roots, source/custody
coverage, selected addresses and operation counts. This part uses synthetic L1
history and unsigned host consensus, with no network submission.

## Signed VM execution of the real-DB proofs

The same four saved transitions were then equipped with actual script dependencies,
signed using public test-key **L1 governance/PoA authorities**, and executed in CKB
VM. After changing dependency outpoints, successor input references were remapped
to the preceding signed transaction hash, and the exact predecessor output bytes
were checked. Every signed transaction was inserted into the simulated chain;
replaying its consumed inputs failed. Beneficiary private keys were never used.

| Transition | Cycles | Signed bytes |
|---|---:|---:|
| Begin | 30,086,145 | 4,845 |
| Partial Pay | 30,224,077 | 21,860 |
| Remainder Pay | 29,925,629 | 17,456 |
| New applicant Pay | 20,311,245 | 17,568 |

[Signed-chain results](../tests/fixtures/root-only/signed-vm.json) record the exact before/after roots. These
are actual original-DB Merkle proofs and real beneficiaries/balances, with synthetic
L1 funding and test authority keys. Real recipients' onward withdrawals are not
signed by this test; onward OmniLock spending is covered by the separate synthetic
EOA/controller end-to-end fixtures. No production keys, deployment or broadcast.

## Reproduce the real-DB proof VM test without copying the DB

From `exit-withdraw/`, configure LLVM clang/ar/libclang and build the CLI first:

```sh
FIXTURE="$PWD/tests/fixtures/root-only/v0-vm-fixture"
GW_WITHDRAW_CLI="$PWD/target/debug/gw-withdraw" LLVM_STRIP=/path/to/llvm-strip \
  bash scripts/build-settlement-validator.sh "$FIXTURE" /tmp/v0-cold-validator v0
GW_V0_COLD_ARTIFACTS="$FIXTURE" GW_V0_SETTLEMENT_BINARY=/tmp/v0-cold-validator \
  cargo +1.94.0 test --locked --offline --features e2e-tests \
  --test withdraw_e2e v0_real_db_cold_artifacts_signed_vm -- --nocapture
```

The small `archive.json` in this fixture directory is deliberately only VM context;
it is not accepted as a `rehearse-next` archive. To repeat cold DB reconstruction,
use the commands in the design document with the actual DB, complete plan and a
new synthetic rehearsal directory. Host operating paths remain in the machine
inventory. Use fresh filenames for builds; the production build script refuses
replacement of existing release files.

## Portal coverage and boundary

Personal browser scenarios cover wallet discovery, every asset row, invalid network, account change, signature rejection, confirmation, partial spend, remaining balance, excessive second request, final spend and mobile layout. V0/V1 controller scenarios cover tabs/protocol isolation, proposal review, signer approval exchange and duplicate submission.

Browser scenarios use injected test wallets and intercepted APIs, not the actual MetaMask extension. The real Rust HTTP service with VM-backed RPC is separately exercised by the V1 controller test. A single real-wallet-extension-to-live-node end-to-end acceptance has not been performed. Original beneficiaries' private keys are not available; onward OmniLock spends use synthetic test beneficiaries.

From `exit-withdraw/`, use `scripts/test-settlement-e2e.sh`, `scripts/test-v0-settlement-e2e.sh`, `scripts/test-control-settlement-e2e.sh`, and `cargo +1.94.0 test --locked --offline --features e2e-tests --test withdraw_e2e multisig_upgrade -- --nocapture`. Frontend: `npm test --prefix claim-web`, `npm run build --prefix claim-web`. Browser scenario generators are `claim-web/tests/{browser,partial-browser,control-browser}-rehearsal.mjs`; follow their fixture/server requirements. Optional VM cases require their environment variables; an early-return case is not evidence of VM execution.

Historical VM inputs and compact execution evidence live under `tests/fixtures/`, separate from current financial tables. Reproducing those historical transactions does not require replacing the current tables with an old release.

## Documentation reorganization checks (14 October 2026)

After moving executable test inputs from docs to tests/fixtures, all Rust targets compiled. The versioned integration scripts passed (V1: 13 reported cases; V0: 12, including environment-selected cases). Both dedicated controller VM tests also passed with actual binaries. Ordinary unit/audit/inventory suites passed. The generic unconfigured e2e command initially failed cases requiring VM binaries; the versioned scripts supply those binaries. Optional real-DB/controller tests are not counted as executed merely because a case returns without its environment.

The two complete DB regenerations and live L1 observations are separately recorded under each version's assets/manifest.json and refunds/regeneration.json. They do not depend on historical tables; comparison occurs only after generation. No mainnet deployment or payout is implied.

The documentation cleanup also reran the real personal HTTP/VM test successfully. The dual-protocol smoke script now accepts both exported portal directories explicitly, instead of opening a hardcoded historical release. Its two-current-package run verifies 54 V0 / 48 V1 asset definitions, separate manifests, CKB decimals, rejected cross-protocol requests and disabled broadcasting.

After the old documentation directories and superseded accounting tool were actually removed, 63 maintained unit/CLI/audit/inventory tests passed (30 + 2 + 23 + 5 + 3). All current document links, JSON files and published checksum manifests validate; no published table is gitignored. At that documentation-cleanup revision, fresh V0 and V1 complete reports were byte-identical to their previous final outputs. The subsequent YOK policy revision is documented below. Each registered source matches the independent fresh L2 query, and all planned asset payments fit the new finalized custodian observations. V0 backing review returns 2 for its census scope; V1 returns 1 for the already documented recorded-supply mismatch, not a capture error.


## Whole-YOK exclusion verification — 14 October 2026

The production CLI was rebuilt after excluding V1 asset 128 (YOK). It regenerated V1 and V0 tables from their original read-only databases, with no historical report supplied to generation. Old reports were used only for independent comparison. See [policy comparison](v1/refunds/policy-comparison.json) for exact hashes, asset deltas and audit-only plan roots.

- Every non-YOK beneficiary entitlement and payable amount, claim source, source partition, retained lot and authority matched the previous V1 result exactly.
- All V1 YOK payments are zero; its known balances remain in the retained and custodian reconciliation. No non-YOK right was redistributed.
- Capacity-ready fields and top-up requirements were recalculated after removing YOK outputs. These can improve even though non-YOK monetary entitlements do not change. Accounts removed from the beneficiary list had only YOK entitlements.
- Every V0 generator output was byte-identical to its previous counterpart.
- Source balances were independently matched to the published authenticated L2 inventory. All planned payments fit finalized custody. A fresh V1 L1 capture found the same rollup account root, custody totals and finalized amounts as the published observation; its raw evidence is retained as [custody-recheck.json.gz](v1/assets/custody-recheck.json.gz).
- Both old and new V1 plans were exported under the same explicitly audit-only deployment ID; the new plan passed `settlement verify-plan`. These are comparison commitments, not a production deployment or initialized refund root. Production release plans, profiles and profile-bound validator binaries must be generated from the new report.
- 64 local library/CLI/audit/inventory tests passed, including a regression rejecting any payable YOK allocation and checking that retained YOK adds no payout capacity requirement. Contract verification logic and web behavior were not changed by this policy revision.

No original database write, production signature, contract deployment or payment was performed. Existing recorded-supply discrepancies and approved holds remain unchanged.
