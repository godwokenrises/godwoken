# Refund validation

Current code supports settlement V3 on native networks V0 and V1. It uses one settlement SMT with bounded account pages and the unchanged upstream sparse-merkle-tree 0.6.1. The native Godwoken account root remains separate. See [contract design](contract-design.md) for encoding and write restrictions, and [operator runbook](ops-refund-runbook.md) for release procedures.

## Executed checks — 16 October 2026

- 89 contract tests and 32 host library tests.
- 13 V0 and 13 V1 regression cases; four opt-in V0 artifact/benchmark cases are excluded from the regular run and exercised separately below.
- Signed V0/V1 controller transactions, onward OmniLock spends and actual ckb-cli multisig upgrades.
- 47 frontend tests and a production build. Controller fixtures were regenerated from V3 VM runs.
- Signed V0 batch/paging tests: exact cumulative payments, unchanged unrelated state, rejection of static-record mutations and arbitrary output roots. Re-signed corrupt, missing, reordered, duplicated and wrong-count witness chunks are rejected by the type script.

## Batch size and cycles

Measurements include complete signed V0 transactions and actual CKB-VM execution, including authority/custodian locks and payout cells. Tested budgets: **70,000,000 cycles**, **512,000 transaction bytes**, **32,768 bytes per witness**. Actions are split into 16,000-byte witness chunks. The 70M budget is a CLI/test admission policy, not a consensus-wide per-transaction cap.

The fixture has 20 target source accounts, 20 recipients, two assets (CKB and one token), 51 claims and 20,000 background accounts. “Mature” means synthetic authenticated paid/reserve/source counters and native balances; it does not mean replaying 20,000 historical transactions.

| State | Selection | Records | Cycles | Transaction bytes |
| --- | --- | ---: | ---: | ---: |
| Fresh | 20 recipients, CKB only | 20 | 60,193,973 | 88,299 |
| Fresh | 20 recipients, 40 selected records (two assets) | 40 | > 70M (rejected) | 96,404 |
| Fresh | 20 recipients, all claims | 51 | > 70M (rejected) | 100,178 |
| Fresh | First 10 recipients, all assets | 25 | 41,568,489 | 54,110 |
| Fresh | Second 10 recipients, all assets | 26 | 42,702,427 | 55,922 |
| Mature | 20 recipients, CKB only | 20 | 58,778,654 | 85,496 |
| Mature | 20 recipients, 40 selected records (two assets) | 40 | 69,604,607 | 93,665 |
| Mature | 20 recipients, all claims | 51 | > 70M (rejected) | 97,274 |
| Mature | First 10 recipients, all assets | 25 | 40,781,256 | 52,365 |
| Mature | Second 10 recipients, all assets | 26 | 42,013,692 | 54,796 |

For this fixture, use two ten-recipient transactions to pay all assets. They execute sequentially and fully drain all 40 target source/asset balances and all 51 claim caps. The mature 40-record transaction has less than 0.6% cycle headroom and does not cover all 51 claims. The failed single-transaction checks leave state unchanged.

A separate fixture groups all 51 claims under one source account: seven account pages plus epoch and two asset leaves. It pays 20 recipients in **43,338,880 cycles / 60,968 bytes**, using a 610-byte settlement proof, and fully drains the claims. Begin costs 10,165,560 cycles in the main fixture and 10,184,971 in the paged fixture. Main-fixture settlement proofs are 754 bytes for CKB-only and 901/903 bytes for the two full-asset batches; transaction sizes also include native proofs and account-page preimages.

The protocol ceilings are 64 payment records and 20 recipients. Actual feasibility depends on assets, page contents, controller locks, custodian fragmentation and proofs. Pre-execute the exact signed transaction under the target node policy.

## Reproduce

From `exit-withdraw/`, with Rust 1.94.0, the RISC-V target, clang, llvm-ar, llvm-strip, jq and ckb-cli available:

```sh
cargo +1.94.0 test --locked --lib
cargo +1.94.0 test --manifest-path ../gwos/contracts/exit-withdraw-state-validator/Cargo.toml --locked --lib --tests
bash scripts/test-v0-settlement-e2e.sh
bash scripts/test-settlement-e2e.sh
bash scripts/test-control-settlement-e2e.sh
cargo +1.94.0 test --locked --offline --features e2e-tests --test withdraw_e2e multisig_upgrade -- --nocapture
GW_LLVM_STRIP=llvm-strip bash scripts/test-unified-batch-e2e.sh
GW_LLVM_STRIP=llvm-strip bash scripts/test-unified-batch-e2e.sh paged
npm test --prefix claim-web
npm run build --prefix claim-web
```

VM scripts build synthetic profile-pinned binaries. The batch scripts print `UNIFIED_BENCH` records with cycles, transaction size, leaf count and proof size; retain the output with any release measurement. Strip binaries before recording their deployment code hashes, as in the production build. Release Rust uses optimization level 3 with overflow checks enabled. Optional environment-dependent cases count as VM coverage only when the required artifacts are supplied.

## Test JSON files

| File | Purpose |
| --- | --- |
| [control-v0-fixture.json](../claim-web/tests/control-v0-fixture.json), [control-v1-fixture.json](../claim-web/tests/control-v1-fixture.json) | Synthetic VM-accepted controller proposals, transactions and test-key signatures; used by frontend signature and tampering tests. Regenerated by `test-control-settlement-e2e.sh`. |
| [settlement-v3-vectors.json](../../gwos/contracts/exit-withdraw-state-validator/tests/fixtures/settlement-v3-vectors.json) | Fixed record encodings, keys and commitments checked by `settlement_records.rs`. Detects unintended protocol encoding changes. |

## Scope and earlier evidence

These fixtures use public test keys. They are not production releases or evidence of a mainnet payout. A release needs freshly generated, independently checked V3 plans/profiles and profile-pinned binaries, plus the existing upgrade authorities.

The external browser scenario suite and original-DB release export were not rerun for V3. Earlier browser runs used injected wallets/intercepted APIs; a real-wallet-extension-to-live-node acceptance has not been performed. The Rust HTTP service with VM-backed RPC is exercised separately by the V1 controller test.

Earlier V0 cold-process checks read 770,736 native leaves and a 24,483-claim plan, then paid two selected applicants using real DB proofs and synthetic L1 authorities. [Host evidence](../tests/fixtures/root-only/acceptance.json) and [signed VM evidence](../tests/fixtures/root-only/signed-vm.json) are historical results for the prior format, not V3 acceptance. Old replay archives have been removed. Use [cold-process rehearsal](contract-design.md#cold-process-rehearsal) to generate fresh context.

Financial-table verification is recorded in each network's asset manifests and refund regeneration records. The [V1 policy comparison](v1/refunds/policy-comparison.json) checks whole-YOK exclusion: other entitlements remain unchanged, YOK payouts are zero and V0 outputs are byte-identical. These historical calculation checks do not validate a V3 deployment. Machine-specific paths belong in the machine inventory.
