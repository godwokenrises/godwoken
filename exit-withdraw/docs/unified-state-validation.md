# Single settlement root — 16 October 2026

The V3 implementation combines all eight logical settlement trees into one physical SMT. Account pages group immutable source, lot and claim records with their mutable counters. Asset counters and the initial epoch use separate namespaces in the same tree. The native Godwoken account SMT remains a separate tree and commits the settlement metadata.

```text
GlobalState.account.merkle_root        (existing native SMT)
  +-- native identities / balances / supply
  +-- settlement metadata
        +-- state_root                (one settlement SMT)
              +-- account(account_id, page_number)
              |     +-- immutable Source / Lot / Claim
              |     +-- mutable Sources / Reserves / Paid
              +-- asset(asset_id): mutable cumulative Paid
              +-- epoch(0): initialized once by Begin
```

The upstream sparse-merkle-tree 0.6.1 algorithm, compiled proof format and hash rules are unchanged. Chain verification uses its C backend; host generation and verification use its Rust backend. The adapter only supplies sorted unique leaves to the upstream API. Release Rust optimization is `opt-level = 3`, with overflow checks retained.

## Why keep this layout

A controlled prototype comparison used the same logical-key layout, upstream C verifier, optimization level, 20 source accounts, 20 recipients, two assets, 51 claims and 20,000 background accounts. Each payment batch completed all assets for ten recipients:

| Fresh state / batch | Six-root account grouping | One physical root | Reduction |
| --- | ---: | ---: | ---: |
| First 25 records, cycles | 43,123,130 | 40,590,707 | 5.87% |
| First 25 records, transaction bytes | 59,464 | 54,842 | 7.78% |
| Second 26 records, cycles | 44,453,913 | 41,756,466 | 6.07% |
| Second 26 records, transaction bytes | 61,438 | 56,786 | 7.57% |

This comparison motivated implementation. Production V3 uses full collision-resistant logical key hashing and full lot IDs, removes redundant root fields, adds deterministic bounded paging, and explicitly versions the protocol. Its measurements below are a separate run; they are not an exact comparison against the older prototype binary.

There is a tradeoff: reading one asset requires the authenticated account page, which can contain other assets. In the controlled CKB-only 20-recipient prototype, six roots used 56,714,132 cycles / 73,739 bytes, while one root used 59,469,383 / 89,130. Fewer roots do not guarantee lower cost for every selection.

## Production V3 measurements

Measurements use complete signed V0 transactions and actual CKB-VM script execution, including governance/PoA authority, custodian locks and payout cells. The transaction budget is 70,000,000 cycles. Tests assert serialized transaction size <= 512,000 bytes and every witness <= 32,768 bytes. The largest observed witness is 16,000 bytes.

The fixture has 20 target source accounts, 20 recipients, CKB plus one token, 51 claims and 20,000 additional funded source accounts. The mature fixture gives all background claims consistent authenticated paid/reserve/source counters and native balances. It is a synthetic input state, not a replay of 20,000 historical transactions. These are development fixtures, not mainnet transactions or a claim that every real applicant combination fits.

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

Begin uses 10,165,560 cycles. The settlement compiled proof is 754 bytes for the 20-person CKB selection and 901/903 bytes for the two full-asset batches. These figures exclude the native-state proof and authenticated page preimages, which are included in the transaction bytes above.

The separate paged-account VM test groups all 51 claims under one source account: seven account pages plus epoch and two asset leaves, one state root, a 610-byte settlement proof, **43,338,880 cycles / 60,968 transaction bytes** for 20 recipients. It completes both source/asset balances and all claim caps. Its independently pinned Begin uses 10,184,971 cycles.

[Machine-readable production measurements and artifact hashes](unified-state-results.json) preserve the exact successful and budget-rejected results.

The 40-record mature-state selection passes at 69,604,607 cycles, leaving less than 0.6% headroom; its fresh-state counterpart fails. The 51-record single transaction fails in both states. These diagnostic preflights do not change fixture state. The two ten-recipient payments execute sequentially and completely drain all 40 target source/asset balances and all 51 claim caps. The tests also verify that target recipients have no remaining L2 balance.

The 70M budget is an admission policy used by this CLI and test VM; it is not a consensus-wide fixed per-transaction cycles limit. Real node policy, recipient controllers, asset count, custodian fragmentation and proof shape can change feasibility. Pre-execute the exact signed transaction before submitting it.

## Write authorization and paging

- Clients submit claim references and amounts, old authenticated state preimages and compiled proofs. They cannot submit an arbitrary write map.
- `Trees::set` accepts only `PAID`, `RESERVES`, `SOURCES`, `ASSETS`; epoch insertion is permitted only for Begin. Only Begin and epoch-0 Pay are admitted by this release.
- The evaluator computes every new counter from the payment, authenticated static records and checked budgets. It starts with the complete old account page and preserves every other entry. The declared output metadata/root must match independently recomputed results.
- Static source/lot/claim mutation, an arbitrary output root and an unrelated token counter mutation are rejected in signed VM tests. A subsequent CKB-only payment preserves unrelated token counters byte for byte.
- Pages are deterministic and frozen before publication: at most 16 static records plus their corresponding mutable slots, 32 entries total and 8 KiB encoded. Large accounts span pages; payments never relocate slots. Host tests cover 80 claims spread over 11 pages, immutable preservation and orphan-counter rejection.
- Physical namespaces reject aliases/noncanonical keys. Pages reject duplicate or unsorted entries, unknown kinds, missing static parents, oversized input and trailing bytes.
- Large actions use 16,000-byte extra witnesses and a bounded length/count/full-payload-hash header. Complete signed VM tests re-sign corrupted, missing, reordered, duplicated and wrong-count chunks and require type-script rejection. Unit tests independently check framing bounds and hashes.

## Reproduction and regression scope

From `exit-withdraw`, with Rust 1.94.0, the RISC-V target, clang, llvm-ar, llvm-strip, jq and ckb-cli available:

```sh
cargo +1.94.0 test --locked --lib
cargo +1.94.0 test --manifest-path ../gwos/contracts/exit-withdraw-state-validator/Cargo.toml --locked --lib --tests
bash scripts/test-v0-settlement-e2e.sh
bash scripts/test-settlement-e2e.sh
bash scripts/test-control-settlement-e2e.sh
GW_LLVM_STRIP=llvm-strip bash scripts/test-unified-batch-e2e.sh
GW_LLVM_STRIP=llvm-strip bash scripts/test-unified-batch-e2e.sh paged
```

The batch scripts export synthetic profiles, build the corresponding profile-pinned binaries and run opt-in full VM tests. Their `UNIFIED_BENCH` lines contain exact bytes, cycles, state-leaf count and compiled settlement-proof size. LLVM stripping matches the documented production build; its code data hash must be taken after stripping. All current VM regression scripts strip their protocol-specific binaries in the same way as the release build. Large-state tests optimize the host-only `blake2b-ref` dependency to shorten fixture generation; this does not change the contract compiler settings or VM cycles accounting.

Executed regression checks: 89 contract unit cases, 32 host library cases, 13 V0 cases (four opt-in artifact/benchmark cases ignored in that regular run), 13 V1 cases, and dedicated V0/V1 controller VM flows. The frontend passes 47 unit tests and its production build. Controller browser fixtures are regenerated from the V3 VM runs. The external browser scenario suite and original-DB mainnet release export are not rerun in this cleanup.

Only V3 plan/profile/action/state/native-payload/signing-skeleton encodings are implemented. Both key and value commitment domains use V3, and V0 profile bytes use `GWV0SET3`. Obsolete record kinds, lifecycle fields, compatibility fixtures, standalone direct-account construction and its validator/VM tests have been removed. Shared native invariants run as internal evaluator-transition tests. A release requires newly generated and independently checked V3 artifacts and the existing upgrade authorities. This work makes no deployment, broadcast or authority change.
